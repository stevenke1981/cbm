//! IMPORTS edge extraction with relative-path resolution.
//!
//! Aligns with DeusData-style import resolution: relative specifiers are
//! resolved against the importing file and matched to known project files
//! when possible; bare module names remain external Module nodes.

use crate::store::Edge;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Path alias from tsconfig/jsconfig (`paths` + `baseUrl`).
#[derive(Debug, Clone)]
pub struct PathAlias {
    /// Pattern prefix without trailing `*`, e.g. `@/` or `@app/`
    pub prefix: String,
    /// Target prefix(es), repo-relative, without trailing `*`
    pub targets: Vec<String>,
}

/// OOP import resolver with a project file index for path resolution.
pub struct ImportResolver {
    /// Normalized repo-relative paths (forward slashes).
    known_files: HashSet<String>,
    /// path without extension → full path (first wins).
    by_stem: HashMap<String, String>,
    /// tsconfig/jsconfig path aliases
    aliases: Vec<PathAlias>,
    /// Python package roots (dirs containing `__init__.py`)
    python_roots: Vec<String>,
}

impl ImportResolver {
    pub fn new(known_files: impl IntoIterator<Item = String>) -> Self {
        Self::with_aliases(known_files, Vec::new())
    }

    pub fn with_aliases(
        known_files: impl IntoIterator<Item = String>,
        aliases: Vec<PathAlias>,
    ) -> Self {
        let known_files: HashSet<String> = known_files
            .into_iter()
            .map(|p| p.replace('\\', "/"))
            .collect();
        let mut by_stem = HashMap::new();
        let mut python_roots = Vec::new();
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
            if path.ends_with("/__init__.py") || path == "__init__.py" {
                let root = path
                    .trim_end_matches("__init__.py")
                    .trim_end_matches('/')
                    .to_string();
                if !root.is_empty() && !python_roots.contains(&root) {
                    python_roots.push(root);
                }
            }
        }
        python_roots.sort();
        Self {
            known_files,
            by_stem,
            aliases,
            python_roots,
        }
    }

    /// Build resolver and load tsconfig/jsconfig aliases from known file contents map.
    pub fn from_project_files(
        known_files: impl IntoIterator<Item = String>,
        file_contents: &HashMap<String, String>,
    ) -> Self {
        let files: Vec<String> = known_files.into_iter().collect();
        let aliases = load_ts_path_aliases(&files, file_contents);
        Self::with_aliases(files, aliases)
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
                return self.file_target(specifier, &resolved, kind, "path");
            }
        }

        // TS/JS path aliases from tsconfig (`@/`, `@app/*`, …)
        if matches!(language, "javascript" | "typescript" | "tsx" | "jsx") {
            if let Some(resolved) = self.resolve_alias(specifier, language) {
                return self.file_target(specifier, &resolved, kind, "alias");
            }
        }

        // Rust `mod foo` → sibling foo.rs / foo/mod.rs
        if language == "rust" && kind == "mod" {
            if let Some(resolved) = self.resolve_rust_mod(file_path, specifier) {
                return self.file_target(specifier, &resolved, "mod", "path");
            }
        }

        // Python dotted package → package/module path if present
        if language == "python" && !specifier.starts_with('.') {
            if let Some(resolved) = self.resolve_python_absolute(specifier) {
                return self.file_target(specifier, &resolved, kind, "path");
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
        if let Some(p) = self.lookup_file(&as_path, "python") {
            return Some(p);
        }
        // Try under known package roots
        for root in &self.python_roots {
            let candidate = format!("{root}/{as_path}");
            if let Some(p) = self.lookup_file(&candidate, "python") {
                return Some(p);
            }
        }
        None
    }

    fn resolve_alias(&self, specifier: &str, language: &str) -> Option<String> {
        for alias in &self.aliases {
            if let Some(rest) = specifier.strip_prefix(&alias.prefix) {
                for target in &alias.targets {
                    let joined = if rest.is_empty() {
                        target.clone()
                    } else if target.ends_with('/') {
                        format!("{target}{rest}")
                    } else {
                        format!("{target}/{rest}")
                    };
                    if let Some(p) = self.lookup_file(&joined, language) {
                        return Some(p);
                    }
                }
            }
        }
        None
    }

    fn file_target(
        &self,
        specifier: &str,
        resolved: &str,
        kind: &str,
        method: &str,
    ) -> (String, String) {
        let props = format!(
            r#"{{"specifier":"{}","resolved":"{}","kind":"{kind}","method":"{method}"}}"#,
            json_escape(specifier),
            json_escape(resolved)
        );
        (format!("{resolved}::File::{resolved}"), props)
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

/// Load `compilerOptions.paths` (+ baseUrl) from tsconfig.json / jsconfig.json in the project.
pub fn load_ts_path_aliases(
    known_files: &[String],
    contents: &HashMap<String, String>,
) -> Vec<PathAlias> {
    let mut aliases = Vec::new();
    for name in ["tsconfig.json", "jsconfig.json"] {
        // Prefer root config; also accept nested */tsconfig.json
        let candidates: Vec<&String> = known_files
            .iter()
            .filter(|p| p.ends_with(name) || *p == name)
            .collect();
        for path in candidates {
            let Some(raw) = contents.get(path.as_str()) else {
                // Try path key variants
                continue;
            };
            if let Some(mut found) = parse_tsconfig_paths(raw, path) {
                aliases.append(&mut found);
            }
        }
        // Also match content map keys that equal the basename at repo root
        if let Some(raw) = contents.get(name) {
            if let Some(mut found) = parse_tsconfig_paths(raw, name) {
                aliases.append(&mut found);
            }
        }
    }
    aliases
}

fn parse_tsconfig_paths(raw: &str, config_path: &str) -> Option<Vec<PathAlias>> {
    // Strip JSONC-ish comments lightly
    let cleaned = strip_json_comments(raw);
    let v: serde_json::Value = serde_json::from_str(&cleaned).ok()?;
    let compiler = v.get("compilerOptions")?;
    let base_url = compiler
        .get("baseUrl")
        .and_then(|b| b.as_str())
        .unwrap_or(".");
    let config_dir = parent_dir(config_path);
    let base = if config_dir.is_empty() {
        base_url.to_string()
    } else if base_url == "." {
        config_dir
    } else {
        normalize_path(&format!("{config_dir}/{base_url}"))
    };
    let paths = compiler.get("paths")?.as_object()?;
    let mut out = Vec::new();
    for (pattern, targets) in paths {
        let prefix = pattern.trim_end_matches('*').to_string();
        if prefix.is_empty() {
            continue;
        }
        let mut target_list = Vec::new();
        if let Some(arr) = targets.as_array() {
            for t in arr {
                if let Some(s) = t.as_str() {
                    let t_prefix = s.trim_end_matches('*');
                    let joined = if base.is_empty() {
                        t_prefix.to_string()
                    } else {
                        normalize_path(&format!("{base}/{t_prefix}"))
                    };
                    target_list.push(joined);
                }
            }
        }
        if !target_list.is_empty() {
            out.push(PathAlias {
                prefix,
                targets: target_list,
            });
        }
    }
    Some(out)
}

fn strip_json_comments(s: &str) -> String {
    // Minimal: remove // line comments outside strings (good enough for tsconfig)
    let mut out = String::with_capacity(s.len());
    for line in s.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") {
            continue;
        }
        if let Some(idx) = line.find("//") {
            // keep if inside quotes naively
            let before = &line[..idx];
            if before.matches('"').count() % 2 == 0 {
                out.push_str(before);
                out.push('\n');
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    out
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

    #[test]
    fn resolves_tsconfig_path_alias() {
        let mut contents = HashMap::new();
        contents.insert(
            "tsconfig.json".into(),
            r#"{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": { "@/*": ["src/*"] }
  }
}"#
            .into(),
        );
        contents.insert("src/main.ts".into(), "import x from '@/util/helper';\n".into());
        contents.insert("src/util/helper.ts".into(), "export const x = 1;\n".into());
        let files = vec![
            "tsconfig.json".into(),
            "src/main.ts".into(),
            "src/util/helper.ts".into(),
        ];
        let resolver = ImportResolver::from_project_files(files, &contents);
        let edges = resolver.extract("src/main.ts", "typescript", contents.get("src/main.ts").unwrap());
        assert!(
            edges
                .iter()
                .any(|e| e.dst_qn.contains("src/util/helper.ts::File::")),
            "{edges:?}"
        );
    }
}
