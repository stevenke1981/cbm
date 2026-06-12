use crate::store::{Edge, Symbol};
use std::collections::HashMap;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Parser, Query, QueryCursor};

/// AST-aware CALLS for Rust; returns edges with high confidence when tree-sitter succeeds.
pub fn resolve_calls_rust_ast(
    symbols: &[Symbol],
    content: &str,
    registry: &HashMap<String, Vec<String>>,
) -> Vec<Edge> {
    let lang: Language = tree_sitter_rust::LANGUAGE.into();
    let mut parser = Parser::new();
    if parser.set_language(&lang).is_err() {
        return Vec::new();
    }
    let tree = match parser.parse(content, None) {
        Some(t) => t,
        None => return Vec::new(),
    };
    let Ok(query) = Query::new(
        &lang,
        r#"
(call_expression
  function: (identifier) @callee)
(call_expression
  function: (field_expression
    field: (field_identifier) @method))
(call_expression
  function: (scoped_identifier
    name: (identifier) @scoped))
"#,
    ) else {
        return Vec::new();
    };
    let mut cursor = QueryCursor::new();
    let mut edges = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let functions: Vec<&Symbol> = symbols.iter().filter(|s| s.label == "Function").collect();

    for sym in functions {
        let body_start = sym.line_start.saturating_sub(1) as usize;
        let body_end = sym.line_end.max(sym.line_start) as usize;
        let mut matches = cursor.matches(&query, tree.root_node(), content.as_bytes());
        while let Some(m) = matches.next() {
            let mut callee = String::new();
            let mut line = 0usize;
            for cap in m.captures {
                let name = query.capture_names()[cap.index as usize];
                if matches!(name, "callee" | "method" | "scoped") {
                    callee = cap
                        .node
                        .utf8_text(content.as_bytes())
                        .unwrap_or("")
                        .to_string();
                    line = cap.node.start_position().row;
                }
            }
            if callee.is_empty() {
                continue;
            }
            let call_line = (line + 1) as i64;
            if call_line < sym.line_start || call_line > sym.line_end {
                continue;
            }
            if is_rust_keyword(&callee) || callee == sym.name {
                continue;
            }
            let targets = super::pick_callees(&callee, &sym.file_path, registry);
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
                        properties_json: Some(r#"{"confidence":"high","method":"ast"}"#.into()),
                    });
                }
            }
        }
        let _ = (body_start, body_end);
    }
    edges
}

fn is_rust_keyword(name: &str) -> bool {
    matches!(
        name,
        "if" | "for" | "while" | "match" | "return" | "let" | "loop" | "move" | "async" | "await"
    )
}
