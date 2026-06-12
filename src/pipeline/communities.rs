use crate::store::{Edge, Symbol};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct CommunityResult {
    pub assignments: HashMap<String, u32>,
    pub community_count: usize,
}

struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
        }
    }

    fn find(&mut self, x: usize) -> usize {
        if self.parent[x] != x {
            self.parent[x] = self.find(self.parent[x]);
        }
        self.parent[x]
    }

    fn union(&mut self, a: usize, b: usize) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            self.parent[rb] = ra;
        }
    }
}

/// Deterministic connected-component communities on CALLS + IMPORTS edges.
pub fn detect_communities(symbols: &[Symbol], edges: &[Edge]) -> CommunityResult {
    let nodes: Vec<String> = symbols
        .iter()
        .filter(|s| {
            !matches!(
                s.label.as_str(),
                "Project" | "Folder" | "File" | "Module" | "Route"
            )
        })
        .map(|s| s.qualified_name.clone())
        .collect();
    if nodes.is_empty() {
        return CommunityResult {
            assignments: HashMap::new(),
            community_count: 0,
        };
    }

    let index: HashMap<String, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.clone(), i))
        .collect();
    let mut uf = UnionFind::new(nodes.len());
    for edge in edges {
        if edge.edge_type != "CALLS" && edge.edge_type != "IMPORTS" {
            continue;
        }
        if let (Some(&a), Some(&b)) = (index.get(&edge.src_qn), index.get(&edge.dst_qn)) {
            uf.union(a, b);
        }
    }

    let mut root_to_id: HashMap<usize, u32> = HashMap::new();
    let mut assignments: HashMap<String, u32> = HashMap::new();
    for (i, node) in nodes.iter().enumerate() {
        let root = uf.find(i);
        let id = if let Some(&id) = root_to_id.get(&root) {
            id
        } else {
            let id = root_to_id.len() as u32;
            root_to_id.insert(root, id);
            id
        };
        assignments.insert(node.clone(), id);
    }

    CommunityResult {
        community_count: root_to_id.len(),
        assignments,
    }
}

pub fn apply_community_properties(symbols: &mut [Symbol], result: &CommunityResult) {
    for sym in symbols.iter_mut() {
        let Some(id) = result.assignments.get(&sym.qualified_name) else {
            continue;
        };
        let mut props = sym
            .properties_json
            .as_ref()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        if let Some(obj) = props.as_object_mut() {
            obj.insert("community_id".into(), serde_json::json!(id));
        }
        sym.properties_json = Some(props.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(qn: &str, label: &str) -> Symbol {
        Symbol {
            qualified_name: qn.into(),
            name: qn.split("::").nth(2).unwrap_or("x").into(),
            label: label.into(),
            file_path: "a.rs".into(),
            line_start: 1,
            line_end: 2,
            signature: None,
            properties_json: None,
        }
    }

    #[test]
    fn connected_pair_shares_community() {
        let symbols = vec![
            sym("a.rs::Function::foo@L1", "Function"),
            sym("a.rs::Function::bar@L5", "Function"),
        ];
        let edges = vec![Edge {
            src_qn: symbols[0].qualified_name.clone(),
            dst_qn: symbols[1].qualified_name.clone(),
            edge_type: "CALLS".into(),
            properties_json: None,
        }];
        let result = detect_communities(&symbols, &edges);
        assert_eq!(result.community_count, 1);
        assert_eq!(
            result.assignments.get(&symbols[0].qualified_name),
            result.assignments.get(&symbols[1].qualified_name)
        );
    }
}
