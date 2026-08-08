use crate::discover::IndexMode;
use crate::git::{self, GitStatus};
use crate::pipeline::Pipeline;
use crate::store::Store;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

const BASE_INTERVAL_MS: u64 = 5_000;
const MAX_INTERVAL_MS: u64 = 60_000;
const MIN_TICK_MS: u64 = 1_000;
const PROJECT_REFRESH_INTERVAL_MS: u64 = 60_000;

#[derive(Debug, Clone, serde::Serialize)]
pub struct WatcherProjectStatus {
    pub project: String,
    pub interval_ms: u64,
    pub last_dirty_signature: Option<String>,
    pub last_head: Option<String>,
    pub next_poll_in_ms: u64,
}

#[derive(Debug, Clone)]
struct WatchState {
    project: String,
    repo_path: PathBuf,
    last_head: Option<String>,
    last_dirty_signature: Option<String>,
    interval_ms: u64,
    next_poll_at: Instant,
}

pub struct Watcher {
    stop: Arc<AtomicBool>,
    pipeline_busy: Arc<AtomicBool>,
    states: Arc<Mutex<Vec<WatchState>>>,
    next_project_refresh_at: Arc<Mutex<Instant>>,
}

impl Watcher {
    pub fn new() -> Self {
        Self {
            stop: Arc::new(AtomicBool::new(false)),
            pipeline_busy: Arc::new(AtomicBool::new(false)),
            states: Arc::new(Mutex::new(Vec::new())),
            next_project_refresh_at: Arc::new(Mutex::new(Instant::now())),
        }
    }

    pub fn pipeline_busy(&self) -> Arc<AtomicBool> {
        self.pipeline_busy.clone()
    }

    pub fn register(&self, project: &str, repo_path: PathBuf) {
        let mut states = self.states.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = states.iter_mut().find(|state| state.project == project) {
            if entry.repo_path != repo_path {
                entry.repo_path = repo_path;
                entry.interval_ms = BASE_INTERVAL_MS;
                entry.next_poll_at = Instant::now();
            }
            return;
        }

        let last_head = Store::open(project)
            .ok()
            .and_then(|store| store.get_meta("git_head").ok().flatten());
        states.push(WatchState {
            project: project.to_string(),
            repo_path,
            last_head,
            last_dirty_signature: None,
            interval_ms: BASE_INTERVAL_MS,
            next_poll_at: Instant::now(),
        });
    }

    pub fn refresh_from_disk(&self) {
        let now = Instant::now();
        match Store::list_projects() {
            Ok(projects) => {
                for project in projects {
                    self.register(&project.name, PathBuf::from(project.repo_path));
                }
            }
            Err(error) => {
                warn!(%error, "failed to refresh watcher project list");
            }
        }
        self.schedule_project_refresh(now);
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    /// Per-project watcher status for observability.
    pub fn project_status(&self) -> Vec<WatcherProjectStatus> {
        let now = Instant::now();
        self.states
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|state| WatcherProjectStatus {
                project: state.project.clone(),
                interval_ms: state.interval_ms,
                last_dirty_signature: state.last_dirty_signature.clone(),
                last_head: state.last_head.clone(),
                next_poll_in_ms: state
                    .next_poll_at
                    .saturating_duration_since(now)
                    .as_millis() as u64,
            })
            .collect()
    }

