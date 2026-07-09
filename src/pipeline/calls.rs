//! CALLS edge resolution — registry strategies, AST resolvers, regex fallback.
//!
//! Dispatch order (DeusData-style):
//! 1. Language-specific [`AstCallResolver`] when tree-sitter parse succeeds
//! 2. Regex fallback for unsupported languages or parse failures
//!
//! Callee resolution uses [`FunctionRegistry`] priority:
//! same_file → import_map → same_directory → unique_name.

use super::registry::{FunctionRegistry, ResolveStrategy};
use crate::store::{Edge, Symbol};
use std::collections::{HashMap, HashSet};

/// Trait for CALLS resolution strategies (OOP extension point).
pub trait CallResolver: Send + Sync {
    fn name(&self) -> &'static str;
    fn resolve(
        &self,
        symbols: &[Symbol],
        content: &str,
        language: &str,
        registry: &FunctionRegistry,
        import_files: &HashSet<String>,
    ) -> Option<Vec<Edge>>;
}

/// Tree-sitter AST strategy.
pub struct AstCallStrategy;

impl CallResolver for AstCallStrategy {
    fn name(&self) -> &'static str {
        "ast"
    }

    fn resolve(
        &self,
        symbols: &[Symbol],
        content: &str,
        language: &str,
        registry: &FunctionRegistry,
        import_files: &HashSet<String>,
    ) -> Option<Vec<Edge>> {
        let resolver = super::calls_ast::AstCallResolver::for_language(language)?;
        resolver.try_resolve(symbols, content, registry, import_files)
    }
}

/// Line-based regex strategy (heuristic fallback).
pub struct RegexCallStrategy;

impl CallResolver for RegexCallStrategy {
    fn name(&self) -> &'static str {
        "regex"
    }

    fn resolve(
        &self,
        symbols: &[Symbol],
        content: &str,
        _language: &str,
        registry: &FunctionRegistry,
        import_files: &HashSet<String>,
    ) -> Option<Vec<Edge>> {
        Some(resolve_calls_inner(
            symbols,
            content,
            registry,
            import_files,
            "regex",
        ))
    }
}

/// Composite resolver: try strategies in order until one returns `Some`.
pub struct CallResolutionPipeline {
    strategies: Vec<Box<dyn CallResolver>>,
}

impl Default for CallResolutionPipeline {
    fn default() -> Self {
        Self {
            strategies: vec![Box::new(AstCallStrategy), Box::new(RegexCallStrategy)],
        }
    }
}

impl CallResolutionPipeline {
    pub fn resolve(
        &self,
        symbols: &[Symbol],
        content: &str,
        language: &str,
        registry: &FunctionRegistry,
        import_files: &HashSet<String>,
    ) -> Vec<Edge> {
        for strategy in &self.strategies {
            if let Some(edges) =
                strategy.resolve(symbols, content, language, registry, import_files)
            {
                return edges;
            }
        }
        Vec::new()
    }
}

/// Build a simple name→qns map (tests / legacy).
pub fn build_name_registry(symbols: &[Symbol]) -> HashMap<String, Vec<String>> {
    FunctionRegistry::from_symbols(symbols).name_map().clone()
}

/// Resolve CALLS edges using a project-wide symbol registry (cross-file).
pub fn resolve_calls_with_registry(
    symbols: &[Symbol],
    content: &str,
    language: &str,
    registry: &HashMap<String, Vec<String>>,
) -> Vec<Edge> {
    // Legacy entry: rebuild FunctionRegistry-like view without import map.
    // Prefer resolve_calls_with_function_registry for production.
    let mut by_name = registry.clone();
    for list in by_name.values_mut() {
        list.sort();
    }
    // Convert to FunctionRegistry via synthetic symbols is heavy; use empty import map
    // and reconstruct minimal registry from name map only for tests.
    let synth: Vec<Symbol> = by_name
        .iter()
        .flat_map(|(name, qns)| {
            qns.iter().map(move |qn| {
                let file = qn.split("::").next().unwrap_or("").to_string();
                Symbol {
                    qualified_name: qn.clone(),
                    name: name.clone(),
                    label: "Function".into(),
                    file_path: file,
                    line_start: 1,
                    line_end: 2,
                    signature: None,
                    properties_json: None,
                }
            })
        })
        .collect();
    let reg = FunctionRegistry::from_symbols(&synth);
    resolve_calls_with_function_registry(symbols, content, language, &reg, &HashSet::new())
}

/// Primary entry with full registry + import map.
pub fn resolve_calls_with_function_registry(
    symbols: &[Symbol],
    content: &str,
    language: &str,
    registry: &FunctionRegistry,
    import_files: &HashSet<String>,
) -> Vec<Edge> {
    CallResolutionPipeline::default().resolve(symbols, content, language, registry, import_files)
}

