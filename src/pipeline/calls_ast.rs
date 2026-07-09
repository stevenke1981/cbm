//! Multi-language tree-sitter CALLS resolution (aligned with DeusData pass_calls).
//!
//! OOP design: each language is an [`AstCallProfile`]; [`AstCallResolver`] runs a
//! shared match pipeline so Rust/Python/JS/TS/Go/Java/C/C++ share one code path.

use super::calls::make_call_edge;
use super::registry::FunctionRegistry;
use crate::store::{Edge, Symbol};
use std::collections::HashSet;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Parser, Query, QueryCursor};

/// Language-specific tree-sitter call-query profile.
#[derive(Debug, Clone, Copy)]
pub struct AstCallProfile {
    pub language_id: &'static str,
    pub query_src: &'static str,
    language_fn: fn() -> Language,
}

impl AstCallProfile {
    pub fn language(&self) -> Language {
        (self.language_fn)()
    }

    pub fn for_id(language: &str) -> Option<&'static AstCallProfile> {
        PROFILES.iter().find(|p| {
            p.language_id == language
                || (p.language_id == "javascript" && matches!(language, "jsx"))
                || (p.language_id == "typescript" && matches!(language, "tsx"))
                || (p.language_id == "cpp" && matches!(language, "c" | "cc" | "cxx"))
                || (p.language_id == "shell" && matches!(language, "bash"))
                || (p.language_id == "csharp" && matches!(language, "c_sharp" | "cs"))
        })
    }
}

fn lang_rust() -> Language {
    tree_sitter_rust::LANGUAGE.into()
}
fn lang_python() -> Language {
    tree_sitter_python::LANGUAGE.into()
}
fn lang_javascript() -> Language {
    tree_sitter_javascript::LANGUAGE.into()
}
fn lang_typescript() -> Language {
    tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
}
fn lang_go() -> Language {
    tree_sitter_go::LANGUAGE.into()
}
fn lang_java() -> Language {
    tree_sitter_java::LANGUAGE.into()
}
fn lang_c() -> Language {
    tree_sitter_c::LANGUAGE.into()
}
fn lang_cpp() -> Language {
    tree_sitter_cpp::LANGUAGE.into()
}
fn lang_ruby() -> Language {
    tree_sitter_ruby::LANGUAGE.into()
}
fn lang_csharp() -> Language {
    tree_sitter_c_sharp::LANGUAGE.into()
}
fn lang_php() -> Language {
    tree_sitter_php::LANGUAGE_PHP.into()
}
fn lang_bash() -> Language {
    tree_sitter_bash::LANGUAGE.into()
}
fn lang_kotlin() -> Language {
    tree_sitter_kotlin_ng::LANGUAGE.into()
}
fn lang_swift() -> Language {
    tree_sitter_swift::LANGUAGE.into()
}

