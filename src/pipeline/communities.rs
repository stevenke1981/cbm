//! Community detection on the code graph.
//!
//! Default: **Louvain** modularity (single-level local moves, deterministic).
//! Fallback / override: connected components via `CBM_COMMUNITY_ALGO=components`.
//!
//! Env:
//! - `CBM_COMMUNITY_ALGO=louvain|components` (default `louvain`)
//! - `CBM_COMMUNITY_RESOLUTION=1.0` modularity resolution

use crate::store::{Edge, Symbol};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct CommunityResult {
    pub assignments: HashMap<String, u32>,
    pub community_count: usize,
    pub algorithm: &'static str,
}

/// Public entry — selects algorithm from env.
pub fn detect_communities(symbols: &[Symbol], edges: &[Edge]) -> CommunityResult {
    let algo = std::env::var("CBM_COMMUNITY_ALGO")
        .unwrap_or_else(|_| "louvain".into())
        .to_lowercase();
    match algo.as_str() {
        "components" | "cc" | "connected" => detect_connected_components(symbols, edges),
        _ => {
            let r = detect_louvain(symbols, edges);
            if r.community_count == 0 {
                detect_connected_components(symbols, edges)
            } else {
                r
            }
        }
    }
}

// ── Connected components ────────────────────────────────────────────────────

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

fn code_nodes(symbols: &[Symbol]) -> Vec<String> {
    symbols
        .iter()
        .filter(|s| {
            !matches!(
                s.label.as_str(),
                "Project" | "Folder" | "File" | "Module" | "Route" | "Decorator"
            )
        })
        .map(|s| s.qualified_name.clone())
        .collect()
}

fn is_community_edge(edge_type: &str) -> bool {
    matches!(
        edge_type,
        "CALLS" | "IMPORTS" | "INHERITS" | "IMPLEMENTS" | "HTTP_CALLS"
    )
}

fn edge_weight(edge_type: &str) -> f64 {
    match edge_type {
        "CALLS" => 1.0,
        "IMPORTS" => 0.7,
        "INHERITS" | "IMPLEMENTS" => 0.9,
        "HTTP_CALLS" => 0.5,
        _ => 0.3,
    }
}

fn detect_connected_components(symbols: &[Symbol], edges: &[Edge]) -> CommunityResult {
    let nodes = code_nodes(symbols);
    if nodes.is_empty() {
        return CommunityResult {
            assignments: HashMap::new(),
            community_count: 0,
            algorithm: "components",
        };
    }

    let index: HashMap<String, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.clone(), i))
        .collect();
    let mut uf = UnionFind::new(nodes.len());
    for edge in edges {
        if !is_community_edge(&edge.edge_type) {
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
        algorithm: "components",
    }
}

// ── Louvain (single-level local modularity moves) ───────────────────────────

