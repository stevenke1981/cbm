use crate::error::{Error, Result};
use crate::store::Symbol;
use crate::symbol_id::qualified_name;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Parser, Query, QueryCursor};

pub fn extract_symbols(file_path: &str, language: &str, content: &str) -> Result<Vec<Symbol>> {
    let lang = match language_for_ts(language) {
        Some(l) => l,
        None => return Ok(extract_symbols_regex(file_path, language, content)),
    };

    let mut parser = Parser::new();
    parser
        .set_language(&lang)
        .map_err(|e| Error::TreeSitter(e.to_string()))?;
    let tree = match parser.parse(content, None) {
        Some(t) => t,
        None => return Ok(extract_symbols_regex(file_path, language, content)),
    };

    let (query_src, label) = query_for_language(language);
    let query = Query::new(&lang, query_src).map_err(|e| Error::TreeSitter(e.to_string()))?;
    let mut cursor = QueryCursor::new();
    let mut symbols = Vec::new();

    let mut matches = cursor.matches(&query, tree.root_node(), content.as_bytes());
    while let Some(m) = matches.next() {
        let mut name = String::new();
        let mut node = None;
        for capture in m.captures {
            let capture_name = query.capture_names()[capture.index as usize];
            let n = capture.node;
            if capture_name == "name" {
                name = n
                    .utf8_text(content.as_bytes())
                    .map_err(|e| Error::TreeSitter(e.to_string()))?
                    .to_string();
            }
            if capture_name == "definition" {
                node = Some(n);
            }
        }
        if let (Some(n), true) = (node, !name.is_empty()) {
            let start = n.start_position();
            let end = n.end_position();
            let sym_label = label_for_kind(n.kind()).unwrap_or(label);
            let line = (start.row + 1) as i64;
            let qn = qualified_name(file_path, sym_label, &name, line);
            let sig = extract_signature(content, start.row, end.row);
            symbols.push(Symbol {
                qualified_name: qn,
                name,
                label: sym_label.to_string(),
                file_path: file_path.to_string(),
                line_start: (start.row + 1) as i64,
                line_end: (end.row + 1) as i64,
                signature: Some(sig),
                properties_json: None,
            });
        }
    }

    if symbols.is_empty() {
        return Ok(extract_symbols_regex(file_path, language, content));
    }
    Ok(symbols)
}

fn language_for_ts(language: &str) -> Option<Language> {
    Some(match language {
        "rust" => tree_sitter_rust::LANGUAGE.into(),
        "python" => tree_sitter_python::LANGUAGE.into(),
        "javascript" | "jsx" => tree_sitter_javascript::LANGUAGE.into(),
        "typescript" | "tsx" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "go" => tree_sitter_go::LANGUAGE.into(),
        "java" => tree_sitter_java::LANGUAGE.into(),
        "c" => tree_sitter_c::LANGUAGE.into(),
        "cpp" => tree_sitter_cpp::LANGUAGE.into(),
        _ => return None,
    })
}

fn query_for_language(language: &str) -> (&'static str, &'static str) {
    match language {
        "rust" => (
            r#"
            (function_item name: (identifier) @name) @definition
            (impl_item type: (type_identifier) @name) @definition
            (struct_item name: (type_identifier) @name) @definition
            (enum_item name: (type_identifier) @name) @definition
            (trait_item name: (type_identifier) @name) @definition
            "#,
            "Function",
        ),
        "python" => (
            r#"
            (function_definition name: (identifier) @name) @definition
            (class_definition name: (identifier) @name) @definition
            "#,
            "Function",
        ),
        "javascript" | "jsx" | "typescript" | "tsx" => (
            r#"
            (function_declaration name: (identifier) @name) @definition
            (method_definition name: (property_identifier) @name) @definition
            (class_declaration name: (identifier) @name) @definition
            "#,
            "Function",
        ),
        "go" => (
            r#"
            (function_declaration name: (identifier) @name) @definition
            (method_declaration name: (field_identifier) @name) @definition
            (type_declaration (type_spec name: (type_identifier) @name)) @definition
            "#,
            "Function",
        ),
        "java" => (
            r#"
            (method_declaration name: (identifier) @name) @definition
            (class_declaration name: (identifier) @name) @definition
            (interface_declaration name: (identifier) @name) @definition
            "#,
            "Function",
        ),
        "c" => (
            r#"
            (function_definition declarator: (function_declarator declarator: (identifier) @name)) @definition
            (struct_specifier name: (type_identifier) @name) @definition
            "#,
            "Function",
        ),
        "cpp" => (
            r#"
            (function_definition declarator: (function_declarator declarator: (identifier) @name)) @definition
            (class_specifier name: (type_identifier) @name) @definition
            "#,
            "Function",
        ),
        _ => (r#"(identifier) @name"#, "Function"),
    }
}

fn extract_signature(content: &str, start_row: usize, end_row: usize) -> String {
    let lines: Vec<&str> = content.lines().collect();
    if start_row >= lines.len() {
        return String::new();
    }
    let first = lines[start_row];
    if first.contains('{') || first.len() > 200 {
        return first.chars().take(200).collect();
    }
    let mut sig = first.to_string();
    let end = end_row
        .min(start_row + 5)
        .min(lines.len().saturating_sub(1));
    for line in lines.iter().take(end + 1).skip(start_row + 1) {
        sig.push(' ');
        sig.push_str(line.trim());
        if sig.contains('{') || sig.contains(':') && language_looks_complete(&sig) {
            break;
        }
    }
    sig.chars().take(300).collect()
}

fn language_looks_complete(sig: &str) -> bool {
    sig.contains("->") || sig.ends_with(':') || sig.ends_with('{')
}

fn label_for_kind(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "function_item"
        | "function_definition"
        | "function_declaration"
        | "method_declaration"
        | "method_definition" => "Function",
        "struct_item"
        | "struct_specifier"
        | "class_specifier"
        | "class_definition"
        | "class_declaration"
        | "enum_item"
        | "trait_item"
        | "impl_item"
        | "interface_declaration" => "Class",
        _ => return None,
    })
}

