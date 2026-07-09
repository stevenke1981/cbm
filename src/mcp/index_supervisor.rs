//! Background index job supervisor (DeusData index_supervisor spirit).
//!
//! Long `index_repository` runs can block MCP stdio. When `background=true`,
//! work is spawned on a worker thread and the tool returns immediately with a
//! job handle. Clients poll via `index_status`.

use crate::discover::IndexMode;
use crate::error::{Error, Result};
use crate::pipeline::{IndexResult, Pipeline};
use crate::project::{normalize_project_name, project_name_from_path};
use crate::store::StorePool;
use crate::watcher::Watcher;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct IndexJobSnapshot {
    pub job_id: String,
    pub project: String,
    pub repo_path: String,
    pub state: JobState,
    pub mode: String,
    pub incremental: bool,
    pub started_unix_ms: u64,
    pub finished_unix_ms: Option<u64>,
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<IndexResult>,
}

struct IndexJob {
    snapshot: IndexJobSnapshot,
}

/// Process-global supervisor for background index jobs.
pub struct IndexSupervisor {
    jobs: Mutex<HashMap<String, IndexJob>>,
    /// project → job_id currently running/queued
    active_by_project: Mutex<HashMap<String, String>>,
    /// Global single-flight optional lock (one heavy index at a time)
    global_busy: AtomicBool,
}

impl IndexSupervisor {
    pub fn global() -> &'static IndexSupervisor {
        static SUP: OnceLock<IndexSupervisor> = OnceLock::new();
        SUP.get_or_init(|| IndexSupervisor {
            jobs: Mutex::new(HashMap::new()),
            active_by_project: Mutex::new(HashMap::new()),
            global_busy: AtomicBool::new(false),
        })
    }

    pub fn get_job(&self, job_id: &str) -> Option<IndexJobSnapshot> {
        self.jobs
            .lock()
            .ok()?
            .get(job_id)
            .map(|j| j.snapshot.clone())
    }

    pub fn active_for_project(&self, project: &str) -> Option<IndexJobSnapshot> {
        let project = normalize_project_name(project);
        let active = self.active_by_project.lock().ok()?;
        let job_id = active.get(&project)?;
        self.get_job(job_id)
    }

    pub fn is_project_busy(&self, project: &str) -> bool {
        self.active_for_project(project)
            .is_some_and(|j| matches!(j.state, JobState::Queued | JobState::Running))
    }

    /// Start a background index. Returns snapshot with state=running/queued.
    pub fn start(
        &self,
        repo_path: PathBuf,
        project: Option<String>,
        mode: IndexMode,
        incremental: bool,
        persistence: bool,
        watcher: Option<Arc<Watcher>>,
    ) -> Result<IndexJobSnapshot> {
        let abs = repo_path
            .canonicalize()
            .unwrap_or_else(|_| repo_path.clone());
        let project_name = match project.as_deref() {
            Some(p) => normalize_project_name(p),
            None => project_name_from_path(&abs),
        };

        // Reject concurrent index on same project
        if self.is_project_busy(&project_name) {
            return Err(Error::InvalidArgument(format!(
                "index already in progress for project {project_name}; poll index_status"
            )));
        }

        let job_id = Uuid::new_v4().to_string();
        let now = unix_ms();
        let snapshot = IndexJobSnapshot {
            job_id: job_id.clone(),
            project: project_name.clone(),
            repo_path: abs.to_string_lossy().to_string(),
            state: JobState::Queued,
            mode: format!("{mode:?}").to_lowercase(),
            incremental,
            started_unix_ms: now,
            finished_unix_ms: None,
            error: None,
            result: None,
        };

        {
            let mut jobs = self.jobs.lock().expect("index jobs lock");
            jobs.insert(
                job_id.clone(),
                IndexJob {
                    snapshot: snapshot.clone(),
                },
            );
            // prune old finished jobs (keep last 32)
            prune_finished(&mut jobs, 32);
        }
        {
            let mut active = self.active_by_project.lock().expect("active projects lock");
            active.insert(project_name.clone(), job_id.clone());
        }

        let job_id_worker = job_id.clone();
        let project_worker = project_name.clone();
        let abs_worker = abs.clone();
        let mode_worker = mode;
        let watcher_worker = watcher;

        thread::Builder::new()
            .name(format!("cbm-index-{job_id}"))
            .spawn(move || {
                let supervisor = IndexSupervisor::global();
                supervisor.set_state(&job_id_worker, JobState::Running, None, None);

                // Optional global busy for watcher coordination
                let _g = GlobalBusyGuard::new(&supervisor.global_busy);
                let busy_flag = watcher_worker
                    .as_ref()
                    .map(|w| w.pipeline_busy())
                    .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
                let _pg = PipelineBusyGuard::new(busy_flag);

                let pipeline = Pipeline::new(mode_worker).set_export_artifact(persistence);
                let run = if incremental {
                    pipeline.run_smart(&abs_worker, Some(&project_worker), true)
                } else {
                    pipeline.run(&abs_worker, Some(&project_worker))
                };

                match run {
                    Ok(result) => {
                        StorePool::global().invalidate(&result.project);
                        if let Some(w) = &watcher_worker {
                            w.register(&result.project, abs_worker.clone());
                        }
                        supervisor.set_state(
                            &job_id_worker,
                            JobState::Completed,
                            None,
                            Some(result),
                        );
                    }
                    Err(e) => {
                        supervisor.set_state(
                            &job_id_worker,
                            JobState::Failed,
                            Some(e.to_string()),
                            None,
                        );
                    }
                }

                // Clear active marker only if still this job
                if let Ok(mut active) = supervisor.active_by_project.lock() {
                    if active.get(&project_worker) == Some(&job_id_worker) {
                        active.remove(&project_worker);
                    }
                }
            })
            .map_err(|e| Error::InvalidArgument(format!("failed to spawn index worker: {e}")))?;

        // Return running snapshot (worker may already have flipped to Running)
        Ok(self.get_job(&job_id).unwrap_or(snapshot))
    }

    fn set_state(
        &self,
        job_id: &str,
        state: JobState,
        error: Option<String>,
        result: Option<IndexResult>,
    ) {
        if let Ok(mut jobs) = self.jobs.lock() {
            if let Some(job) = jobs.get_mut(job_id) {
                job.snapshot.state = state;
                if matches!(state, JobState::Completed | JobState::Failed) {
                    job.snapshot.finished_unix_ms = Some(unix_ms());
                }
                if error.is_some() {
                    job.snapshot.error = error;
                }
                if result.is_some() {
                    job.snapshot.result = result;
                }
            }
        }
    }

    /// Wait for a job up to `timeout` (used by tests / optional sync-wait).
    pub fn wait(&self, job_id: &str, timeout: Duration) -> Option<IndexJobSnapshot> {
        let start = Instant::now();
        loop {
            let snap = self.get_job(job_id)?;
            if matches!(snap.state, JobState::Completed | JobState::Failed) {
                return Some(snap);
            }
            if start.elapsed() >= timeout {
                return Some(snap);
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}

struct GlobalBusyGuard<'a> {
    flag: &'a AtomicBool,
}

impl<'a> GlobalBusyGuard<'a> {
    fn new(flag: &'a AtomicBool) -> Self {
        flag.store(true, Ordering::SeqCst);
        Self { flag }
    }
}

impl Drop for GlobalBusyGuard<'_> {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::SeqCst);
    }
}

