//! IMPORTS edge extraction with relative-path resolution.
//!
//! Aligns with DeusData-style import resolution: relative specifiers are
//! resolved against the importing file and matched to known project files
//! when possible; bare module names remain external Module nodes.

use crate::store::Edge;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// OOP import resolver with a project file index for path resolution.
pub struct ImportResolver {
    /// Normalized repo-relative paths (forward slashes).
    known_files: HashSet<String>,
    /// path without extension → full path (first wins).
    by_stem: HashMap<String, String>,
}

impl ImportResolver {
    pub fn new(known_files: impl IntoIterator<Item = String>) -> Self {
        let known_files: HashSet<String> = known_files
            .into_iter()
            .map(|p| p.replace('\\', "/"))
            .collect();
        let mut by_stem = HashMap::new();
        for path in &known_files {
            let stem = strip_known_extension(path);
            by_stem.entry(stem).or_insert_with(|| path.clone());
            // Also index without trailing /index or /mod
            if let Some(dir) = path
                .strip_suffix("/index.js")
                .or_else(|| path.strip_suffix("/index.ts"))
                .or_else(|| path.strip_suffix("/index.tsx"))
                .or_else(|| path.strip_suffix("/index.jsx"))
                .or_else(|| path.strip_suffix("/mod.rs"))
                .or_else(|| path.strip_suffix("/__init__.py"))
            {
                by_stem.entry(dir.to_string()).or_insert_with(|| path.clone());
            }
        }
        Self {
            known_files,
            by_stem,
        }
    }

    pub fn extract(&self, file_path: &str, language: &str, content: &str) -> Vec<Edge> {
        let specs = collect_import_specs(language, content);
        let mut edges = Vec::new();
        let mut seen = HashSet::new();
        let src_qn = format!("{file_path}::File::{file_path}");

        for (specifier, kind) in specs {
            let (dst_qn, props) = self.resolve_target(file_path, language, &specifier, &kind);
            let key = (src_qn.clone(), dst_qn.clone());
            if seen.insert(key) {
                edges.push(Edge {
                    src_qn: src_qn.clone(),
                    dst_qn,
                    edge_type: "IMPORTS".into(),
                    properties_json: Some(props),
                });
            }
        }
        edges
    }

    fn resolve_target(
        &self,
        file_path: &str,
        language: &str,
        specifier: &str,
        kind: &str,
    ) -> (String, String) {
        // Relative / path-like imports
        if is_relative_or_path_import(specifier, language) {
            if let Some(resolved) = self.resolve_relative(file_path, language, specifier) {
                let props = format!(
                    r#"{{"specifier":"{}","resolved":"{}","kind":"{kind}","method":"path"}}"#,
                    json_escape(specifier),
                    json_escape(&resolved)
                );
                return (format!("{resolved}::File::{resolved}"), props);
            }
        }

        // Rust `mod foo` → sibling foo.rs / foo/mod.rs
        if language == "rust" && kind == "mod" {
            if let Some(resolved) = self.resolve_rust_mod(file_path, specifier) {
                let props = format!(
                    r#"{{"specifier":"{}","resolved":"{}","kind":"mod","method":"path"}}"#,
                    json_escape(specifier),
                    json_escape(&resolved)
                );
                return (format!("{resolved}::File::{resolved}"), props);
            }
        }

        // Python dotted package → package/module path if present
        if language == "python" && !specifier.starts_with('.') {
            if let Some(resolved) = self.resolve_python_absolute(specifier) {
                let props = format!(
                    r#"{{"specifier":"{}","resolved":"{}","kind":"{kind}","method":"path"}}"#,
                    json_escape(specifier),
                    json_escape(&resolved)
                );
                return (format!("{resolved}::File::{resolved}"), props);
            }
        }

        // External / unresolved module node
        let module_key = specifier.replace('\\', "/");
        let props = format!(
            r#"{{"specifier":"{}","kind":"{kind}","method":"external"}}"#,
            json_escape(specifier)
        );
        (
            format!("{module_key}::Module::{module_key}"),
            props,
        )
    }

    fn resolve_relative(
        &self,
        file_path: &str,
        language: &str,
        specifier: &str,
    ) -> Option<String> {
        let base_dir = parent_dir(file_path);
        let joined = join_relative(&base_dir, specifier);
        self.lookup_file(&joined, language)
    }

    fn resolve_rust_mod(&self, file_path: &str, mod_name: &str) -> Option<String> {
        let base_dir = parent_dir(file_path);
        let candidates = [
            format!("{base_dir}/{mod_name}.rs"),
            format!("{base_dir}/{mod_name}/mod.rs"),
        ];
        for c in candidates {
            let norm = normalize_path(&c);
            if self.known_files.contains(&norm) {
                return Some(norm);
            }
        }
        None
    }

