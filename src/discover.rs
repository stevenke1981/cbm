use crate::error::Result;
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct DiscoveredFile {
    pub path: PathBuf,
    pub relative_path: String,
    pub language: String,
}

pub const SKIP_DIRS: &[&str] = &[
    ".git",
    ".codebase-memory",
    "node_modules",
    "__pycache__",
    ".venv",
    "venv",
    "target",
    "dist",
    "build",
    ".cargo",
];

pub const SKIP_SUFFIXES: &[&str] = &[
    ".pyc", ".pyo", ".exe", ".dll", ".so", ".dylib", ".o", ".a", ".lib", ".png", ".jpg", ".jpeg",
    ".gif", ".webp", ".ico", ".svg", ".mp3", ".mp4", ".wav", ".zip", ".tar", ".gz", ".zst",
    ".woff", ".woff2", ".ttf", ".eot",
];

pub const SKIP_FILENAMES: &[&str] = &["package-lock.json", "yarn.lock", "go.sum", "Cargo.lock"];

const CODE_EXTENSIONS: &[&str] = &[
    "rs", "py", "pyi", "js", "mjs", "cjs", "ts", "mts", "cts", "tsx", "jsx", "go", "java", "kt",
    "c", "h", "cpp", "cc", "cxx", "hpp", "cs", "rb", "php", "swift", "zig", "lua", "sh", "bash",
    "ps1", "sql",
];

const MODERATE_SKIP_DIRS: &[&str] = &[
    "tests",
    "test",
    "__tests__",
    "spec",
    "specs",
    "docs",
    "doc",
    "examples",
    "example",
    "fixtures",
    "testdata",
    "snapshots",
    "coverage",
];

const FAST_MAX_BYTES: u64 = 512 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexMode {
    Full,
    Moderate,
    Fast,
}

impl IndexMode {
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "fast" => Self::Fast,
            "moderate" => Self::Moderate,
            _ => Self::Full,
        }
    }
}

pub fn language_for_path(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?;
    Some(
        match ext {
            "rs" => "rust",
            "py" | "pyi" => "python",
            "js" | "mjs" | "cjs" => "javascript",
            "ts" | "mts" | "cts" => "typescript",
            "tsx" => "tsx",
            "jsx" => "jsx",
            "go" => "go",
            "java" => "java",
            "kt" => "kotlin",
            "c" | "h" => "c",
            "cpp" | "cc" | "cxx" | "hpp" => "cpp",
            "cs" => "csharp",
            "rb" => "ruby",
            "php" => "php",
            "swift" => "swift",
            "zig" => "zig",
            "lua" => "lua",
            "sh" | "bash" => "shell",
            "ps1" => "powershell",
            "sql" => "sql",
            "html" | "htm" => "html",
            "css" | "scss" => "css",
            "json" => "json",
            "yaml" | "yml" => "yaml",
            "toml" => "toml",
            "md" => "markdown",
            _ => return None,
        }
        .to_string(),
    )
}

/// Shared ignore-aware walker for discovery and RLM scan.
pub fn configure_walker(repo_path: &Path, mode: IndexMode) -> WalkBuilder {
    let mut builder = WalkBuilder::new(repo_path);
    builder
        .hidden(false)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(true)
        .add_custom_ignore_filename(".cbmignore")
        .filter_entry(move |entry| {
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if SKIP_DIRS.contains(&name) {
                        return false;
                    }
                    if mode != IndexMode::Full && matches!(name, "vendor" | "dist" | "build") {
                        return false;
                    }
                }
            }
            true
        });
    builder
}

pub fn discover(repo_path: &Path, mode: IndexMode) -> Result<Vec<DiscoveredFile>> {
    let repo_path = repo_path
        .canonicalize()
        .unwrap_or_else(|_| repo_path.to_path_buf());
    let mut files = Vec::new();

    let walker = configure_walker(&repo_path, mode).build();

    for entry in walker {
        let entry = entry.map_err(|e| crate::error::Error::Other(e.to_string()))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if mode == IndexMode::Moderate
            && path
                .components()
                .filter_map(|c| c.as_os_str().to_str())
                .any(|part| MODERATE_SKIP_DIRS.contains(&part))
        {
            continue;
        }
        if let Ok(meta) = path.metadata() {
            if mode == IndexMode::Fast && meta.len() > FAST_MAX_BYTES {
                continue;
            }
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if SKIP_FILENAMES.contains(&name) {
                continue;
            }
        }
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let dotted = format!(".{ext}");
            if SKIP_SUFFIXES.contains(&dotted.as_str()) {
                continue;
            }
            if mode == IndexMode::Fast && !CODE_EXTENSIONS.contains(&ext) {
                continue;
            }
        }
        let language = match language_for_path(path) {
            Some(lang) => lang,
            None => continue,
        };
        let relative = path
            .strip_prefix(&repo_path)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        files.push(DiscoveredFile {
            path: path.to_path_buf(),
            relative_path: relative,
            language,
        });
    }

    files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn detects_rust_language() {
        assert_eq!(language_for_path(Path::new("foo.rs")), Some("rust".into()));
        assert_eq!(language_for_path(Path::new("foo.txt")), None);
    }

    #[test]
    fn respects_cbmignore_and_gitignore() {
        let dir = TempDir::new().unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .expect("git init");
        fs::write(dir.path().join(".gitignore"), "ignored_git.rs\n").unwrap();
        fs::write(dir.path().join(".cbmignore"), "ignored_cbm.rs\n").unwrap();
        fs::write(dir.path().join("kept.rs"), "fn main() {}\n").unwrap();
        fs::write(dir.path().join("ignored_git.rs"), "fn g() {}\n").unwrap();
        fs::write(dir.path().join("ignored_cbm.rs"), "fn c() {}\n").unwrap();

        let files = discover(dir.path(), IndexMode::Full).unwrap();
        let names: Vec<_> = files.iter().map(|f| f.relative_path.as_str()).collect();
        assert!(names.contains(&"kept.rs"));
        assert!(!names.contains(&"ignored_git.rs"));
        assert!(!names.contains(&"ignored_cbm.rs"));
    }
}
