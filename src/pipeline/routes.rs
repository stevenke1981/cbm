use crate::store::{Edge, Symbol};
use regex::Regex;
use std::collections::{HashMap, HashSet};

/// Extract HTTP_ROUTE edges linking route paths to handler symbols.
pub fn extract_http_routes(
    file_path: &str,
    language: &str,
    content: &str,
    symbols: &[Symbol],
) -> Vec<Edge> {
    let handlers = build_handler_index(symbols, file_path);
    let mut edges = Vec::new();
    let mut seen = HashSet::new();

    let patterns: &[(&str, &str)] = match language {
        "python" => &[
            (
                r#"(?m)^\s*@(?:app|router|bp|api)\.(get|post|put|delete|patch|route)\(\s*["']([^"']+)["']"#,
                "decorator",
            ),
            (
                r#"(?m)^\s*@(?:get|post|put|delete|patch)\(\s*["']([^"']+)["']"#,
                "fastapi",
            ),
        ],
        "javascript" | "typescript" | "tsx" | "jsx" => &[(
            r#"(?m)\.(get|post|put|delete|patch)\(\s*["']([^"']+)["']"#,
            "express",
        )],
        "rust" => &[(r#"\.route\(\s*["']([^"']+)["']"#, "axum")],
        _ => &[],
    };

    for (pattern, framework) in patterns {
        let Ok(re) = Regex::new(pattern) else {
            continue;
        };
        for cap in re.captures_iter(content) {
            let path = match *framework {
                "decorator" | "express" => cap.get(2).map(|m| m.as_str()),
                "fastapi" | "axum" => cap.get(1).map(|m| m.as_str()),
                _ => None,
            };
            let Some(route_path) = path else { continue };
            let line = line_number(content, cap.get(0).unwrap().start());
            let Some(handler) = handler_after_line(symbols, file_path, line) else {
                continue;
            };
            let dst = format!("{file_path}::Route::{route_path}@L{line}");
            let key = (handler.clone(), dst.clone(), "HTTP_ROUTE".to_string());
            if seen.insert(key) {
                edges.push(Edge {
                    src_qn: handler,
                    dst_qn: dst,
                    edge_type: "HTTP_ROUTE".into(),
                    properties_json: Some(format!(
                        r#"{{"path":"{route_path}","framework":"{framework}"}}"#
                    )),
                });
            }
        }
    }

    let _ = handlers;
    edges
}

fn build_handler_index(symbols: &[Symbol], file_path: &str) -> HashMap<String, String> {
    symbols
        .iter()
        .filter(|s| s.file_path == file_path && s.label == "Function")
        .map(|s| (s.name.clone(), s.qualified_name.clone()))
        .collect()
}

fn handler_after_line(symbols: &[Symbol], file_path: &str, line: i64) -> Option<String> {
    symbols
        .iter()
        .filter(|s| {
            s.file_path == file_path
                && s.label == "Function"
                && s.line_start > line
                && s.line_start <= line + 5
        })
        .min_by_key(|s| s.line_start)
        .map(|s| s.qualified_name.clone())
}

fn line_number(content: &str, byte_offset: usize) -> i64 {
    content[..byte_offset.min(content.len())].lines().count() as i64 + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(file: &str, name: &str, line: i64) -> Symbol {
        Symbol {
            qualified_name: format!("{file}::Function::{name}@L{line}"),
            name: name.into(),
            label: "Function".into(),
            file_path: file.into(),
            line_start: line,
            line_end: line + 3,
            signature: None,
            properties_json: None,
        }
    }

    #[test]
    fn extracts_python_route() {
        let src = "@app.get(\"/users\")\ndef list_users():\n    pass\n";
        let symbols = vec![sym("api.py", "list_users", 2)];
        let edges = extract_http_routes("api.py", "python", src, &symbols);
        assert!(edges.iter().any(|e| e.edge_type == "HTTP_ROUTE"));
    }
}
