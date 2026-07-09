//! INHERITS / IMPLEMENTS / DECORATES extraction.
//!
//! Strategy order (OOP, aligned with CALLS):
//! 1. [`AstInheritanceResolver`] when tree-sitter parse succeeds
//! 2. Regex patterns for language-specific fallbacks
//! 3. Decorator attribute patterns (always regex)

use super::inheritance_ast::{resolve_target_name, AstInheritanceResolver};
use crate::store::{Edge, Symbol};
use regex::Regex;
use std::collections::{HashMap, HashSet};

/// Trait for inheritance resolution strategies.
pub trait InheritanceResolver: Send + Sync {
    fn name(&self) -> &'static str;
    fn resolve(
        &self,
        file_path: &str,
        language: &str,
        content: &str,
        local_index: &HashMap<String, String>,
        project_index: &HashMap<String, Vec<String>>,
    ) -> Option<Vec<Edge>>;
}

pub struct AstInheritanceStrategy;

impl InheritanceResolver for AstInheritanceStrategy {
    fn name(&self) -> &'static str {
        "ast"
    }

    fn resolve(
        &self,
        file_path: &str,
        language: &str,
        content: &str,
        local_index: &HashMap<String, String>,
        project_index: &HashMap<String, Vec<String>>,
    ) -> Option<Vec<Edge>> {
        let resolver = AstInheritanceResolver::for_language(language)?;
        resolver.try_resolve(file_path, content, local_index, project_index)
    }
}

pub struct RegexInheritanceStrategy;

impl InheritanceResolver for RegexInheritanceStrategy {
    fn name(&self) -> &'static str {
        "regex"
    }

    fn resolve(
        &self,
        file_path: &str,
        language: &str,
        content: &str,
        local_index: &HashMap<String, String>,
        project_index: &HashMap<String, Vec<String>>,
    ) -> Option<Vec<Edge>> {
        Some(extract_inheritance_regex(
            file_path,
            language,
            content,
            local_index,
            project_index,
        ))
    }
}

/// Composite pipeline: AST first, then regex.
pub struct InheritancePipeline {
    strategies: Vec<Box<dyn InheritanceResolver>>,
}

impl Default for InheritancePipeline {
    fn default() -> Self {
        Self {
            strategies: vec![
                Box::new(AstInheritanceStrategy),
                Box::new(RegexInheritanceStrategy),
            ],
        }
    }
}

impl InheritancePipeline {
    pub fn resolve(
        &self,
        file_path: &str,
        language: &str,
        content: &str,
        local_index: &HashMap<String, String>,
        project_index: &HashMap<String, Vec<String>>,
    ) -> Vec<Edge> {
        for strategy in &self.strategies {
            if let Some(edges) =
                strategy.resolve(file_path, language, content, local_index, project_index)
            {
                return edges;
            }
        }
        Vec::new()
    }
}

/// Build name → qualified_name map for symbols in one file (prefer Class labels).
pub fn build_file_name_index(symbols: &[Symbol], file_path: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for s in symbols.iter().filter(|s| s.file_path == file_path) {
        // Prefer Class over Function when same simple name exists
        if s.label == "Class" || !map.contains_key(&s.name) {
            map.insert(s.name.clone(), s.qualified_name.clone());
        }
    }
    map
}

/// Project-wide name → candidate QNs (for unique cross-file resolution).
pub fn build_project_name_index(symbols: &[Symbol]) -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for s in symbols {
        if !matches!(s.label.as_str(), "Class" | "Interface" | "Function") {
            continue;
        }
        map.entry(s.name.clone())
            .or_default()
            .push(s.qualified_name.clone());
    }
    map
}

/// Extract INHERITS, IMPLEMENTS, and DECORATES edges.
///
/// `file_symbols` are symbols defined in this file; `all_symbols` powers
/// project-wide unique name resolution for parents/traits.
pub fn extract_inheritance_edges(
    file_path: &str,
    language: &str,
    content: &str,
    file_symbols: &[Symbol],
) -> Vec<Edge> {
    extract_inheritance_edges_with_project(file_path, language, content, file_symbols, file_symbols)
}