    pub fn spawn(
        self: Arc<Self>,
        shutdown: Option<Arc<crate::runtime::Shutdown>>,
    ) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            info!("watcher started");
            while !self.stop.load(Ordering::SeqCst) {
                if shutdown.as_ref().is_some_and(|state| state.is_triggered()) {
                    break;
                }
                let sleep_ms = self.poll_once();
                thread::sleep(Duration::from_millis(sleep_ms));
            }
            info!("watcher stopped");
        })
    }

    fn poll_once(&self) -> u64 {
        self.refresh_projects_if_due(Instant::now());
        let now = Instant::now();

        let mut states = self.states.lock().unwrap_or_else(|e| e.into_inner());
        let mut next_wake = MAX_INTERVAL_MS;

        for state in states.iter_mut() {
            if now < state.next_poll_at {
                let remaining = state
                    .next_poll_at
                    .saturating_duration_since(now)
                    .as_millis() as u64;
                next_wake = next_wake.min(remaining.max(MIN_TICK_MS));
                continue;
            }

            if self.pipeline_busy.load(Ordering::SeqCst) {
                debug!(project = %state.project, "pipeline busy, defer watcher poll");
                let interval_ms = state.interval_ms.max(BASE_INTERVAL_MS);
                schedule_after(state, now, interval_ms);
                next_wake = next_wake.min(interval_ms);
                continue;
            }

            let git_status = match git::status(&state.repo_path) {
                Ok(status) => status,
                Err(error) => {
                    warn!(project = %state.project, %error, "git status failed");
                    apply_backoff(state, now);
                    next_wake = next_wake.min(state.interval_ms);
                    continue;
                }
            };

            if repo_is_clean(state, &git_status) {
                state.last_dirty_signature = None;
                state.last_head = git_status.head.clone();
                apply_backoff(state, now);
                debug!(
                    project = %state.project,
                    interval_ms = state.interval_ms,
                    "repository idle, watcher backed off"
                );
                next_wake = next_wake.min(state.interval_ms);
                continue;
            }

            let changed = collect_changed_files(state, &git_status);
            let signature = status_signature(&git_status, &changed);

            if !should_reindex(state, &git_status, &signature) {
                apply_backoff(state, now);
                debug!(
                    project = %state.project,
                    interval_ms = state.interval_ms,
                    signature = %signature,
                    "dirty set unchanged, watcher backed off"
                );
                next_wake = next_wake.min(state.interval_ms);
                continue;
            }

            if self
                .pipeline_busy
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                schedule_after(state, now, BASE_INTERVAL_MS);
                next_wake = next_wake.min(BASE_INTERVAL_MS);
                continue;
            }

            info!(
                project = %state.project,
                files = changed.len(),
                signature = %signature,
                "watcher triggering incremental reindex"
            );

            let pipeline = Pipeline::new(IndexMode::Full);
            let repo_path = state.repo_path.clone();
            let project = state.project.clone();
            let result = pipeline.run_incremental(&repo_path, &project, &changed);
            self.pipeline_busy.store(false, Ordering::SeqCst);

            match result {
                Ok(index_result) => {
                    if let Ok(store) = Store::open(&state.project) {
                        if let Some(head) = &git_status.head {
                            let _ = store.set_meta("git_head", head);
                        }
                    }
                    state.last_head = git_status.head;
                    state.last_dirty_signature = Some(signature);
                    state.interval_ms = BASE_INTERVAL_MS;
                    schedule_after(state, now, BASE_INTERVAL_MS);
                    info!(
                        project = %state.project,
                        files = index_result.files_indexed,
                        symbols = index_result.symbols_extracted,
                        interval_ms = state.interval_ms,
                        "incremental reindex done"
                    );
                    next_wake = next_wake.min(BASE_INTERVAL_MS);
                }
                Err(error) => {
                    apply_backoff(state, now);
                    warn!(
                        project = %state.project,
                        interval_ms = state.interval_ms,
                        %error,
                        "incremental reindex failed, watcher backed off"
                    );
                    next_wake = next_wake.min(state.interval_ms);
                }
            }
        }

        next_wake.clamp(MIN_TICK_MS, MAX_INTERVAL_MS)
    }

    fn refresh_projects_if_due(&self, now: Instant) {
        let due = {
            let next = self
                .next_project_refresh_at
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            now >= *next
        };

        if due {
            self.refresh_from_disk();
        }
    }

    fn schedule_project_refresh(&self, now: Instant) {
        let mut next = self
            .next_project_refresh_at
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        *next = now + Duration::from_millis(PROJECT_REFRESH_INTERVAL_MS);
    }
}

fn schedule_after(state: &mut WatchState, now: Instant, interval_ms: u64) {
    state.next_poll_at = now + Duration::from_millis(interval_ms);
}

fn apply_backoff(state: &mut WatchState, now: Instant) {
    let interval_ms = next_interval(state.interval_ms);
    state.interval_ms = interval_ms;
    schedule_after(state, now, interval_ms);
}

fn next_interval(current_ms: u64) -> u64 {
    current_ms
        .max(BASE_INTERVAL_MS)
        .saturating_mul(2)
        .min(MAX_INTERVAL_MS)
}

