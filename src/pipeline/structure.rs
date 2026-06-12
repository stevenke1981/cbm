use crate::project::normalize_project_name;
use crate::store::{Edge, Symbol};
use std::collections::HashSet;
use std::path::{Component, Path};

pub fn build_structure_graph(
    project: &str,
    repo_path: &str,
    file_paths: &[String],
    symbol_qns: &[String],
) -> (Vec<Symbol>, Vec<Edge>) {
    let project_name = normalize_project_name(project);
    let mut symbols = Vec::new();
    let mut edges = Vec::new();
    let mut seen_folders = HashSet::new();

    let project_qn = format!("{project_name}::Project::{project_name}");
    symbols.push(Symbol {
        qualified_name: project_qn.clone(),
        name: project_name.clone(),
        label: "Project".into(),
        file_path: repo_path.into(),
        line_start: 1,
        line_end: 1,
        signature: None,
        properties_json: None,
    });

    for rel in file_paths {
        let file_qn = format!("{rel}::File::{rel}");
        symbols.push(Symbol {
            qualified_name: file_qn.clone(),
            name: rel.clone(),
            label: "File".into(),
            file_path: rel.clone(),
            line_start: 1,
            line_end: 1,
            signature: None,
            properties_json: None,
        });
        edges.push(Edge {
            src_qn: project_qn.clone(),
            dst_qn: file_qn.clone(),
            edge_type: "CONTAINS".into(),
            properties_json: None,
        });

        let mut folder = String::new();
        for comp in Path::new(rel).components() {
            if let Component::Normal(part) = comp {
                folder = if folder.is_empty() {
                    part.to_string_lossy().into_owned()
                } else {
                    format!("{folder}/{}", part.to_string_lossy())
                };
                if seen_folders.insert(folder.clone()) {
                    let folder_qn = format!("{folder}::Folder::{folder}");
                    symbols.push(Symbol {
                        qualified_name: folder_qn.clone(),
                        name: folder.clone(),
                        label: "Folder".into(),
                        file_path: folder.clone(),
                        line_start: 1,
                        line_end: 1,
                        signature: None,
                        properties_json: None,
                    });
                    edges.push(Edge {
                        src_qn: project_qn.clone(),
                        dst_qn: folder_qn,
                        edge_type: "CONTAINS".into(),
                        properties_json: None,
                    });
                }
            }
        }

        for sym_qn in symbol_qns
            .iter()
            .filter(|q| q.starts_with(&format!("{rel}::")))
        {
            edges.push(Edge {
                src_qn: file_qn.clone(),
                dst_qn: sym_qn.clone(),
                edge_type: "CONTAINS".into(),
                properties_json: None,
            });
        }
    }

    (symbols, edges)
}