/// Full extractor with an explicit project symbol list.
pub fn extract_inheritance_edges_with_project(
    file_path: &str,
    language: &str,
    content: &str,
    file_symbols: &[Symbol],
    all_symbols: &[Symbol],
) -> Vec<Edge> {
    let local = build_file_name_index(file_symbols, file_path);
    let project = build_project_name_index(all_symbols);
    let mut edges =
        InheritancePipeline::default().resolve(file_path, language, content, &local, &project);
    // Decorators always use regex (language-agnostic-ish attributes).
    extract_decorators(file_path, content, file_symbols, &mut edges);
    edges
}

fn extract_inheritance_regex(
    file_path: &str,
    language: &str,
    content: &str,
    local: &HashMap<String, String>,
    project: &HashMap<String, Vec<String>>,
) -> Vec<Edge> {
    let mut edges = Vec::new();
    let mut seen = HashSet::new();

    let mut ctx = ExtractCtx {
        file_path,
        content,
        local,
        project,
        edges: &mut edges,
        seen: &mut seen,
    };

    match language {
        "rust" => {
            extract_pairs(
                &mut ctx,
                r"(?m)^\s*impl(?:<[^>]+>)?\s+([\w:]+)\s+for\s+(\w+)",
                "IMPLEMENTS",
                |cap| Some((cap.get(2)?.as_str(), cap.get(1)?.as_str())),
            );
        }
        "python" => {
            // class Child(Parent) or class Child(Parent, Mixin)
            if let Ok(re) = Regex::new(r"(?m)^\s*class\s+(\w+)\s*\(([^)]*)\)\s*:") {
                for cap in re.captures_iter(content) {
                    let Some(child) = cap.get(1) else { continue };
                    let Some(bases) = cap.get(2) else { continue };
                    let Some(src) = local.get(child.as_str()) else {
                        continue;
                    };
                    for base in bases.as_str().split(',') {
                        let base = base.trim();
                        if base.is_empty() || base == "object" || base.starts_with("metaclass") {
                            continue;
                        }
                        let base_name = base.rsplit('.').next().unwrap_or(base);
                        let dst = resolve_target_name(base_name, local, project, file_path);
                        push_edge("INHERITS", src, &dst, "regex", &mut edges, &mut seen);
                    }
                }
            }
        }
        "java" => {
            extract_pairs(
                &mut ctx,
                r"(?m)^\s*(?:public\s+|protected\s+|private\s+)?(?:abstract\s+|final\s+)?class\s+(\w+)\s+extends\s+(\w+)",
                "INHERITS",
                |cap| Some((cap.get(1)?.as_str(), cap.get(2)?.as_str())),
            );
            if let Ok(re) = Regex::new(
                r"(?m)^\s*(?:public\s+|protected\s+|private\s+)?(?:abstract\s+|final\s+)?class\s+(\w+)(?:\s+extends\s+\w+)?\s+implements\s+([\w,\s]+)",
            ) {
                for cap in re.captures_iter(content) {
                    let Some(child) = cap.get(1) else { continue };
                    let Some(ifaces) = cap.get(2) else { continue };
                    let Some(src) = local.get(child.as_str()) else {
                        continue;
                    };
                    for iface in ifaces.as_str().split(',') {
                        let iface = iface.trim();
                        if iface.is_empty() {
                            continue;
                        }
                        let dst = resolve_target_name(iface, local, project, file_path);
                        push_edge("IMPLEMENTS", src, &dst, "regex", &mut edges, &mut seen);
                    }
                }
            }
        }
        "kotlin" => {
            // class Child : Parent(), IFace
            if let Ok(re) = Regex::new(
                r"(?m)^\s*(?:open\s+|abstract\s+|data\s+|sealed\s+|private\s+|internal\s+)*class\s+(\w+)[^{]*:\s*([^{\n]+)",
            ) {
                for cap in re.captures_iter(content) {
                    let Some(child) = cap.get(1) else { continue };
                    let Some(bases) = cap.get(2) else { continue };
                    let Some(src) = local.get(child.as_str()) else {
                        continue;
                    };
                    for base in bases.as_str().split(',') {
                        let base = base.trim();
                        if base.is_empty() {
                            continue;
                        }
                        // Strip constructor args: Parent() / Parent(x)
                        let base_name = base
                            .split('(')
                            .next()
                            .unwrap_or(base)
                            .trim()
                            .rsplit('.')
                            .next()
                            .unwrap_or(base);
                        let edge_type = if base.contains('(') {
                            "INHERITS"
                        } else {
                            "IMPLEMENTS"
                        };
                        let dst = resolve_target_name(base_name, local, project, file_path);
                        push_edge(edge_type, src, &dst, "regex", &mut edges, &mut seen);
                    }
                }
            }
        }
        "javascript" | "typescript" | "tsx" | "jsx" => {
            extract_pairs(
                &mut ctx,
                r"(?m)^\s*(?:export\s+)?class\s+(\w+)\s+extends\s+(\w+)",
                "INHERITS",
                |cap| Some((cap.get(1)?.as_str(), cap.get(2)?.as_str())),
            );
            extract_pairs(
                &mut ctx,
                r"(?m)^\s*(?:export\s+)?class\s+(\w+)[^{]*\bimplements\s+([\w,\s]+)",
                "IMPLEMENTS",
                |cap| {
                    // only first iface via extract_pairs single parent — handle multi below
                    let child = cap.get(1)?.as_str();
                    let ifaces = cap.get(2)?.as_str();
                    let first = ifaces.split(',').next()?.trim();
                    Some((child, first))
                },
            );
        }
        "csharp" => {
            // class Child : Parent, IFace
            if let Ok(re) =
                Regex::new(r"(?m)^\s*(?:public\s+|internal\s+|private\s+)?class\s+(\w+)\s*:\s*([^{\n]+)")
            {
                for cap in re.captures_iter(content) {
                    let Some(child) = cap.get(1) else { continue };
                    let Some(bases) = cap.get(2) else { continue };
                    let Some(src) = local.get(child.as_str()) else {
                        continue;
                    };
                    for (i, base) in bases.as_str().split(',').enumerate() {
                        let base = base.trim();
                        if base.is_empty() {
                            continue;
                        }
                        let edge_type = if i == 0 && !base.starts_with('I') {
                            "INHERITS"
                        } else if base.starts_with('I') {
                            "IMPLEMENTS"
                        } else if i == 0 {
                            "INHERITS"
                        } else {
                            "IMPLEMENTS"
                        };
                        let dst = resolve_target_name(base, local, project, file_path);
                        push_edge(edge_type, src, &dst, "regex", &mut edges, &mut seen);
                    }
                }
            }
        }
        "ruby" => {
            extract_pairs(
                &mut ctx,
                r"(?m)^\s*class\s+(\w+)\s*<\s*(\w+)",
                "INHERITS",
                |cap| Some((cap.get(1)?.as_str(), cap.get(2)?.as_str())),
            );
        }
        "cpp" | "c" => {
            extract_pairs(
                &mut ctx,
                r"(?m)^\s*(?:class|struct)\s+(\w+)\s*:\s*(?:public|protected|private)\s+(\w+)",
                "INHERITS",
                |cap| Some((cap.get(1)?.as_str(), cap.get(2)?.as_str())),
            );
        }
        "go" => {
            // type Child struct { Parent }  — embedded field without name
            if let Ok(re) = Regex::new(
                r"(?m)^\s*type\s+(\w+)\s+struct\s*\{[^}]*?(?:^|\n)\s*([A-Z]\w*)\s*(?:\n|\})",
            ) {
                for cap in re.captures_iter(content) {
                    let Some(child) = cap.get(1) else { continue };
                    let Some(parent) = cap.get(2) else { continue };
                    let Some(src) = local.get(child.as_str()) else {
                        continue;
                    };
                    let dst = resolve_target_name(parent.as_str(), local, project, file_path);
                    push_edge("INHERITS", src, &dst, "regex", &mut edges, &mut seen);
                }
            }
        }
        _ => {}
    }

    edges
}

