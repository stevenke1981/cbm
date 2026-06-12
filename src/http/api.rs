use crate::error::Result;
use crate::project::normalize_project_name;
use crate::store::{Store, Symbol};
use serde::Serialize;
use std::collections::HashMap;
use std::f32::consts::PI;

#[derive(Debug, Clone, Serialize)]
pub struct GraphNode {
    pub id: String,
    pub name: String,
    pub label: String,
    pub file_path: String,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub color: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub edge_type: String,
    pub color: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphPayload {
    pub project: String,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub symbol_count: usize,
    pub edge_count: usize,
}

pub fn build_graph_payload(project: &str, limit: usize) -> Result<GraphPayload> {
    let project = normalize_project_name(project);
    let store = Store::open(&project)?;
    build_graph_from_store(&store, &project, limit)
}

pub fn build_graph_from_store(store: &Store, project: &str, limit: usize) -> Result<GraphPayload> {
    let symbols = store.list_symbols()?;
    let edges = store.list_edges_limited(limit.saturating_mul(4))?;

    let nodes: Vec<Symbol> = symbols.into_iter().take(limit).collect();
    let node_ids: HashMap<String, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, s)| (s.qualified_name.clone(), i))
        .collect();

    let graph_nodes: Vec<GraphNode> = nodes
        .iter()
        .enumerate()
        .map(|(i, sym)| {
            let (x, y, z) = fibonacci_sphere(i, nodes.len());
            GraphNode {
                id: sym.qualified_name.clone(),
                name: sym.name.clone(),
                label: sym.label.clone(),
                file_path: sym.file_path.clone(),
                x,
                y,
                z,
                color: color_for_label(&sym.label),
            }
        })
        .collect();

    let graph_edges: Vec<GraphEdge> = edges
        .into_iter()
        .filter(|e| node_ids.contains_key(&e.src_qn) && node_ids.contains_key(&e.dst_qn))
        .take(limit.saturating_mul(2))
        .map(|e| {
            let edge_type = e.edge_type.clone();
            GraphEdge {
                source: e.src_qn,
                target: e.dst_qn,
                color: color_for_edge(&edge_type),
                edge_type,
            }
        })
        .collect();

    Ok(GraphPayload {
        project: project.to_string(),
        symbol_count: graph_nodes.len(),
        edge_count: graph_edges.len(),
        nodes: graph_nodes,
        edges: graph_edges,
    })
}

fn fibonacci_sphere(index: usize, total: usize) -> (f32, f32, f32) {
    if total <= 1 {
        return (0.0, 0.0, 0.0);
    }
    let i = index as f32;
    let n = total as f32;
    let phi = (1.0 + 5.0_f32.sqrt()) / 2.0;
    let theta = 2.0 * PI * i / phi;
    let y = 1.0 - (2.0 * i + 1.0) / n;
    let r = (1.0 - y * y).sqrt();
    let x = r * theta.cos();
    let z = r * theta.sin();
    (x * 8.0, y * 8.0, z * 8.0)
}

pub fn color_for_label(label: &str) -> String {
    match label {
        "Function" => "#4fc3f7".into(),
        "Class" => "#81c784".into(),
        "Module" => "#ffb74d".into(),
        "Interface" => "#ba68c8".into(),
        "File" => "#90a4ae".into(),
        "Folder" => "#78909c".into(),
        "Project" => "#ffd54f".into(),
        _ => "#e0e0e0".into(),
    }
}

pub fn color_for_edge(edge_type: &str) -> u32 {
    match edge_type {
        "CALLS" => 0x58a6ff,
        "IMPORTS" => 0xffb74d,
        "CONTAINS" => 0x484f58,
        "INHERITS" => 0x81c784,
        "IMPLEMENTS" => 0xba68c8,
        "DECORATES" => 0xf778ba,
        "HTTP_ROUTE" => 0xff8a65,
        "SIMILAR_TO" | "SEMANTICALLY_RELATED" => 0xbc8cff,
        _ => 0x484f58,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct NodeDetailPayload {
    pub symbol: Symbol,
    pub inbound: Vec<GraphEdge>,
    pub outbound: Vec<GraphEdge>,
}

pub fn search_symbols(project: &str, query: &str, limit: usize) -> Result<Vec<Symbol>> {
    let project = normalize_project_name(project);
    let store = Store::open(&project)?;
    let result = store.search(&crate::store::SearchFilter {
        query: Some(query.into()),
        limit,
        ..Default::default()
    })?;
    Ok(result.symbols)
}

pub fn node_detail(project: &str, qn: &str) -> Result<Option<NodeDetailPayload>> {
    let project = normalize_project_name(project);
    let store = Store::open(&project)?;
    let Some(symbol) = store.find_symbol(qn)? else {
        return Ok(None);
    };
    let edges = store.list_edges()?;
    let mut inbound = Vec::new();
    let mut outbound = Vec::new();
    for e in edges {
        let color = color_for_edge(&e.edge_type);
        if e.dst_qn == qn {
            inbound.push(GraphEdge {
                source: e.src_qn.clone(),
                target: e.dst_qn.clone(),
                edge_type: e.edge_type.clone(),
                color,
            });
        }
        if e.src_qn == qn {
            outbound.push(GraphEdge {
                source: e.src_qn,
                target: e.dst_qn,
                edge_type: e.edge_type,
                color,
            });
        }
    }
    Ok(Some(NodeDetailPayload {
        symbol,
        inbound,
        outbound,
    }))
}
