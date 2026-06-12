use crate::store::Edge;
use regex::Regex;
use std::collections::HashSet;

pub fn extract_import_edges(file_path: &str, language: &str, content: &str) -> Vec<Edge> {
    let patterns: &[&str] = match language {
        "rust" => &[r"(?m)^\s*use\s+([\w:]+)", r"(?m)^\s*mod\s+(\w+)"],
        "python" => &[
            r"(?m)^\s*from\s+([\w.]+)\s+import",
            r"(?m)^\s*import\s+([\w.]+)",
        ],
        "javascript" | "typescript" | "tsx" | "jsx" => &[
            r#"(?m)^\s*import\s+.*?from\s+['"]([^'"]+)['"]"#,
            r#"(?m)require\(['"]([^'"]+)['"]\)"#,
        ],
        "go" => &[r#"(?m)^\s*import\s+"([^"]+)""#, r#"(?m)^\s*import\s+(\w+)"#],
        _ => &[],
    };

    let mut edges = Vec::new();
    let mut seen = HashSet::new();
    for pat in patterns {
        let Ok(re) = Regex::new(pat) else { continue };
        for cap in re.captures_iter(content) {
            if let Some(m) = cap.get(1) {
                let target = m.as_str().replace('.', "/");
                let dst = format!("{target}::Module::{target}");
                let key = (file_path.to_string(), dst.clone());
                if seen.insert(key) {
                    edges.push(Edge {
                        src_qn: format!("{file_path}::File::{file_path}"),
                        dst_qn: dst,
                        edge_type: "IMPORTS".into(),
                        properties_json: None,
                    });
                }
            }
        }
    }
    edges
}
