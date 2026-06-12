use cbrlm::test_lock;
use std::path::PathBuf;
use std::sync::MutexGuard;

/// Per-test isolated cache directory. Hold the returned guard for the whole test.
pub fn isolated_cache() -> (MutexGuard<'static, ()>, tempfile::TempDir, PathBuf) {
    let guard = test_lock::acquire();
    let dir = tempfile::TempDir::new().expect("tempdir");
    let path = dir.path().to_path_buf();
    std::env::set_var("CBRLM_CACHE_DIR", &path);
    (guard, dir, path)
}