const PROFILES: &[AstCallProfile] = &[
    AstCallProfile {
        language_id: "rust",
        language_fn: lang_rust,
        query_src: r#"
(call_expression
  function: (identifier) @callee)
(call_expression
  function: (field_expression
    field: (field_identifier) @method))
(call_expression
  function: (scoped_identifier
    name: (identifier) @scoped))
"#,
    },
    AstCallProfile {
        language_id: "python",
        language_fn: lang_python,
        query_src: r#"
(call
  function: (identifier) @callee)
(call
  function: (attribute
    attribute: (identifier) @method))
"#,
    },
    AstCallProfile {
        language_id: "javascript",
        language_fn: lang_javascript,
        query_src: r#"
(call_expression
  function: (identifier) @callee)
(call_expression
  function: (member_expression
    property: (property_identifier) @method))
"#,
    },
    AstCallProfile {
        language_id: "typescript",
        language_fn: lang_typescript,
        query_src: r#"
(call_expression
  function: (identifier) @callee)
(call_expression
  function: (member_expression
    property: (property_identifier) @method))
"#,
    },
    AstCallProfile {
        language_id: "go",
        language_fn: lang_go,
        query_src: r#"
(call_expression
  function: (identifier) @callee)
(call_expression
  function: (selector_expression
    field: (field_identifier) @method))
"#,
    },
    AstCallProfile {
        language_id: "java",
        language_fn: lang_java,
        query_src: r#"
(method_invocation
  name: (identifier) @callee)
"#,
    },
    AstCallProfile {
        language_id: "c",
        language_fn: lang_c,
        query_src: r#"
(call_expression
  function: (identifier) @callee)
(call_expression
  function: (field_expression
    field: (field_identifier) @method))
"#,
    },
    AstCallProfile {
        language_id: "cpp",
        language_fn: lang_cpp,
        query_src: r#"
(call_expression
  function: (identifier) @callee)
(call_expression
  function: (field_expression
    field: (field_identifier) @method))
(call_expression
  function: (qualified_identifier
    name: (identifier) @scoped))
"#,
    },
    AstCallProfile {
        language_id: "ruby",
        language_fn: lang_ruby,
        query_src: r#"
(call
  method: (identifier) @callee)
(command
  method: (identifier) @callee)
"#,
    },
    AstCallProfile {
        language_id: "csharp",
        language_fn: lang_csharp,
        query_src: r#"
(invocation_expression
  function: (identifier) @callee)
(invocation_expression
  function: (member_access_expression
    name: (identifier) @method))
"#,
    },
    AstCallProfile {
        language_id: "php",
        language_fn: lang_php,
        query_src: r#"
(function_call_expression
  function: (name) @callee)
(member_call_expression
  name: (name) @method)
"#,
    },
    AstCallProfile {
        language_id: "shell",
        language_fn: lang_bash,
        query_src: r#"
(command
  name: (command_name (word) @callee))
"#,
    },
    AstCallProfile {
        language_id: "kotlin",
        language_fn: lang_kotlin,
        query_src: r#"
(call_expression
  (identifier) @callee)
(call_expression
  (navigation_expression
    (identifier) @method))
"#,
    },
    AstCallProfile {
        language_id: "swift",
        language_fn: lang_swift,
        query_src: r#"
(call_expression
  (simple_identifier) @callee)
(call_expression
  (navigation_expression
    (navigation_suffix (simple_identifier) @method)))
"#,
    },
];

/// Shared AST call resolver for one language profile.
pub struct AstCallResolver {
    profile: &'static AstCallProfile,
}

impl AstCallResolver {
    pub fn for_language(language: &str) -> Option<Self> {
        AstCallProfile::for_id(language).map(|profile| Self { profile })
    }

    pub fn language_id(&self) -> &'static str {
        self.profile.language_id
    }

    /// Parse and resolve CALLS. Returns `None` if parse/query setup fails so
    /// callers can fall back to regex. Empty `Some(vec![])` means parse OK but
    /// no resolvable calls (do not fall back).
    pub fn try_resolve(
        &self,
        symbols: &[Symbol],
        content: &str,
        registry: &FunctionRegistry,
        import_files: &HashSet<String>,
    ) -> Option<Vec<Edge>> {
        let lang = self.profile.language();
        let mut parser = Parser::new();
        if parser.set_language(&lang).is_err() {
            return None;
        }
        let tree = parser.parse(content, None)?;
        let query = Query::new(&lang, self.profile.query_src).ok()?;
        Some(self.collect_edges(symbols, content, registry, import_files, &tree, &query))
    }

    fn collect_edges(
        &self,
        symbols: &[Symbol],
        content: &str,
        registry: &FunctionRegistry,
        import_files: &HashSet<String>,
        tree: &tree_sitter::Tree,
        query: &Query,
    ) -> Vec<Edge> {
        let mut cursor = QueryCursor::new();
        let mut edges = Vec::new();
        let mut seen: HashSet<(String, String)> = HashSet::new();
        let bytes = content.as_bytes();

        // Collect all call sites once (name + 1-based line).
        let mut call_sites: Vec<(String, i64)> = Vec::new();
        let mut matches = cursor.matches(query, tree.root_node(), bytes);
        while let Some(m) = matches.next() {
            let mut callee = String::new();
            let mut line = 0usize;
            for cap in m.captures {
                let name = query.capture_names()[cap.index as usize];
                if matches!(name, "callee" | "method" | "scoped") {
                    callee = cap.node.utf8_text(bytes).unwrap_or("").to_string();
                    line = cap.node.start_position().row;
                }
            }
            if callee.is_empty() || is_common_keyword(&callee) {
                continue;
            }
            call_sites.push((callee, (line + 1) as i64));
        }

        let functions: Vec<&Symbol> = symbols
            .iter()
            .filter(|s| matches!(s.label.as_str(), "Function" | "Method"))
            .collect();

        for sym in functions {
            for (callee, call_line) in &call_sites {
                if *call_line < sym.line_start || *call_line > sym.line_end {
                    continue;
                }
                if callee == &sym.name {
                    continue;
                }
                let resolutions = registry.resolve(callee, &sym.file_path, import_files);
                for res in resolutions {
                    if res.qualified_name == sym.qualified_name {
                        continue;
                    }
                    let key = (sym.qualified_name.clone(), res.qualified_name.clone());
                    if seen.insert(key.clone()) {
                        let mut edge = make_call_edge(&key.0, &key.1, "ast", res.strategy);
                        // Enrich with language id
                        if let Some(props) = edge.properties_json.as_mut() {
                            if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(props) {
                                if let Some(obj) = v.as_object_mut() {
                                    obj.insert(
                                        "language".into(),
                                        serde_json::json!(self.profile.language_id),
                                    );
                                }
                                *props = v.to_string();
                            }
                        }
                        edges.push(edge);
                    }
                }
            }
        }
        edges
    }
}