fn extract_symbols_regex(file_path: &str, language: &str, content: &str) -> Vec<Symbol> {
    let patterns: &[(&str, &str)] = match language {
        "rust" => &[
            (r"(?m)^\s*(?:pub\s+)?fn\s+(\w+)", "Function"),
            (r"(?m)^\s*(?:pub\s+)?struct\s+(\w+)", "Class"),
            (r"(?m)^\s*(?:pub\s+)?enum\s+(\w+)", "Class"),
            (r"(?m)^\s*(?:pub\s+)?trait\s+(\w+)", "Class"),
            (r"(?m)^\s*impl(?:<[^>]+>)?\s+(\w+)", "Class"),
        ],
        "python" => &[
            (r"(?m)^\s*(?:async\s+)?def\s+(\w+)", "Function"),
            (r"(?m)^\s*class\s+(\w+)", "Class"),
        ],
        "go" => &[
            (r"(?m)^func\s+(?:\([^)]+\)\s+)?(\w+)", "Function"),
            (r"(?m)^type\s+(\w+)\s+struct", "Class"),
        ],
        "java" => &[
            (
                r"(?m)^\s*(?:public|private|protected)?\s+\w[\w<>,\s]*\s+(\w+)\s*\(",
                "Function",
            ),
            (r"(?m)^\s*(?:public|private)?\s*class\s+(\w+)", "Class"),
            (r"(?m)^\s*(?:public|private)?\s*interface\s+(\w+)", "Class"),
        ],
        "javascript" | "typescript" | "jsx" | "tsx" => &[
            (
                r"(?m)^\s*(?:export\s+)?(?:async\s+)?function\s+(\w+)",
                "Function",
            ),
            (r"(?m)^\s*(?:export\s+)?class\s+(\w+)", "Class"),
            (
                r"(?m)^\s*(?:export\s+)?const\s+(\w+)\s*=\s*(?:async\s+)?\(",
                "Function",
            ),
        ],
        _ => &[(r"(?m)^\s*(?:fn|def|func)\s+(\w+)", "Function")],
    };

    let mut symbols = Vec::new();
    for (pat, label) in patterns {
        if let Ok(re) = regex::Regex::new(pat) {
            for cap in re.captures_iter(content) {
                if let Some(name) = cap.get(1) {
                    let name = name.as_str().to_string();
                    let line = content[..cap.get(0).unwrap().start()].lines().count() as i64 + 1;
                    symbols.push(Symbol {
                        qualified_name: qualified_name(file_path, label, &name, line),
                        name,
                        label: label.to_string(),
                        file_path: file_path.to_string(),
                        line_start: line,
                        line_end: line + 1,
                        signature: cap.get(0).map(|m| m.as_str().chars().take(200).collect()),
                        properties_json: None,
                    });
                }
            }
        }
    }
    symbols
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_rust_functions() {
        let src = "pub fn hello() {}\nstruct Foo {}\n";
        let syms = extract_symbols("lib.rs", "rust", src).unwrap();
        assert!(!syms.is_empty());
        assert!(syms.iter().any(|s| s.name == "hello"));
    }

    #[test]
    fn extracts_java_methods() {
        let src = "public class App {\n  public void run() {}\n}\n";
        let syms = extract_symbols("App.java", "java", src).unwrap();
        assert!(syms.iter().any(|s| s.name == "run" || s.name == "App"));
    }

    #[test]
    fn extracts_python_functions() {
        let src = "def foo(x):\n    pass\n\nclass Bar:\n    pass\n";
        let syms = extract_symbols("mod.py", "python", src).unwrap();
        assert!(syms.iter().any(|s| s.name == "foo"));
        assert!(syms.iter().any(|s| s.name == "Bar"));
    }
}
