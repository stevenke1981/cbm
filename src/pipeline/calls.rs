use crate::store::{Edge, Symbol};
use std::collections::HashMap;

pub fn build_name_registry(symbols: &[Symbol]) -> HashMap<String, Vec<String>> {
    let mut name_to_qn: HashMap<String, Vec<String>> = HashMap::new();
    for sym in symbols {
        name_to_qn
            .entry(sym.name.clone())
            .or_default()
            .push(sym.qualified_name.clone());
    }
    name_to_qn
}

/// Resolve CALLS edges using a project-wide symbol registry (cross-file).
pub fn resolve_calls_with_registry(
    symbols: &[Symbol],
    content: &str,
    language: &str,
    registry: &HashMap<String, Vec<String>>,
) -> Vec<Edge> {
    if language == "rust" {
        let ast_edges = super::calls_ast::resolve_calls_rust_ast(symbols, content, registry);
        if !ast_edges.is_empty() {
            return ast_edges;
        }
    }
    resolve_calls_inner(symbols, content, registry)
}

/// Resolve CALLS edges from symbol definitions using name matching within file scope.
pub fn resolve_calls(symbols: &[Symbol], content: &str, _language: &str) -> Vec<Edge> {
    let registry = build_name_registry(symbols);
    resolve_calls_inner(symbols, content, &registry)
}

fn resolve_calls_inner(
    symbols: &[Symbol],
    content: &str,
    name_to_qn: &HashMap<String, Vec<String>>,
) -> Vec<Edge> {
    let call_patterns = [
        regex::Regex::new(r"\b(\w+)\s*\(").unwrap(),
        regex::Regex::new(r"\.(\w+)\s*\(").unwrap(),
    ];

    let mut edges = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    for sym in symbols {
        if sym.label != "Function" {
            continue;
        }
        let start = sym.line_start.saturating_sub(1) as usize;
        let end = sym.line_end.min(lines.len() as i64) as usize;
        if start >= end {
            continue;
        }
        let body = lines[start..end].join("\n");
        let mut seen = std::collections::HashSet::new();
        for re in &call_patterns {
            for cap in re.captures_iter(&body) {
                if let Some(name_match) = cap.get(1) {
                    let callee_name = name_match.as_str();
                    if callee_name == sym.name
                        || matches!(
                            callee_name,
                            "if" | "for"
                                | "while"
                                | "match"
                                | "return"
                                | "let"
                                | "const"
                                | "var"
                                | "new"
                                | "self"
                                | "super"
                                | "print"
                                | "println"
                                | "format"
                        )
                    {
                        continue;
                    }
                    let targets = pick_callees(callee_name, &sym.file_path, name_to_qn);
                    for dst in targets {
                        if dst == sym.qualified_name {
                            continue;
                        }
                        let key = (sym.qualified_name.clone(), dst.clone());
                        if seen.insert(key.clone()) {
                            edges.push(Edge {
                                src_qn: key.0,
                                dst_qn: key.1,
                                edge_type: "CALLS".into(),
                                properties_json: Some(
                                    r#"{"confidence":"resolved","method":"regex"}"#.into(),
                                ),
                            });
                        }
                    }
                }
            }
        }
    }
    edges
}

/// Same-file matches first; cross-file only when the name is globally unique.
pub(crate) fn pick_callees(
    callee_name: &str,
    caller_file: &str,
    registry: &HashMap<String, Vec<String>>,
) -> Vec<String> {
    let Some(qns) = registry.get(callee_name) else {
        return Vec::new();
    };

    let same_file: Vec<String> = qns
        .iter()
        .filter(|qn| qn.starts_with(&format!("{caller_file}::")))
        .cloned()
        .collect();
    if !same_file.is_empty() {
        return same_file;
    }
    if qns.len() == 1 {
        return qns.clone();
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn qn(file: &str, label: &str, name: &str, line: i64) -> String {
        crate::symbol_id::qualified_name(file, label, name, line)
    }

    #[test]
    fn resolves_internal_calls() {
        let symbols = vec![
            Symbol {
                qualified_name: qn("a.rs", "Function", "main", 1),
                name: "main".into(),
                label: "Function".into(),
                file_path: "a.rs".into(),
                line_start: 1,
                line_end: 5,
                signature: None,
                properties_json: None,
            },
            Symbol {
                qualified_name: qn("a.rs", "Function", "helper", 7),
                name: "helper".into(),
                label: "Function".into(),
                file_path: "a.rs".into(),
                line_start: 7,
                line_end: 9,
                signature: None,
                properties_json: None,
            },
        ];
        let src = "fn main() {\n    helper();\n}\n\nfn helper() {}\n";
        let edges = resolve_calls(&symbols, src, "rust");
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].src_qn, qn("a.rs", "Function", "main", 1));
        assert_eq!(edges[0].dst_qn, qn("a.rs", "Function", "helper", 7));
    }

    #[test]
    fn skips_ambiguous_cross_file_calls() {
        let symbols = vec![
            Symbol {
                qualified_name: qn("a.rs", "Function", "main", 1),
                name: "main".into(),
                label: "Function".into(),
                file_path: "a.rs".into(),
                line_start: 1,
                line_end: 3,
                signature: None,
                properties_json: None,
            },
            Symbol {
                qualified_name: qn("b.rs", "Function", "helper", 1),
                name: "helper".into(),
                label: "Function".into(),
                file_path: "b.rs".into(),
                line_start: 1,
                line_end: 2,
                signature: None,
                properties_json: None,
            },
            Symbol {
                qualified_name: qn("c.rs", "Function", "helper", 1),
                name: "helper".into(),
                label: "Function".into(),
                file_path: "c.rs".into(),
                line_start: 1,
                line_end: 2,
                signature: None,
                properties_json: None,
            },
        ];
        let src = "fn main() { helper(); }\n";
        let registry = build_name_registry(&symbols);
        let edges = resolve_calls_with_registry(&symbols[..1], src, "rust", &registry);
        assert!(
            edges.is_empty(),
            "ambiguous cross-file callee should not link"
        );
    }
}
