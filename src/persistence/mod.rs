//! Compressed graph artifact export/import (`.codebase-memory/graph.db.zst`).

use crate::error::{Error, Result};
use crate::project::{project_db_path, project_name_from_path};
use crate::store::Store;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const ARTIFACT_DIR: &str = ".codebase-memory";
pub const ARTIFACT_FILE: &str = "graph.db.zst";
pub const MANIFEST_FILE: &str = "manifest.json";

const ZSTD_LEVEL: i32 = 3;

pub fn env_enabled() -> bool {
    matches!(
        std::env::var("CBM_PERSISTENCE").as_deref(),
        Ok("1") | Ok("true") | Ok("yes") | Ok("on")
    )
}

pub fn artifact_dir(repo_path: &Path) -> PathBuf {
    repo_path.join(ARTIFACT_DIR)
}

pub fn artifact_path(repo_path: &Path) -> PathBuf {
    artifact_dir(repo_path).join(ARTIFACT_FILE)
}

pub fn manifest_path(repo_path: &Path) -> PathBuf {
    artifact_dir(repo_path).join(MANIFEST_FILE)
}

pub fn export_artifact(repo_path: &Path, project: &str, store: &Store) -> Result<PathBuf> {
    store.checkpoint_truncate()?;
    let db_path = project_db_path(project);
    if !db_path.is_file() {
        return Err(Error::Other(format!(
            "database not found: {}",
            db_path.display()
        )));
    }

    let raw = fs::read(&db_path)?;
    let compressed = zstd::encode_all(&raw[..], ZSTD_LEVEL)?;

    let dest = artifact_path(repo_path);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&dest, &compressed)?;

    let symbols = store.count_symbols().unwrap_or(0);
    let files = store.count_files().unwrap_or(0);
    let manifest = json!({
        "project": project,
        "format": "sqlite-zstd",
        "version": env!("CARGO_PKG_VERSION"),
        "exported_at": unix_now(),
        "bytes_raw": raw.len(),
        "bytes_compressed": compressed.len(),
        "symbols": symbols,
        "files": files,
    });
    fs::write(
        manifest_path(repo_path),
        serde_json::to_string_pretty(&manifest)? + "\n",
    )?;

    Ok(dest)
}

pub fn import_artifact(repo_path: &Path, project: Option<&str>) -> Result<bool> {
    let src = artifact_path(repo_path);
    if !src.is_file() {
        return Ok(false);
    }

    let project = match project {
        Some(p) => crate::project::normalize_project_name(p),
        None => project_name_from_path(repo_path),
    };

    let compressed = fs::read(&src)?;
    let raw = zstd::decode_all(&compressed[..])
        .map_err(|e| Error::Other(format!("zstd decode failed: {e}")))?;

    let db_path = project_db_path(&project);
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&db_path, raw)?;

    let store = Store::open(&project)?;
    store.upsert_project(repo_path.to_string_lossy().as_ref())?;
    Ok(true)
}

pub fn try_restore(repo_path: &Path, project: &str) -> Result<bool> {
    if project_db_path(project).is_file() {
        return Ok(false);
    }
    import_artifact(repo_path, Some(project))
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover::IndexMode;
    use crate::pipeline::Pipeline;
    use crate::test_lock;
    use tempfile::TempDir;

    fn fixture_repo() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("main.rs"), "pub fn persist_me() {}\n").unwrap();
        dir
    }

    #[test]
    fn roundtrip_artifact() {
        let _guard = test_lock::acquire();
        let cache = TempDir::new().unwrap();
        std::env::set_var("CBM_CACHE_DIR", cache.path());

        let dir = fixture_repo();
        let project = "cbm+artifact-roundtrip";
        let result = Pipeline::new(IndexMode::Full)
            .set_export_artifact(true)
            .run(dir.path(), Some("artifact-roundtrip"))
            .unwrap();
        assert!(result.success);
        assert!(result.artifact_path.is_some());

        let artifact = artifact_path(dir.path());
        assert!(artifact.is_file());
        assert!(manifest_path(dir.path()).is_file());

        {
            let store = Store::open(project).unwrap();
            store.delete_project().unwrap();
        }
        crate::store::delete_project_db(project).unwrap();
        assert!(import_artifact(dir.path(), Some("artifact-roundtrip")).unwrap());

        let store = Store::open(project).unwrap();
        let count = store.count_symbols().unwrap();
        assert!(count >= 1);
        assert!(store
            .search(&crate::store::SearchFilter {
                query: Some("persist_me".into()),
                ..Default::default()
            })
            .unwrap()
            .symbols
            .iter()
            .any(|s| s.name == "persist_me"));
    }

    #[test]
    fn try_restore_skips_when_cache_exists() {
        let _guard = test_lock::acquire();
        let cache = TempDir::new().unwrap();
        std::env::set_var("CBM_CACHE_DIR", cache.path());
        let dir = fixture_repo();
        let project = "cbm+restore-skip";
        Pipeline::new(IndexMode::Full)
            .set_export_artifact(true)
            .run(dir.path(), Some("restore-skip"))
            .unwrap();
        assert!(!try_restore(dir.path(), project).unwrap());
    }
}