fn is_common_keyword(name: &str) -> bool {
    matches!(
        name,
        "if" | "for"
            | "while"
            | "match"
            | "return"
            | "let"
            | "loop"
            | "move"
            | "async"
            | "await"
            | "const"
            | "var"
            | "new"
            | "self"
            | "super"
            | "this"
            | "print"
            | "println"
            | "format"
            | "typeof"
            | "sizeof"
            | "switch"
            | "case"
            | "default"
            | "break"
            | "continue"
            | "class"
            | "struct"
            | "enum"
            | "import"
            | "from"
            | "def"
            | "fn"
            | "func"
            | "package"
            | "true"
            | "false"
            | "null"
            | "None"
            | "True"
            | "False"
            | "undefined"
            | "console"
            | "require"
            | "include"
            | "printf"
            | "malloc"
            | "free"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol_id::qualified_name;

    fn function_sym(file: &str, name: &str, line: i64, end: i64) -> Symbol {
        Symbol {
            qualified_name: qualified_name(file, "Function", name, line),
            name: name.into(),
            label: "Function".into(),
            file_path: file.into(),
            line_start: line,
            line_end: end,
            signature: None,
            properties_json: None,
        }
    }

    #[test]
    fn python_ast_resolves_local_call() {
        let symbols = vec![
            function_sym("main.py", "helper", 1, 2),
            function_sym("main.py", "main", 4, 5),
        ];
        let src = "def helper():\n    pass\n\ndef main():\n    helper()\n";
        let reg = FunctionRegistry::from_symbols(&symbols);
        let r = AstCallResolver::for_language("python").unwrap();
        let edges = r
            .try_resolve(&symbols, src, &reg, &HashSet::new())
            .unwrap();
        assert!(edges.iter().any(|e| e.dst_qn.contains("helper")));
        assert!(edges[0]
            .properties_json
            .as_ref()
            .is_some_and(|p| p.contains("ast")));
    }

    #[test]
    fn javascript_ast_resolves_local_call() {
        let symbols = vec![
            function_sym("main.js", "helper", 1, 1),
            function_sym("main.js", "main", 2, 2),
        ];
        let src = "function helper() {}\nfunction main() { helper(); }\n";
        let reg = FunctionRegistry::from_symbols(&symbols);
        let r = AstCallResolver::for_language("javascript").unwrap();
        let edges = r
            .try_resolve(&symbols, src, &reg, &HashSet::new())
            .unwrap();
        assert_eq!(edges.len(), 1);
    }

    #[test]
    fn go_ast_resolves_local_call() {
        let symbols = vec![
            function_sym("main.go", "helper", 2, 2),
            function_sym("main.go", "main", 3, 3),
        ];
        let src = "package main\nfunc helper() {}\nfunc main() { helper() }\n";
        let reg = FunctionRegistry::from_symbols(&symbols);
        let r = AstCallResolver::for_language("go").unwrap();
        let edges = r
            .try_resolve(&symbols, src, &reg, &HashSet::new())
            .unwrap();
        assert_eq!(edges.len(), 1);
    }
}
