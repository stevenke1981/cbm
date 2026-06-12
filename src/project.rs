use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// Normalize project name; CBRLM indexes use `cbrlm+` prefix.
pub fn normalize_project_name(name: &str) -> String {
    if name.starts_with("cbrlm+") {
        name.to_string()
    } else {
        format!("cbrlm+{name}")
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
    if let Ok(dir) = std::env::var("CBRLM_CACHE_DIR") {
        return PathBuf::from(dir);
    }
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("codebase-memory-mcp")
}

pub fn project_db_path(project: &str) -> PathBuf {
    default_cache_dir().join(format!("{project}.db"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_cbrlm_prefix() {
        assert_eq!(normalize_project_name("my-app"), "cbrlm+my-app");
        assert_eq!(normalize_project_name("cbrlm+my-app"), "cbrlm+my-app");
    }

    #[test]
    fn distinct_paths_with_same_basename_differ() {
        let a = project_name_from_path(Path::new("D:\\foo\\app"));
        let b = project_name_from_path(Path::new("D:\\bar\\app"));
        assert_ne!(a, b);
        assert!(a.starts_with("cbrlm+app-"));
        assert!(b.starts_with("cbrlm+app-"));
    }
}