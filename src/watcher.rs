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

const BASE_INTERVAL_MS: u64 = 5000;
const MAX_INTERVAL_MS: u64 = 60_000;
const MIN_TICK_MS: u64 = 250;

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
}

impl Watcher {
    pub fn new() -> Self {
        Self {
            stop: Arc::new(AtomicBool::new(false)),
            pipeline_busy: Arc::new(AtomicBool::new(false)),
            states: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn pipeline_busy(&self) -> Arc<AtomicBool> {
        self.pipeline_busy.clone()
    }

    pub fn register(&self, project: &str, repo_path: PathBuf) {
        let mut states = self.states.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = states.iter_mut().find(|s| s.project == project) {
            entry.repo_path = repo_path;
            return;
        }
        let last_head = Store::open(project)
            .ok()
            .and_then(|s| s.get_meta("git_head").ok().flatten());
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
        let Ok(projects) = Store::list_projects() else {
            return;
        };
        for p in projects {
            self.register(&p.name, PathBuf::from(p.repo_path));
        }
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
            .map(|s| WatcherProjectStatus {
                project: s.project.clone(),
                interval_ms: s.interval_ms,
                last_dirty_signature: s.last_dirty_signature.clone(),
                last_head: s.last_head.clone(),
                next_poll_in_ms: s.next_poll_at.saturating_duration_since(now).as_millis() as u64,
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
                if shutdown.as_ref().is_some_and(|s| s.is_triggered()) {
                    break;
                }
                let sleep_ms = self.poll_once();
                thread::sleep(Duration::from_millis(sleep_ms));
            }
            info!("watcher stopped");
        })
    }

    fn poll_once(&self) -> u64 {
        let now = Instant::now();
        self.refresh_from_disk();

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
                debug!(project = %state.project, "pipeline busy, defer");
                state.next_poll_at = now + Duration::from_millis(state.interval_ms);
                next_wake = next_wake.min(state.interval_ms);
                continue;
            }

            let git_status = match git::status(&state.repo_path) {
                Ok(s) => s,
                Err(e) => {
                    warn!(project = %state.project, error = %e, "git status failed");
                    apply_backoff(state, now);
                    next_wake = next_wake.min(state.interval_ms);
                    continue;
                }
            };

            if repo_is_clean(state, &git_status) {
                state.last_dirty_signature = None;
                state.last_head = git_status.head.clone();
                state.interval_ms = BASE_INTERVAL_MS;
                state.next_poll_at = now + Duration::from_millis(BASE_INTERVAL_MS);
                next_wake = next_wake.min(BASE_INTERVAL_MS);
                continue;
            }

            let changed = collect_changed_files(state, &git_status);
            let signature = status_signature(&git_status, &changed);

            if !git::is_repo(&state.repo_path) {
                apply_backoff(state, now);
                next_wake = next_wake.min(state.interval_ms);
                continue;
            }

            if !should_reindex(state, &git_status, &signature) {
                apply_backoff(state, now);
                info!(
                    project = %state.project,
                    interval_ms = state.interval_ms,
                    signature = %signature,
                    "dirty set unchanged, backing off"
                );
                next_wake = next_wake.min(state.interval_ms);
                continue;
            }

            if self
                .pipeline_busy
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                state.next_poll_at = now + Duration::from_millis(MIN_TICK_MS);
                next_wake = next_wake.min(MIN_TICK_MS);
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
                Ok(res) => {
                    if let Ok(store) = Store::open(&state.project) {
                        if let Some(head) = &git_status.head {
                            let _ = store.set_meta("git_head", head);
                        }
                    }
                    state.last_head = git_status.head;
                    state.last_dirty_signature = Some(signature);
                    state.interval_ms = BASE_INTERVAL_MS;
                    state.next_poll_at = now + Duration::from_millis(BASE_INTERVAL_MS);
                    info!(
                        project = %state.project,
                        files = res.files_indexed,
                        symbols = res.symbols_extracted,
                        interval_ms = state.interval_ms,
                        "incremental reindex done"
                    );
                    next_wake = next_wake.min(BASE_INTERVAL_MS);
                }
                Err(e) => {
                    apply_backoff(state, now);
                    warn!(
                        project = %state.project,
                        interval_ms = state.interval_ms,
                        error = %e,
                        "incremental reindex failed, backing off"
                    );
                    next_wake = next_wake.min(state.interval_ms);
                }
            }
        }

        next_wake.clamp(MIN_TICK_MS, MAX_INTERVAL_MS)
    }
}

fn apply_backoff(state: &mut WatchState, now: Instant) {
    state.interval_ms = (state.interval_ms.saturating_mul(2)).min(MAX_INTERVAL_MS);
    state.next_poll_at = now + Duration::from_millis(state.interval_ms);
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
                for f in diff {
                    if !files.contains(&f) {
                        files.push(f);
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
            changed_files: files.iter().map(|s| (*s).into()).collect(),
            deleted_files: vec![],
        }
    }

    fn state_with(sig: Option<&str>, head: Option<&str>) -> WatchState {
        WatchState {
            project: "cbm+test".into(),
            repo_path: PathBuf::from("."),
            last_head: head.map(str::to_string),
            last_dirty_signature: sig.map(str::to_string),
            interval_ms: BASE_INTERVAL_MS,
            next_poll_at: Instant::now(),
        }
    }

    #[test]
    fn signature_includes_head_and_files() {
        let git = git_with(&["a.rs", "b.rs"], "abc123");
        let changed = vec!["a.rs".into(), "b.rs".into()];
        let sig = status_signature(&git, &changed);
        assert_eq!(sig, "abc123:a.rs,b.rs");
    }

    #[test]
    fn skips_reindex_when_dirty_signature_unchanged() {
        let git = git_with(&["lib.rs"], "head1");
        let changed = vec!["lib.rs".into()];
        let sig = status_signature(&git, &changed);
        let state = state_with(Some(&sig), Some("head1"));
        assert!(!should_reindex(&state, &git, &sig));
    }

    #[test]
    fn reindexes_when_dirty_file_set_changes() {
        let git = git_with(&["lib.rs", "main.rs"], "head1");
        let changed = vec!["lib.rs".into(), "main.rs".into()];
        let sig = status_signature(&git, &changed);
        let state = state_with(Some("head1:lib.rs"), Some("head1"));
        assert!(should_reindex(&state, &git, &sig));
    }

    #[test]
    fn reindexes_when_head_changes() {
        let git = GitStatus {
            head: Some("newhead".into()),
            dirty: false,
            changed_files: vec![],
            deleted_files: vec![],
        };
        let sig = status_signature(&git, &[]);
        let state = state_with(None, Some("oldhead"));
        assert!(should_reindex(&state, &git, &sig));
    }

    #[test]
    fn backoff_doubles_interval() {
        let mut state = state_with(None, None);
        state.interval_ms = 5000;
        let now = Instant::now();
        apply_backoff(&mut state, now);
        assert_eq!(state.interval_ms, 10_000);
        apply_backoff(&mut state, now);
        assert_eq!(state.interval_ms, 20_000);
        state.interval_ms = 40_000;
        apply_backoff(&mut state, now);
        assert_eq!(state.interval_ms, MAX_INTERVAL_MS);
    }
}
