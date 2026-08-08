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
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--git-dir"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

pub fn head_sha(path: &Path) -> Result<Option<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "HEAD"])
        .output()
        .map_err(|error| Error::Other(format!("git not available: {error}")))?;
    if !output.status.success() {
        return Ok(None);
    }
    let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if sha.is_empty() {
        Ok(None)
    } else {
        Ok(Some(sha))
    }
}

/// Return HEAD and worktree changes with a single Git process.
///
/// The previous implementation spawned `git rev-parse` and `git status` for
/// every watcher poll. With multiple indexed repositories that doubled the
/// process churn even when every repository was idle.
pub fn status(path: &Path) -> Result<GitStatus> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args([
            "-c",
            "core.quotepath=false",
            "status",
            "--porcelain=v2",
            "--branch",
            "--untracked-files=normal",
        ])
        .output()
        .map_err(|error| Error::Other(format!("git not available: {error}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(Error::Other(if stderr.is_empty() {
            format!("git status failed for {}", path.display())
        } else {
            stderr
        }));
    }

    Ok(parse_porcelain_v2(&String::from_utf8_lossy(&output.stdout)))
}

fn parse_porcelain_v2(output: &str) -> GitStatus {
    let mut status = GitStatus::default();

    for line in output.lines() {
        if let Some(oid) = line.strip_prefix("# branch.oid ") {
            let oid = oid.trim();
            if !oid.is_empty() && oid != "(initial)" {
                status.head = Some(oid.to_string());
            }
            continue;
        }

        let parsed = if let Some(rest) = line.strip_prefix("1 ") {
            parse_changed_record(rest, 8, 7)
        } else if let Some(rest) = line.strip_prefix("2 ") {
            parse_changed_record(rest, 9, 8)
        } else if let Some(rest) = line.strip_prefix("u ") {
            parse_changed_record(rest, 11, 10)
        } else if let Some(path) = line.strip_prefix("? ") {
            Some(("??", path))
        } else {
            None
        };

        let Some((code, raw_path)) = parsed else {
            continue;
        };
        let path = normalize_status_path(raw_path);
        if path.is_empty() {
            continue;
        }
        if code.contains('D') {
            status.deleted_files.push(path.clone());
        }
        status.changed_files.push(path);
    }

    status.changed_files.sort();
    status.changed_files.dedup();
    status.deleted_files.sort();
    status.deleted_files.dedup();
    status.dirty = !status.changed_files.is_empty();
    status
}

fn parse_changed_record(rest: &str, field_count: usize, path_index: usize) -> Option<(&str, &str)> {
    let fields: Vec<&str> = rest.splitn(field_count, ' ').collect();
    if fields.len() <= path_index {
        return None;
    }

    let code = fields[0];
    let path = fields[path_index].split('\t').next().unwrap_or_default();
    Some((code, path))
}

fn normalize_status_path(path: &str) -> String {
    path.trim()
        .trim_matches('"')
        .replace("\\\"", "\"")
        .replace("\\\\", "\\")
        .replace('\\', "/")
}

pub fn diff_changed_files(path: &Path, old_head: &str, new_head: &str) -> Result<Vec<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args([
            "-c",
            "core.quotepath=false",
            "diff",
            "--name-only",
            old_head,
            new_head,
        ])
        .output()
        .map_err(|error| Error::Other(format!("git not available: {error}")))?;
    if !output.status.success() {
        return Ok(vec![]);
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().replace('\\', "/"))
        .filter(|line| !line.is_empty())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_porcelain_v2_records() {
        let parsed = parse_porcelain_v2(
            "# branch.oid abc123\n\
             # branch.head main\n\
             1 .M N... 100644 100644 100644 aaa bbb src/lib.rs\n\
             1 D. N... 100644 000000 000000 ccc 000 README.md\n\
             ? new file.txt\n",
        );

        assert_eq!(parsed.head.as_deref(), Some("abc123"));
        assert!(parsed.dirty);
        assert_eq!(
            parsed.changed_files,
            vec!["README.md", "new file.txt", "src/lib.rs"]
        );
        assert_eq!(parsed.deleted_files, vec!["README.md"]);
    }

    #[test]
    fn parses_porcelain_paths_from_git() {
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

        let status = status(dir.path()).unwrap();
        assert!(status.dirty);
        assert!(status.changed_files.iter().any(|file| file == "a.rs"));
    }

    #[test]
    fn non_repository_returns_an_error() {
        let dir = tempfile::TempDir::new().unwrap();
        assert!(status(dir.path()).is_err());
    }
}