    fn resolve_python_absolute(&self, specifier: &str) -> Option<String> {
        let as_path = specifier.replace('.', "/");
        self.lookup_file(&as_path, "python")
    }

    fn lookup_file(&self, path_no_ext: &str, language: &str) -> Option<String> {
        let norm = normalize_path(path_no_ext);
        if self.known_files.contains(&norm) {
            return Some(norm);
        }
        if let Some(p) = self.by_stem.get(&norm) {
            return Some(p.clone());
        }
        for ext in extensions_for(language) {
            let with_ext = normalize_path(&format!("{norm}.{ext}"));
            if self.known_files.contains(&with_ext) {
                return Some(with_ext);
            }
            if let Some(p) = self.by_stem.get(&with_ext) {
                return Some(p.clone());
            }
        }
        for suffix in index_suffixes(language) {
            let idx = normalize_path(&format!("{norm}/{suffix}"));
            if self.known_files.contains(&idx) {
                return Some(idx);
            }
        }
        None
    }
}

/// Convenience wrapper when no file index is available (external-only).
pub fn extract_import_edges(file_path: &str, language: &str, content: &str) -> Vec<Edge> {
    ImportResolver::new(std::iter::empty()).extract(file_path, language, content)
}

fn collect_import_specs(language: &str, content: &str) -> Vec<(String, String)> {
    let patterns: &[(&str, &str)] = match language {
        "rust" => &[
            (r"(?m)^\s*use\s+([\w:]+)", "use"),
            (r"(?m)^\s*mod\s+(\w+)\s*;", "mod"),
        ],
        "python" => &[
            (r"(?m)^\s*from\s+(\.+[\w.]*)\s+import", "from_rel"),
            (r"(?m)^\s*from\s+([\w.]+)\s+import", "from"),
            (r"(?m)^\s*import\s+([\w.]+)", "import"),
        ],
        "javascript" | "typescript" | "tsx" | "jsx" => &[
            (r#"(?m)^\s*import\s+.*?from\s+['"]([^'"]+)['"]"#, "esm"),
            (r#"(?m)import\s*\(\s*['"]([^'"]+)['"]\s*\)"#, "dynamic"),
            (r#"(?m)require\s*\(\s*['"]([^'"]+)['"]\s*\)"#, "cjs"),
            (r#"(?m)export\s+.*?from\s+['"]([^'"]+)['"]"#, "reexport"),
        ],
        "go" => &[
            (r#"(?m)^\s*import\s+"([^"]+)""#, "import"),
            (r#"(?m)^\s*"([^"]+)""#, "import_block"),
        ],
        "java" => &[(r"(?m)^\s*import\s+([\w.]+)\s*;", "import")],
        "kotlin" => &[
            (r"(?m)^\s*import\s+([\w.]+)", "import"),
            (r"(?m)^\s*package\s+([\w.]+)", "package"),
        ],
        "swift" => &[
            (r"(?m)^\s*import\s+(\w+)", "import"),
            (r#"(?m)^\s*import\s+class\s+(\w+)"#, "import"),
        ],
        "csharp" => &[(r"(?m)^\s*using\s+([\w.]+)\s*;", "using")],
        "ruby" => &[
            (r#"(?m)^\s*require\s+['"]([^'"]+)['"]"#, "require"),
            (r#"(?m)^\s*require_relative\s+['"]([^'"]+)['"]"#, "require_relative"),
        ],
        "php" => &[
            (r"(?m)^\s*use\s+([\w\\]+)\s*;", "use"),
            (r#"(?m)require(?:_once)?\s*\(?\s*['"]([^'"]+)['"]"#, "require"),
            (r#"(?m)include(?:_once)?\s*\(?\s*['"]([^'"]+)['"]"#, "include"),
        ],
        "c" | "cpp" => &[(r#"(?m)^\s*#\s*include\s*[<"]([^>"]+)[>"]"#, "include")],
        _ => &[],
    };

    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for (pat, kind) in patterns {
        let Ok(re) = Regex::new(pat) else { continue };
        for cap in re.captures_iter(content) {
            if let Some(m) = cap.get(1) {
                let spec = m.as_str().trim().to_string();
                if spec.is_empty() {
                    continue;
                }
                if seen.insert(spec.clone()) {
                    out.push((spec, (*kind).to_string()));
                }
            }
        }
    }
    out
}

fn is_relative_or_path_import(specifier: &str, language: &str) -> bool {
    if specifier.starts_with("./")
        || specifier.starts_with("../")
        || specifier.starts_with('/')
        || specifier.starts_with('.')
    {
        return true;
    }
    // Windows absolute rarely appears in imports; treat bare paths with slash
    if matches!(language, "javascript" | "typescript" | "tsx" | "jsx" | "c" | "cpp")
        && specifier.contains('/')
        && !specifier.contains(':')
    {
        // local path-like without package scope (@scope/pkg has @)
        return !specifier.starts_with('@');
    }
    if language == "ruby" {
        return true; // require often relative-ish; resolve attempts still safe
    }
    false
}

fn parent_dir(file_path: &str) -> String {
    let p = file_path.replace('\\', "/");
    match p.rfind('/') {
        Some(i) => p[..i].to_string(),
        None => ".".into(),
    }
}

fn join_relative(base_dir: &str, specifier: &str) -> String {
    // Python: from .foo import / from ..bar import
    if specifier.starts_with('.') && !specifier.starts_with("./") && !specifier.starts_with("..") {
        // Python-style .pkg / ..pkg
        let dots = specifier.chars().take_while(|c| *c == '.').count();
        let rest = &specifier[dots..];
        let mut dir = PathBuf::from(if base_dir == "." { "" } else { base_dir });
        for _ in 1..dots {
            if !dir.pop() {
                break;
            }
        }
        if !rest.is_empty() {
            for part in rest.split('.') {
                if !part.is_empty() {
                    dir.push(part);
                }
            }
        }
        return normalize_path(&dir.to_string_lossy());
    }

    let base = if base_dir == "." {
        PathBuf::new()
    } else {
        PathBuf::from(base_dir)
    };
    let joined = if Path::new(specifier).is_absolute() {
        PathBuf::from(specifier)
    } else {
        base.join(specifier)
    };
    normalize_path(&joined.to_string_lossy())
}

fn normalize_path(path: &str) -> String {
    let path = path.replace('\\', "/");
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

fn strip_known_extension(path: &str) -> String {
    let p = path.replace('\\', "/");
    for ext in [
        ".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs", ".py", ".rs", ".go", ".java", ".kt", ".cs",
        ".rb", ".php", ".c", ".h", ".cpp", ".hpp", ".cc",
    ] {
        if let Some(s) = p.strip_suffix(ext) {
            return s.to_string();
        }
    }
    p
}

fn extensions_for(language: &str) -> &'static [&'static str] {
    match language {
        "rust" => &["rs"],
        "python" => &["py"],
        "javascript" | "jsx" => &["js", "jsx", "mjs", "cjs", "ts", "tsx"],
        "typescript" | "tsx" => &["ts", "tsx", "js", "jsx"],
        "go" => &["go"],
        "java" => &["java"],
        "kotlin" => &["kt"],
        "csharp" => &["cs"],
        "ruby" => &["rb"],
        "php" => &["php"],
        "c" => &["h", "c"],
        "cpp" => &["hpp", "h", "cpp", "cc", "cxx"],
        _ => &[],
    }
}

fn index_suffixes(language: &str) -> &'static [&'static str] {
    match language {
        "javascript" | "jsx" | "typescript" | "tsx" => {
            &["index.js", "index.ts", "index.tsx", "index.jsx"]
        }
        "python" => &["__init__.py"],
        "rust" => &["mod.rs"],
        _ => &[],
    }
}

fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_js_relative_import() {
        let resolver = ImportResolver::new(vec![
            "src/main.js".into(),
            "src/util/helper.js".into(),
        ]);
        let src = "import { x } from './util/helper';\n";
        let edges = resolver.extract("src/main.js", "javascript", src);
        assert!(
            edges.iter().any(|e| e.dst_qn.contains("src/util/helper.js::File::")),
            "{edges:?}"
        );
        assert!(edges[0]
            .properties_json
            .as_ref()
            .is_some_and(|p| p.contains("path")));
    }

    #[test]
    fn resolves_python_relative_import() {
        let resolver = ImportResolver::new(vec![
            "pkg/__init__.py".into(),
            "pkg/a.py".into(),
            "pkg/b.py".into(),
        ]);
        let src = "from .b import foo\n";
        let edges = resolver.extract("pkg/a.py", "python", src);
        assert!(
            edges.iter().any(|e| e.dst_qn.contains("pkg/b.py::File::")),
            "{edges:?}"
        );
    }

    #[test]
    fn external_import_stays_module_node() {
        let resolver = ImportResolver::new(vec!["main.py".into()]);
        let src = "import os\n";
        let edges = resolver.extract("main.py", "python", src);
        assert!(edges.iter().any(|e| e.dst_qn.contains("os::Module::")));
    }

    #[test]
    fn resolves_rust_mod() {
        let resolver = ImportResolver::new(vec!["src/lib.rs".into(), "src/util.rs".into()]);
        let src = "mod util;\n";
        let edges = resolver.extract("src/lib.rs", "rust", src);
        assert!(
            edges.iter().any(|e| e.dst_qn.contains("src/util.rs::File::")),
            "{edges:?}"
        );
    }
}