struct ExtractCtx<'a> {
    file_path: &'a str,
    content: &'a str,
    local: &'a HashMap<String, String>,
    project: &'a HashMap<String, Vec<String>>,
    edges: &'a mut Vec<Edge>,
    seen: &'a mut HashSet<(String, String, String)>,
}

fn extract_pairs<'a>(
    ctx: &mut ExtractCtx<'a>,
    pattern: &str,
    edge_type: &str,
    names: impl Fn(regex::Captures<'a>) -> Option<(&'a str, &'a str)>,
) {
    let Ok(re) = Regex::new(pattern) else {
        return;
    };
    for cap in re.captures_iter(ctx.content) {
        let Some((child, parent)) = names(cap) else {
            continue;
        };
        let Some(src) = ctx.local.get(child) else {
            continue;
        };
        let dst = resolve_target_name(parent, ctx.local, ctx.project, ctx.file_path);
        push_edge(edge_type, src, &dst, "regex", ctx.edges, ctx.seen);
    }
}

fn extract_decorators(
    file_path: &str,
    content: &str,
    symbols: &[Symbol],
    edges: &mut Vec<Edge>,
) {
    let mut seen = HashSet::new();
    // already-seen from AST edges
    for e in edges.iter() {
        seen.insert((
            e.src_qn.clone(),
            e.dst_qn.clone(),
            e.edge_type.clone(),
        ));
    }
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
        push_edge("DECORATES", &target, &dst, "regex", edges, &mut seen);
    }
}

