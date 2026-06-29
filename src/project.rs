use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// Normalize project name; CBM indexes use `cbm+` prefix.
pub fn normalize_project_name(name: &str) -> String {
    if name.starts_with("cbm+") {
        name.to_string()
    } else {
        format!("cbm+{name}")
    }
}

/// Derive project name from full canonical path (readable slug + short hash).
pub fn project_name_from_path(repo_path: &Path) -> String {
    let canonical = repo_path
        .canonicalize()
        .unwrap_or_else(|_| repo_path.to_path_buf());
    let path_key = canonical.to_string_lossy().replace('\\', "/");

    let mut hasher = DefaultHasher::new();
    path_key.hash(&mut hasher);
    let hash = format!("{:08x}", hasher.finish());

    let stem = canonical
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .replace(['\\', '/'], "-");

    normalize_project_name(&format!("{stem}-{hash}"))
}

pub fn default_cache_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("CBM_CACHE_DIR") {
        return PathBuf::from(dir);
    }
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("cbm-mcp")
}

pub fn project_db_path(project: &str) -> PathBuf {
    default_cache_dir().join(format!("{project}.db"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_cbm_prefix() {
        assert_eq!(normalize_project_name("my-app"), "cbm+my-app");
        assert_eq!(normalize_project_name("cbm+my-app"), "cbm+my-app");
    }

    #[test]
    fn distinct_paths_with_same_basename_differ() {
        let base = tempfile::TempDir::new().unwrap();
        let foo = base.path().join("foo").join("app");
        let bar = base.path().join("bar").join("app");
        std::fs::create_dir_all(&foo).unwrap();
        std::fs::create_dir_all(&bar).unwrap();

        let a = project_name_from_path(&foo);
        let b = project_name_from_path(&bar);
        assert_ne!(a, b);
        assert!(a.starts_with("cbm+app-"));
        assert!(b.starts_with("cbm+app-"));
    }
}