struct PipelineBusyGuard {
    flag: Arc<AtomicBool>,
}

impl PipelineBusyGuard {
    fn new(flag: Arc<AtomicBool>) -> Self {
        flag.store(true, Ordering::SeqCst);
        Self { flag }
    }
}

impl Drop for PipelineBusyGuard {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::SeqCst);
    }
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn prune_finished(jobs: &mut HashMap<String, IndexJob>, keep: usize) {
    let mut finished: Vec<(String, u64)> = jobs
        .iter()
        .filter(|(_, j)| matches!(j.snapshot.state, JobState::Completed | JobState::Failed))
        .map(|(id, j)| {
            (
                id.clone(),
                j.snapshot
                    .finished_unix_ms
                    .unwrap_or(j.snapshot.started_unix_ms),
            )
        })
        .collect();
    if finished.len() <= keep {
        return;
    }
    finished.sort_by_key(|(_, t)| *t);
    let drop_n = finished.len() - keep;
    for (id, _) in finished.into_iter().take(drop_n) {
        jobs.remove(&id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lock;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn background_index_completes_and_is_pollable() {
        let _guard = test_lock::acquire();
        let cache = TempDir::new().unwrap();
        std::env::set_var("CBM_CACHE_DIR", cache.path());

        let repo = TempDir::new().unwrap();
        fs::write(repo.path().join("main.rs"), "fn main() {}\n").unwrap();

        let sup = IndexSupervisor::global();
        let snap = sup
            .start(
                repo.path().to_path_buf(),
                Some("bg-index-test".into()),
                IndexMode::Fast,
                false,
                false,
                None,
            )
            .unwrap();
        assert!(matches!(snap.state, JobState::Queued | JobState::Running));
        assert!(sup.is_project_busy(&snap.project));

        let done = sup
            .wait(&snap.job_id, Duration::from_secs(30))
            .expect("job");
        assert_eq!(done.state, JobState::Completed, "{done:?}");
        assert!(done.result.is_some());
        assert!(!sup.is_project_busy(&snap.project));

        let _ = crate::store::delete_project_db(&done.project);
    }

    #[test]
    fn rejects_concurrent_same_project() {
        let _guard = test_lock::acquire();
        let cache = TempDir::new().unwrap();
        std::env::set_var("CBM_CACHE_DIR", cache.path());

        let repo = TempDir::new().unwrap();
        fs::write(repo.path().join("a.rs"), "fn a() {}\n").unwrap();

        let sup = IndexSupervisor::global();
        let first = sup
            .start(
                repo.path().to_path_buf(),
                Some("concurrent-idx".into()),
                IndexMode::Full,
                false,
                false,
                None,
            )
            .unwrap();
        let second = sup.start(
            repo.path().to_path_buf(),
            Some("concurrent-idx".into()),
            IndexMode::Full,
            false,
            false,
            None,
        );
        assert!(second.is_err(), "expected concurrent reject");
        let _ = sup.wait(&first.job_id, Duration::from_secs(30));
        let _ = crate::store::delete_project_db(&first.project);
    }
}