fn push_edge(
    edge_type: &str,
    src: &str,
    dst: &str,
    method: &str,
    edges: &mut Vec<Edge>,
    seen: &mut HashSet<(String, String, String)>,
) {
    let key = (src.to_string(), dst.to_string(), edge_type.to_string());
    if seen.insert(key) {
        edges.push(Edge {
            src_qn: src.to_string(),
            dst_qn: dst.to_string(),
            edge_type: edge_type.into(),
            properties_json: Some(format!(
                r#"{{"confidence":"{}","method":"{method}"}}"#,
                if method == "ast" { "high" } else { "resolved" }
            )),
        });
    }
}

fn line_number(content: &str, byte_offset: usize) -> i64 {
    content[..byte_offset.min(content.len())].lines().count() as i64 + 1
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
            qualified_name: format!("{file}::{label}::{name}@L{line}"),
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
        assert!(
            edges.iter().any(|e| {
                e.edge_type == "INHERITS"
                    && e.src_qn.contains("Child")
                    && e.properties_json
                        .as_ref()
                        .is_some_and(|p| p.contains("ast") || p.contains("regex"))
            }),
            "{edges:?}"
        );
    }

    #[test]
    fn extracts_rust_implements() {
        let src = "struct Greeter;\nimpl Display for Greeter {\n}\n";
        let symbols = vec![
            sym("g.rs", "Greeter", "Class", 1),
            sym("g.rs", "Display", "Class", 2),
        ];
        let edges = extract_inheritance_edges("g.rs", "rust", src, &symbols);
        assert!(
            edges.iter().any(|e| e.edge_type == "IMPLEMENTS"),
            "{edges:?}"
        );
    }

    #[test]
    fn extracts_typescript_extends_ast() {
        let src = "class Child extends Parent {\n}\n";
        let symbols = vec![
            sym("a.ts", "Child", "Class", 1),
            sym("a.ts", "Parent", "Class", 5),
        ];
        let edges = extract_inheritance_edges("a.ts", "typescript", src, &symbols);
        assert!(
            edges.iter().any(|e| e.edge_type == "INHERITS"),
            "{edges:?}"
        );
    }

    #[test]
    fn project_unique_parent_resolves_cross_file() {
        let src = "class Child(Parent):\n    pass\n";
        let file_symbols = vec![sym("child.py", "Child", "Class", 1)];
        let all = vec![
            sym("child.py", "Child", "Class", 1),
            sym("parent.py", "Parent", "Class", 1),
        ];
        let edges = extract_inheritance_edges_with_project(
            "child.py",
            "python",
            src,
            &file_symbols,
            &all,
        );
        assert!(
            edges.iter().any(|e| {
                e.edge_type == "INHERITS" && e.dst_qn.contains("parent.py::Class::Parent")
            }),
            "{edges:?}"
        );
    }
}
