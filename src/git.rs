use crate::error::{Error, Result};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct GitStatus {
    pub head: Option<String>,
    pub dirty: bool,
    pub changed_files: Vec<String>,
    pub deleted_files: Vec<String>,
}

pub fn is_repo(path: &Path) -> bool {
    Command::new("git")
        .args(["-C", &path.to_string_lossy(), "rev-parse", "--git-dir"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn head_sha(path: &Path) -> Result<Option<String>> {
    let out = Command::new("git")
        .args(["-C", &path.to_string_lossy(), "rev-parse", "HEAD"])
        .output()
        .map_err(|e| Error::Other(format!("git not available: {e}")))?;
    if !out.status.success() {
        return Ok(None);
    }
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if sha.is_empty() {
        Ok(None)
    } else {
        Ok(Some(sha))
    }
}

pub fn status(path: &Path) -> Result<GitStatus> {
    let head = head_sha(path)?;
    let out = Command::new("git")
        .args(["-C", &path.to_string_lossy(), "status", "--porcelain"])
        .output()
        .map_err(|e| Error::Other(format!("git not available: {e}")))?;

    if !out.status.success() {
        return Ok(GitStatus {
            head,
            dirty: false,
            changed_files: vec![],
            deleted_files: vec![],
        });
    }

    let mut changed_files = Vec::new();
    let mut deleted_files = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if line.len() < 4 {
            continue;
        }
        let code = &line[..2];
        let file = line[3..].trim();
        // handle "old -> new" rename
        let file = file.rsplit(" -> ").next().unwrap_or(file).trim();
        if file.is_empty() {
            continue;
        }
        let normalized = file.replace('\\', "/");
        if code.contains('D') {
            deleted_files.push(normalized.clone());
        }
        changed_files.push(normalized);
    }

    Ok(GitStatus {
        dirty: !changed_files.is_empty(),
        changed_files,
        deleted_files,
        head,
    })
}

pub fn diff_changed_files(path: &Path, old_head: &str, new_head: &str) -> Result<Vec<String>> {
    let out = Command::new("git")
        .args([
            "-C",
            &path.to_string_lossy(),
            "diff",
            "--name-only",
            old_head,
            new_head,
        ])
        .output()
        .map_err(|e| Error::Other(format!("git not available: {e}")))?;
    if !out.status.success() {
        return Ok(vec![]);
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().replace('\\', "/"))
        .filter(|l| !l.is_empty())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_porcelain_paths() {
        let dir = tempfile::TempDir::new().unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::fs::write(dir.path().join("a.rs"), "fn main() {}\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "a.rs"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(dir.path())
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "t@t.com")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "t@t.com")
            .output()
            .unwrap();
        std::fs::write(dir.path().join("a.rs"), "fn main() { foo() }\n").unwrap();

        let st = status(dir.path()).unwrap();
        assert!(st.dirty);
        assert!(st.changed_files.iter().any(|f| f == "a.rs"));
    }
}