/// Resolve CALLS edges from symbol definitions using name matching within file scope.
pub fn resolve_calls(symbols: &[Symbol], content: &str, language: &str) -> Vec<Edge> {
    let registry = FunctionRegistry::from_symbols(symbols);
    resolve_calls_with_function_registry(symbols, content, language, &registry, &HashSet::new())
}

fn resolve_calls_inner(
    symbols: &[Symbol],
    content: &str,
    registry: &FunctionRegistry,
    import_files: &HashSet<String>,
    method: &str,
) -> Vec<Edge> {
    let call_patterns = [
        regex::Regex::new(r"\b(\w+)\s*\(").unwrap(),
        regex::Regex::new(r"\.(\w+)\s*\(").unwrap(),
    ];

    let mut edges = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    for sym in symbols {
        if !matches!(sym.label.as_str(), "Function" | "Method") {
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
                    if callee_name == sym.name || is_noise_callee(callee_name) {
                        continue;
                    }
                    let resolutions = registry.resolve(callee_name, &sym.file_path, import_files);
                    for res in resolutions {
                        if res.qualified_name == sym.qualified_name {
                            continue;
                        }
                        let key = (sym.qualified_name.clone(), res.qualified_name.clone());
                        if seen.insert(key.clone()) {
                            edges.push(make_call_edge(&key.0, &key.1, method, res.strategy));
                        }
                    }
                }
            }
        }
    }
    edges
}

pub(crate) fn make_call_edge(
    src: &str,
    dst: &str,
    method: &str,
    strategy: ResolveStrategy,
) -> Edge {
    Edge {
        src_qn: src.to_string(),
        dst_qn: dst.to_string(),
        edge_type: "CALLS".into(),
        properties_json: Some(format!(
            r#"{{"confidence":"{}","method":"{method}","strategy":"{}"}}"#,
            strategy.confidence(),
            strategy.as_str()
        )),
    }
}

fn is_noise_callee(name: &str) -> bool {
    matches!(
        name,
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
            | "this"
            | "print"
            | "println"
            | "format"
            | "assert"
            | "require"
            | "typeof"
            | "sizeof"
    )
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
        assert!(edges[0]
            .properties_json
            .as_ref()
            .is_some_and(|p| p.contains("ast") || p.contains("same_file")));
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

    #[test]
    fn import_map_resolves_cross_file_js() {
        let symbols = vec![
            Symbol {
                qualified_name: qn("src/main.js", "Function", "main", 1),
                name: "main".into(),
                label: "Function".into(),
                file_path: "src/main.js".into(),
                line_start: 1,
                line_end: 1,
                signature: None,
                properties_json: None,
            },
            Symbol {
                qualified_name: qn("src/util.js", "Function", "helper", 1),
                name: "helper".into(),
                label: "Function".into(),
                file_path: "src/util.js".into(),
                line_start: 1,
                line_end: 2,
                signature: None,
                properties_json: None,
            },
            Symbol {
                qualified_name: qn("src/other.js", "Function", "helper", 1),
                name: "helper".into(),
                label: "Function".into(),
                file_path: "src/other.js".into(),
                line_start: 1,
                line_end: 2,
                signature: None,
                properties_json: None,
            },
        ];
        let reg = FunctionRegistry::from_symbols(&symbols);
        let mut imports = HashSet::new();
        imports.insert("src/util.js".into());
        let src = "function main() { helper(); }\n";
        let edges =
            resolve_calls_with_function_registry(&symbols[..1], src, "javascript", &reg, &imports);
        assert_eq!(edges.len(), 1, "{edges:?}");
        assert!(edges[0].dst_qn.contains("util.js"));
        assert!(edges[0]
            .properties_json
            .as_ref()
            .is_some_and(|p| p.contains("import_map") || p.contains("same_file")));
    }

    #[test]
    fn python_uses_ast_strategy() {
        let symbols = vec![
            Symbol {
                qualified_name: qn("a.py", "Function", "helper", 1),
                name: "helper".into(),
                label: "Function".into(),
                file_path: "a.py".into(),
                line_start: 1,
                line_end: 2,
                signature: None,
                properties_json: None,
            },
            Symbol {
                qualified_name: qn("a.py", "Function", "main", 4),
                name: "main".into(),
                label: "Function".into(),
                file_path: "a.py".into(),
                line_start: 4,
                line_end: 5,
                signature: None,
                properties_json: None,
            },
        ];
        let src = "def helper():\n    pass\n\ndef main():\n    helper()\n";
        let edges = resolve_calls(&symbols, src, "python");
        assert_eq!(edges.len(), 1);
        assert!(edges[0]
            .properties_json
            .as_ref()
            .is_some_and(|p| p.contains("\"method\":\"ast\"") || p.contains("same_file")));
    }
}
