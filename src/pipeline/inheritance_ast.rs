//! Multi-language tree-sitter extraction for INHERITS / IMPLEMENTS.
//!
//! OOP design mirrors CALLS: language profiles + shared match pipeline,
//! returning `None` only when parse/query setup fails (regex fallback).

use crate::store::Edge;
use std::collections::{HashMap, HashSet};
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Parser, Query, QueryCursor};

#[derive(Debug, Clone, Copy)]
pub struct InheritanceProfile {
    pub language_id: &'static str,
    pub query_src: &'static str,
    language_fn: fn() -> Language,
}

impl InheritanceProfile {
    pub fn language(&self) -> Language {
        (self.language_fn)()
    }

    pub fn for_id(language: &str) -> Option<&'static InheritanceProfile> {
        PROFILES.iter().find(|p| {
            p.language_id == language
                || (p.language_id == "javascript" && matches!(language, "jsx"))
                || (p.language_id == "typescript" && matches!(language, "tsx"))
                || (p.language_id == "cpp" && matches!(language, "c" | "cc" | "cxx"))
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
fn lang_java() -> Language {
    tree_sitter_java::LANGUAGE.into()
}
fn lang_go() -> Language {
    tree_sitter_go::LANGUAGE.into()
}
fn lang_cpp() -> Language {
    tree_sitter_cpp::LANGUAGE.into()
}
fn lang_ruby() -> Language {
    tree_sitter_ruby::LANGUAGE.into()
}

const PROFILES: &[InheritanceProfile] = &[
    InheritanceProfile {
        language_id: "python",
        language_fn: lang_python,
        query_src: r#"
(class_definition
  name: (identifier) @child
  superclasses: (argument_list
    (identifier) @parent))
(class_definition
  name: (identifier) @child
  superclasses: (argument_list
    (attribute attribute: (identifier) @parent)))
"#,
    },
    InheritanceProfile {
        language_id: "javascript",
        language_fn: lang_javascript,
        query_src: r#"
(class_declaration
  name: (identifier) @child
  (class_heritage
    (identifier) @parent))
(class_declaration
  name: (identifier) @child
  (class_heritage
    (member_expression
      property: (property_identifier) @parent)))
"#,
    },
    InheritanceProfile {
        language_id: "typescript",
        language_fn: lang_typescript,
        query_src: r#"
(class_declaration
  name: (type_identifier) @child
  (class_heritage
    (extends_clause
      value: (identifier) @parent)))
(class_declaration
  name: (type_identifier) @child
  (class_heritage
    (extends_clause
      value: (member_expression
        property: (property_identifier) @parent))))
(class_declaration
  name: (type_identifier) @child
  (class_heritage
    (implements_clause
      (type_identifier) @iface)))
(interface_declaration
  name: (type_identifier) @child
  (extends_type_clause
    (type_identifier) @parent))
"#,
    },
    InheritanceProfile {
        language_id: "java",
        language_fn: lang_java,
        query_src: r#"
(class_declaration
  name: (identifier) @child
  (superclass
    (type_identifier) @parent))
(class_declaration
  name: (identifier) @child
  (super_interfaces
    (type_list
      (type_identifier) @iface)))
(interface_declaration
  name: (identifier) @child
  (extends_interfaces
    (type_list
      (type_identifier) @parent)))
"#,
    },
    InheritanceProfile {
        language_id: "rust",
        language_fn: lang_rust,
        query_src: r#"
(impl_item
  trait: (type_identifier) @iface
  type: (type_identifier) @child)
(impl_item
  trait: (generic_type type: (type_identifier) @iface)
  type: (type_identifier) @child)
(impl_item
  trait: (scoped_type_identifier name: (type_identifier) @iface)
  type: (type_identifier) @child)
"#,
    },
    InheritanceProfile {
        language_id: "go",
        language_fn: lang_go,
        // Embedded types approximated as INHERITS (Go composition).
        query_src: r#"
(type_spec
  name: (type_identifier) @child
  type: (struct_type
    (field_declaration_list
      (field_declaration
        name: (_)
        type: (type_identifier) @parent))))
(type_spec
  name: (type_identifier) @child
  type: (struct_type
    (field_declaration_list
      (field_declaration
        type: (type_identifier) @parent))))
"#,
    },
    InheritanceProfile {
        language_id: "cpp",
        language_fn: lang_cpp,
        query_src: r#"
(class_specifier
  name: (type_identifier) @child
  (base_class_clause
    (base_class_specifier
      (type_identifier) @parent)))
(struct_specifier
  name: (type_identifier) @child
  (base_class_clause
    (base_class_specifier
      (type_identifier) @parent)))
"#,
    },
    InheritanceProfile {
        language_id: "ruby",
        language_fn: lang_ruby,
        query_src: r#"
(class
  name: (constant) @child
  superclass: (constant) @parent)
(class
  name: (constant) @child
  superclass: (scope_resolution name: (constant) @parent))
"#,
    },
];

/// Capture kinds produced by inheritance queries.
#[derive(Debug, Clone)]
struct RelationHit {
    child: String,
    parent: String,
    edge_type: &'static str, // INHERITS | IMPLEMENTS
}

/// AST inheritance resolver for one language profile.
pub struct AstInheritanceResolver {
    profile: &'static InheritanceProfile,
}

impl AstInheritanceResolver {
    pub fn for_language(language: &str) -> Option<Self> {
        InheritanceProfile::for_id(language).map(|profile| Self { profile })
    }

    /// Returns `None` if parse/query setup fails (caller should regex-fallback).
    /// Empty `Some(vec![])` means parse OK with no relations found.
    pub fn try_resolve(
        &self,
        file_path: &str,
        content: &str,
        local_index: &HashMap<String, String>,
        project_index: &HashMap<String, Vec<String>>,
    ) -> Option<Vec<Edge>> {
        let lang = self.profile.language();
        let mut parser = Parser::new();
        if parser.set_language(&lang).is_err() {
            return None;
        }
        let tree = parser.parse(content, None)?;
        let query = Query::new(&lang, self.profile.query_src).ok()?;
        Some(self.collect_edges(
            file_path,
            content,
            local_index,
            project_index,
            &tree,
            &query,
        ))
    }

    fn collect_edges(
        &self,
        file_path: &str,
        content: &str,
        local_index: &HashMap<String, String>,
        project_index: &HashMap<String, Vec<String>>,
        tree: &tree_sitter::Tree,
        query: &Query,
    ) -> Vec<Edge> {
        let bytes = content.as_bytes();
        let mut cursor = QueryCursor::new();
        let mut hits: Vec<RelationHit> = Vec::new();
        let mut matches = cursor.matches(query, tree.root_node(), bytes);

        while let Some(m) = matches.next() {
            let mut child = String::new();
            let mut parent = String::new();
            let mut iface = String::new();
            for cap in m.captures {
                let name = query.capture_names()[cap.index as usize];
                let text = cap.node.utf8_text(bytes).unwrap_or("").to_string();
                match name {
                    "child" => child = text,
                    "parent" => parent = text,
                    "iface" => iface = text,
                    _ => {}
                }
            }
            if child.is_empty() {
                continue;
            }
            if !parent.is_empty() {
                hits.push(RelationHit {
                    child: child.clone(),
                    parent,
                    edge_type: "INHERITS",
                });
            }
            if !iface.is_empty() {
                // Rust impl Trait for Type / Java implements → IMPLEMENTS
                hits.push(RelationHit {
                    child: child.clone(),
                    parent: iface,
                    edge_type: "IMPLEMENTS",
                });
            }
        }

        let mut edges = Vec::new();
        let mut seen: HashSet<(String, String, String)> = HashSet::new();
        for hit in hits {
            let Some(src) = resolve_local_or_project(&hit.child, local_index, project_index) else {
                continue;
            };
            let dst =
                resolve_target_name(&hit.parent, local_index, project_index, file_path);
            let key = (src.clone(), dst.clone(), hit.edge_type.to_string());
            if seen.insert(key) {
                edges.push(Edge {
                    src_qn: src,
                    dst_qn: dst,
                    edge_type: hit.edge_type.into(),
                    properties_json: Some(format!(
                        r#"{{"confidence":"high","method":"ast","language":"{}"}}"#,
                        self.profile.language_id
                    )),
                });
            }
        }
        edges
    }
}

fn resolve_local_or_project(
    name: &str,
    local: &HashMap<String, String>,
    project: &HashMap<String, Vec<String>>,
) -> Option<String> {
    let base = simple_name(name);
    if let Some(qn) = local.get(base).or_else(|| local.get(name)) {
        return Some(qn.clone());
    }
    // Unique project-wide symbol with this name
    if let Some(qns) = project.get(base) {
        if qns.len() == 1 {
            return Some(qns[0].clone());
        }
    }
    None
}

pub(crate) fn resolve_target_name(
    name: &str,
    local: &HashMap<String, String>,
    project: &HashMap<String, Vec<String>>,
    file_path: &str,
) -> String {
    let base = simple_name(name);
    if let Some(qn) = local.get(base).or_else(|| local.get(name)) {
        return qn.clone();
    }
    if let Some(qns) = project.get(base) {
        if qns.len() == 1 {
            return qns[0].clone();
        }
        // Prefer a Class-labeled qn when ambiguous
        if let Some(class_qn) = qns.iter().find(|q| q.contains("::Class::")) {
            return class_qn.clone();
        }
    }
    // Synthetic external type node (stable placeholder)
    format!("{file_path}::Class::{base}@L0")
}

fn simple_name(name: &str) -> &str {
    name.rsplit(['.', ':', '/'])
        .next()
        .unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idx(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(n, q)| ((*n).to_string(), (*q).to_string()))
            .collect()
    }

    #[test]
    fn python_ast_inherits() {
        let r = AstInheritanceResolver::for_language("python").unwrap();
        let local = idx(&[
            ("Child", "m.py::Class::Child@L1"),
            ("Parent", "m.py::Class::Parent@L10"),
        ]);
        let project = HashMap::new();
        let edges = r
            .try_resolve(
                "m.py",
                "class Child(Parent):\n    pass\n",
                &local,
                &project,
            )
            .unwrap();
        assert!(
            edges.iter().any(|e| {
                e.edge_type == "INHERITS"
                    && e.src_qn.contains("Child")
                    && e.dst_qn.contains("Parent")
                    && e.properties_json
                        .as_ref()
                        .is_some_and(|p| p.contains("ast"))
            }),
            "{edges:?}"
        );
    }

    #[test]
    fn java_ast_implements() {
        let r = AstInheritanceResolver::for_language("java").unwrap();
        let local = idx(&[
            ("Dog", "A.java::Class::Dog@L1"),
            ("Animal", "A.java::Class::Animal@L20"),
            ("Runnable", "A.java::Class::Runnable@L30"),
        ]);
        let project = HashMap::new();
        let src = "class Dog extends Animal implements Runnable {\n}\n";
        let edges = r.try_resolve("A.java", src, &local, &project).unwrap();
        assert!(edges.iter().any(|e| e.edge_type == "INHERITS"));
        assert!(edges.iter().any(|e| e.edge_type == "IMPLEMENTS"));
    }
}
