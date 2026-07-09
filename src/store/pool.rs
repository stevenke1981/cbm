//! Process-local SQLite connection pool keyed by project name.
//!
//! Aligns with DeusData's idle store cache: repeated MCP tool calls against the
//! same project reuse one open connection instead of re-running PRAGMAs each time.
//!
//! `rusqlite::Connection` is `Send` but not `Sync`, so the pool never hands out
//! shared references across threads. Callers use [`StorePool::with`] /
//! [`StorePool::with_mut`] under the mutex for the duration of one tool call.

use super::Store;
use crate::error::Result;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const DEFAULT_IDLE_SECS: u64 = 300;

struct PooledStore {
    store: Store,
    last_used: Instant,
}

/// Thread-safe pool of open project stores.
pub struct StorePool {
    inner: Mutex<HashMap<String, PooledStore>>,
    idle_timeout: Duration,
}

impl StorePool {
    pub fn new(idle_timeout: Duration) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            idle_timeout,
        }
    }

    /// Global process pool (lazy).
    pub fn global() -> &'static StorePool {
        static POOL: OnceLock<StorePool> = OnceLock::new();
        POOL.get_or_init(|| {
            let secs = std::env::var("CBM_STORE_IDLE_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(DEFAULT_IDLE_SECS);
            StorePool::new(Duration::from_secs(secs))
        })
    }

    /// Run `f` with a pooled (or freshly opened) store for `project`.
    pub fn with<R>(&self, project: &str, f: impl FnOnce(&Store) -> Result<R>) -> Result<R> {
        let mut map = self.inner.lock().expect("store pool lock");
        self.evict_idle_locked(&mut map);

        if !map.contains_key(project) {
            let store = Store::open(project)?;
            map.insert(
                project.to_string(),
                PooledStore {
                    store,
                    last_used: Instant::now(),
                },
            );
        }

        let entry = map
            .get_mut(project)
            .expect("store just inserted or already present");
        entry.last_used = Instant::now();
        f(&entry.store)
    }

    /// Drop a project from the pool (after delete / full reindex).
    pub fn invalidate(&self, project: &str) {
        if let Ok(mut map) = self.inner.lock() {
            map.remove(project);
        }
    }

    /// Evict all idle entries.
    pub fn evict_idle(&self) {
        if let Ok(mut map) = self.inner.lock() {
            self.evict_idle_locked(&mut map);
        }
    }

    pub fn clear(&self) {
        if let Ok(mut map) = self.inner.lock() {
            map.clear();
        }
    }

    pub fn len(&self) -> usize {
        self.inner.lock().map(|m| m.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn evict_idle_locked(&self, map: &mut HashMap<String, PooledStore>) {
        let now = Instant::now();
        map.retain(|_, entry| now.duration_since(entry.last_used) < self.idle_timeout);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lock;

    #[test]
    fn reuses_same_project_slot() {
        let _guard = test_lock::acquire();
        let dir = tempfile::TempDir::new().unwrap();
        std::env::set_var("CBM_CACHE_DIR", dir.path());

        let pool = StorePool::new(Duration::from_secs(60));
        pool.with("cbm+pool-test", |s| {
            s.upsert_project("/tmp/x")?;
            Ok(())
        })
        .unwrap();
        assert_eq!(pool.len(), 1);
        pool.with("cbm+pool-test", |s| {
            assert_eq!(s.project(), "cbm+pool-test");
            Ok(())
        })
        .unwrap();
        assert_eq!(pool.len(), 1);
    }

    #[test]
    fn invalidate_drops_entry() {
        let _guard = test_lock::acquire();
        let dir = tempfile::TempDir::new().unwrap();
        std::env::set_var("CBM_CACHE_DIR", dir.path());

        let pool = StorePool::new(Duration::from_secs(60));
        pool.with("cbm+pool-inv", |_| Ok(())).unwrap();
        pool.invalidate("cbm+pool-inv");
        assert_eq!(pool.len(), 0);
    }
}
