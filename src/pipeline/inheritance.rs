use crate::store::{Edge, Symbol};
use regex::Regex;
use std::collections::{HashMap, HashSet};

/// Extract INHERITS, IMPLEMENTS, and DECORATES edges from source patterns.
pub fn extract_inheritance_edges(
    file_path: &str,
    language: &str,
    content: &str,
    symbols: &[Symbol],
) -> Vec<Edge> {
    let qn_by_name = build_file_name_index(symbols, file_path);
    let mut edges = Vec::new();
    let mut seen = HashSet::new();

    let mut ctx = ExtractCtx {
        file_path,
        content,
        qn_by_name: &qn_by_name,
        edges: &mut edges,
        seen: &mut seen,
    };

    match language {
        "rust" => {
            extract_pairs(
                &mut ctx,
                r"(?m)^\s*impl\s+([\w:]+)\s+for\s+(\w+)",
                "IMPLEMENTS",
                |cap| Some((cap.get(2)?.as_str(), cap.get(1)?.as_str())),
            );
            extract_decorators(file_path, content, symbols, &mut edges, &mut seen);
        }
        "python" => {
            extract_pairs(
                &mut ctx,
                r"(?m)^\s*class\s+(\w+)\s*\(\s*([\w.]+)\s*\)",
                "INHERITS",
                |cap| Some((cap.get(1)?.as_str(), cap.get(2)?.as_str())),
            );
            extract_decorators(file_path, content, symbols, &mut edges, &mut seen);
        }
        "java" => {
            extract_pairs(
                &mut ctx,
                r"(?m)^\s*(?:public\s+)?class\s+(\w+)\s+extends\s+(\w+)",
                "INHERITS",
                |cap| Some((cap.get(1)?.as_str(), cap.get(2)?.as_str())),
            );
            if let Ok(re) =
                Regex::new(r"(?m)^\s*(?:public\s+)?class\s+(\w+)\s+implements\s+([\w,\s]+)")
            {
                for cap in re.captures_iter(content) {
                    let Some(child) = cap.get(1) else { continue };
                    let Some(ifaces) = cap.get(2) else { continue };
                    let Some(src) = qn_by_name.get(child.as_str()) else {
                        continue;
                    };
                    for iface in ifaces.as_str().split(',') {
                        let iface = iface.trim();
                        if iface.is_empty() {
                            continue;
                        }
                        push_edge(
                            "IMPLEMENTS",
                            src,
                            &resolve_target(iface, &qn_by_name, file_path),
                            &mut edges,
                            &mut seen,
                        );
                    }
                }
            }
            extract_decorators(file_path, content, symbols, &mut edges, &mut seen);
        }
        "javascript" | "typescript" | "tsx" | "jsx" => {
            extract_pairs(
                &mut ctx,
                r"(?m)^\s*class\s+(\w+)\s+extends\s+(\w+)",
                "INHERITS",
                |cap| Some((cap.get(1)?.as_str(), cap.get(2)?.as_str())),
            );
            extract_decorators(file_path, content, symbols, &mut edges, &mut seen);
        }
        _ => {}
    }

    edges
}

struct ExtractCtx<'a> {
    file_path: &'a str,
    content: &'a str,
    qn_by_name: &'a HashMap<String, String>,
    edges: &'a mut Vec<Edge>,
    seen: &'a mut HashSet<(String, String, String)>,
}

fn extract_pairs<'a>(
    ctx: &mut ExtractCtx<'a>,
    pattern: &str,
    edge_type: &str,
    names: impl Fn(regex::Captures<'a>) -> Option<(&'a str, &'a str)>,
) {
    let Ok(re) = Regex::new(pattern) else { return };
    for cap in re.captures_iter(ctx.content) {
        let Some((child, parent)) = names(cap) else {
            continue;
        };
        let Some(src) = ctx.qn_by_name.get(child) else {
            continue;
        };
        let dst = resolve_target(parent, ctx.qn_by_name, ctx.file_path);
        push_edge(edge_type, src, &dst, ctx.edges, ctx.seen);
    }
}

fn extract_decorators(
    file_path: &str,
    content: &str,
    symbols: &[Symbol],
    edges: &mut Vec<Edge>,
    seen: &mut HashSet<(String, String, String)>,
) {
    let Ok(re) = Regex::new(r"(?m)^\s*#?\[?@([\w.:]+)") else {
        return;
    };
    for cap in re.captures_iter(content) {
        let Some(decorator) = cap.get(1) else {
            continue;
        };
        let line = line_number(content, cap.get(0).unwrap().start());
        let Some(target) = symbol_at_line(symbols, file_path, line + 1) else {
            continue;
        };
        let dst = format!("{file_path}::Decorator::{}@L{line}", decorator.as_str());
        push_edge("DECORATES", &target, &dst, edges, seen);
    }
}

fn push_edge(
    edge_type: &str,
    src: &str,
    dst: &str,
    edges: &mut Vec<Edge>,
    seen: &mut HashSet<(String, String, String)>,
) {
    let key = (src.to_string(), dst.to_string(), edge_type.to_string());
    if seen.insert(key) {
        edges.push(Edge {
            src_qn: src.to_string(),
            dst_qn: dst.to_string(),
            edge_type: edge_type.into(),
            properties_json: None,
        });
    }
}

fn build_file_name_index(symbols: &[Symbol], file_path: &str) -> HashMap<String, String> {
    symbols
        .iter()
        .filter(|s| s.file_path == file_path)
        .map(|s| (s.name.clone(), s.qualified_name.clone()))
        .collect()
}

fn resolve_target(name: &str, local: &HashMap<String, String>, file_path: &str) -> String {
    let base = name.rsplit('.').next().unwrap_or(name);
    local
        .get(base)
        .or_else(|| local.get(name))
        .cloned()
        .unwrap_or_else(|| format!("{file_path}::Class::{base}@L1"))
}

fn line_number(content: &str, byte_offset: usize) -> i64 {
    content[..byte_offset].lines().count() as i64 + 1
}

fn symbol_at_line(symbols: &[Symbol], file_path: &str, line: i64) -> Option<String> {
    symbols
        .iter()
        .filter(|s| s.file_path == file_path && s.line_start >= line)
        .min_by_key(|s| s.line_start)
        .map(|s| s.qualified_name.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(file: &str, name: &str, label: &str, line: i64) -> Symbol {
        Symbol {
            qualified_name: format!("{file}::Class::{name}@L{line}"),
            name: name.into(),
            label: label.into(),
            file_path: file.into(),
            line_start: line,
            line_end: line + 1,
            signature: None,
            properties_json: None,
        }
    }

    #[test]
    fn extracts_python_inherits() {
        let src = "class Child(Parent):\n    pass\n";
        let symbols = vec![
            sym("m.py", "Child", "Class", 1),
            sym("m.py", "Parent", "Class", 10),
        ];
        let edges = extract_inheritance_edges("m.py", "python", src, &symbols);
        assert!(edges
            .iter()
            .any(|e| e.edge_type == "INHERITS" && e.src_qn.contains("Child")));
    }

    #[test]
    fn extracts_rust_implements() {
        let src = "struct Greeter;\nimpl Display for Greeter {\n}\n";
        let symbols = vec![
            sym("g.rs", "Greeter", "Class", 1),
            sym("g.rs", "Display", "Class", 2),
        ];
        let edges = extract_inheritance_edges("g.rs", "rust", src, &symbols);
        assert!(edges.iter().any(|e| e.edge_type == "IMPLEMENTS"));
    }
}