fn repo_is_clean(state: &WatchState, git: &GitStatus) -> bool {
    if git.dirty {
        return false;
    }
    match (&state.last_head, &git.head) {
        (Some(old), Some(new)) => old == new,
        (None, None) => true,
        (None, Some(_)) => false,
        (Some(_), None) => true,
    }
}

fn status_signature(git: &GitStatus, changed: &[String]) -> String {
    let head = git.head.as_deref().unwrap_or("no-head");
    if changed.is_empty() {
        return format!("{head}:clean");
    }
    format!("{head}:{}", changed.join(","))
}

fn should_reindex(state: &WatchState, git: &GitStatus, signature: &str) -> bool {
    if state.last_dirty_signature.as_deref() == Some(signature) {
        return false;
    }
    if git.dirty {
        return true;
    }
    match (&state.last_head, &git.head) {
        (Some(old), Some(new)) => old != new,
        (None, Some(_)) => true,
        _ => false,
    }
}

fn collect_changed_files(state: &WatchState, git: &GitStatus) -> Vec<String> {
    let mut files = git.changed_files.clone();
    if let (Some(old), Some(new)) = (&state.last_head, &git.head) {
        if old != new {
            if let Ok(diff) = git::diff_changed_files(&state.repo_path, old, new) {
                for file in diff {
                    if !files.contains(&file) {
                        files.push(file);
                    }
                }
            }
        }
    }
    files.sort();
    files.dedup();
    files
}

impl Default for Watcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_with(files: &[&str], head: &str) -> GitStatus {
        GitStatus {
            head: Some(head.into()),
            dirty: !files.is_empty(),
            changed_files: files.iter().map(|value| (*value).into()).collect(),
            deleted_files: vec![],
        }
    }

    fn state_with(signature: Option<&str>, head: Option<&str>) -> WatchState {
        WatchState {
            project: "cbm+test".into(),
            repo_path: PathBuf::from("."),
            last_head: head.map(str::to_string),
            last_dirty_signature: signature.map(str::to_string),
            interval_ms: BASE_INTERVAL_MS,
            next_poll_at: Instant::now(),
        }
    }

    #[test]
    fn signature_includes_head_and_files() {
        let git = git_with(&["a.rs", "b.rs"], "abc123");
        let changed = vec!["a.rs".into(), "b.rs".into()];
        let signature = status_signature(&git, &changed);
        assert_eq!(signature, "abc123:a.rs,b.rs");
    }

    #[test]
    fn skips_reindex_when_dirty_signature_unchanged() {
        let git = git_with(&["lib.rs"], "head1");
        let changed = vec!["lib.rs".into()];
        let signature = status_signature(&git, &changed);
        let state = state_with(Some(&signature), Some("head1"));
        assert!(!should_reindex(&state, &git, &signature));
    }

    #[test]
    fn reindexes_when_dirty_file_set_changes() {
        let git = git_with(&["lib.rs", "main.rs"], "head1");
        let changed = vec!["lib.rs".into(), "main.rs".into()];
        let signature = status_signature(&git, &changed);
        let state = state_with(Some("head1:lib.rs"), Some("head1"));
        assert!(should_reindex(&state, &git, &signature));
    }

    #[test]
    fn reindexes_when_head_changes() {
        let git = GitStatus {
            head: Some("newhead".into()),
            dirty: false,
            changed_files: vec![],
            deleted_files: vec![],
        };
        let signature = status_signature(&git, &[]);
        let state = state_with(None, Some("oldhead"));
        assert!(should_reindex(&state, &git, &signature));
    }

    #[test]
    fn idle_backoff_reaches_one_minute_cap() {
        assert_eq!(next_interval(5_000), 10_000);
        assert_eq!(next_interval(10_000), 20_000);
        assert_eq!(next_interval(40_000), 60_000);
        assert_eq!(next_interval(60_000), 60_000);
    }

    #[test]
    fn minimum_wake_tick_is_not_a_busy_loop() {
        assert!(MIN_TICK_MS >= 1_000);
        assert!(PROJECT_REFRESH_INTERVAL_MS >= MAX_INTERVAL_MS);
    }
}