fn detect_louvain(symbols: &[Symbol], edges: &[Edge]) -> CommunityResult {
    let nodes = code_nodes(symbols);
    let n = nodes.len();
    if n == 0 {
        return CommunityResult {
            assignments: HashMap::new(),
            community_count: 0,
            algorithm: "louvain",
        };
    }

    let index: HashMap<String, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.clone(), i))
        .collect();

    let mut adj: Vec<HashMap<usize, f64>> = vec![HashMap::new(); n];
    let mut m = 0.0_f64; // total undirected weight
    for edge in edges {
        if !is_community_edge(&edge.edge_type) {
            continue;
        }
        let (Some(&a), Some(&b)) = (index.get(&edge.src_qn), index.get(&edge.dst_qn)) else {
            continue;
        };
        if a == b {
            continue;
        }
        let w = edge_weight(&edge.edge_type);
        *adj[a].entry(b).or_default() += w;
        *adj[b].entry(a).or_default() += w;
        // Count each undirected edge once toward total weight m.
        m += w;
    }

    if m <= 0.0 {
        return detect_connected_components(symbols, edges);
    }

    let resolution = std::env::var("CBM_COMMUNITY_RESOLUTION")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(1.0)
        .clamp(0.01, 10.0);

    let degree: Vec<f64> = adj.iter().map(|nbr| nbr.values().sum()).collect();
    let mut community: Vec<usize> = (0..n).collect();
    let mut sigma_tot: Vec<f64> = degree.clone();

    let mut changed = true;
    let mut passes = 0usize;
    while changed && passes < 20 {
        changed = false;
        passes += 1;
        for i in 0..n {
            let ci = community[i];
            let ki = degree[i];

            // weight from i into each neighbor community
            let mut to_comm: HashMap<usize, f64> = HashMap::new();
            for (&j, &w) in &adj[i] {
                *to_comm.entry(community[j]).or_default() += w;
            }
            let ki_in_ci = *to_comm.get(&ci).unwrap_or(&0.0);

            // remove i from ci
            sigma_tot[ci] -= ki;

            let mut best = ci;
            let mut best_delta = f64::NEG_INFINITY;

            // evaluate neighbor communities + original
            let mut candidates: Vec<usize> = to_comm.keys().copied().collect();
            if !candidates.contains(&ci) {
                candidates.push(ci);
            }
            candidates.sort_unstable();

            for c in candidates {
                let ki_in = if c == ci {
                    ki_in_ci
                } else {
                    *to_comm.get(&c).unwrap_or(&0.0)
                };
                let sigma = sigma_tot[c];
                // ΔQ for inserting i into community c (Blondel et al. 2008)
                let delta = (ki_in / (2.0 * m)) - resolution * (sigma * ki) / (4.0 * m * m);
                if delta > best_delta + 1e-12 || ((delta - best_delta).abs() <= 1e-12 && c < best) {
                    best_delta = delta;
                    best = c;
                }
            }

            sigma_tot[best] += ki;
            if best != ci {
                community[i] = best;
                changed = true;
            }
        }
    }

    // Dense renumber in first-seen order
    let mut renumber: HashMap<usize, u32> = HashMap::new();
    let mut assignments = HashMap::new();
    for (i, node) in nodes.iter().enumerate() {
        let c = community[i];
        let next_id = renumber.len() as u32;
        let id = *renumber.entry(c).or_insert(next_id);
        assignments.insert(node.clone(), id);
    }

    CommunityResult {
        community_count: renumber.len(),
        assignments,
        algorithm: "louvain",
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
            obj.insert("community_algo".into(), serde_json::json!(result.algorithm));
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
        let result = detect_connected_components(&symbols, &edges);
        assert_eq!(result.community_count, 1);
        assert_eq!(
            result.assignments.get(&symbols[0].qualified_name),
            result.assignments.get(&symbols[1].qualified_name)
        );
    }

    #[test]
    fn louvain_groups_triangle() {
        let symbols = vec![
            sym("a.rs::Function::a@L1", "Function"),
            sym("a.rs::Function::b@L2", "Function"),
            sym("a.rs::Function::c@L3", "Function"),
            sym("a.rs::Function::d@L4", "Function"),
        ];
        let a = symbols[0].qualified_name.clone();
        let b = symbols[1].qualified_name.clone();
        let c = symbols[2].qualified_name.clone();
        let edges = vec![
            Edge {
                src_qn: a.clone(),
                dst_qn: b.clone(),
                edge_type: "CALLS".into(),
                properties_json: None,
            },
            Edge {
                src_qn: b.clone(),
                dst_qn: c.clone(),
                edge_type: "CALLS".into(),
                properties_json: None,
            },
            Edge {
                src_qn: c.clone(),
                dst_qn: a.clone(),
                edge_type: "CALLS".into(),
                properties_json: None,
            },
        ];
        let result = detect_louvain(&symbols, &edges);
        assert_eq!(result.algorithm, "louvain");
        assert_eq!(result.assignments.get(&a), result.assignments.get(&b));
        assert_eq!(result.assignments.get(&b), result.assignments.get(&c));
    }
}
