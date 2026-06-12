use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    pub qualified_name: String,
    pub name: String,
    pub label: String,
    pub file_path: String,
    pub line_start: i64,
    pub line_end: i64,
    pub signature: Option<String>,
    pub properties_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub src_qn: String,
    pub dst_qn: String,
    pub edge_type: String,
    pub properties_json: Option<String>,
}

pub type VectorEntry = (String, Vec<i8>, String, String, String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceFile {
    pub path: String,
    pub content: String,
    pub language: String,
    pub line_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchFilter {
    pub query: Option<String>,
    pub label: Option<String>,
    pub name_pattern: Option<String>,
    pub qn_pattern: Option<String>,
    pub file_pattern: Option<String>,
    /// Edge type filter, e.g. `CALLS`, `IMPORTS`, `CONTAINS`.
    pub relationship: Option<String>,
    /// `inbound`, `outbound`, or `any` (default).
    pub direction: Option<String>,
    pub min_degree: Option<usize>,
    pub max_degree: Option<usize>,
    /// Include 1-hop neighbors connected via `relationship` (defaults to CALLS).
    #[serde(default)]
    pub include_connected: bool,
    /// Exclude symbols with zero inbound edges for `relationship` (defaults to CALLS).
    #[serde(default)]
    pub exclude_entry_points: bool,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
}

fn default_limit() -> usize {
    200
}

impl Default for SearchFilter {
    fn default() -> Self {
        Self {
            query: None,
            label: None,
            name_pattern: None,
            qn_pattern: None,
            file_pattern: None,
            relationship: None,
            direction: None,
            min_degree: None,
            max_degree: None,
            include_connected: false,
            exclude_entry_points: false,
            limit: default_limit(),
            offset: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub symbols: Vec<Symbol>,
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorMatch {
    pub qualified_name: String,
    pub name: String,
    pub label: String,
    pub file_path: String,
    pub score: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_breakdown: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorSearchResult {
    pub matches: Vec<VectorMatch>,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceResult {
    pub start: String,
    pub direction: String,
    pub depth: usize,
    pub nodes: Vec<Symbol>,
    pub edges: Vec<Edge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeSnippet {
    pub symbol: Symbol,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeMatch {
    pub path: String,
    pub language: String,
    pub line_number: usize,
    pub preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<std::collections::HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub name: String,
    pub repo_path: String,
    pub indexed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelCount {
    pub label: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeTypeCount {
    pub edge_type: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommunitySummary {
    pub id: u32,
    pub symbol_count: usize,
    pub sample_symbols: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchitectureSummary {
    pub project: String,
    pub symbol_count: usize,
    pub edge_count: usize,
    pub file_count: usize,
    pub labels: Vec<LabelCount>,
    pub edge_types: Vec<EdgeTypeCount>,
    pub top_functions: Vec<Symbol>,
    pub community_count: usize,
    pub top_communities: Vec<CommunitySummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexStatus {
    pub project: String,
    pub indexed: bool,
    pub repo_path: Option<String>,
    pub indexed_at: Option<String>,
    pub mode: String,
    pub symbol_count: usize,
    pub edge_count: usize,
    pub file_count: usize,
    pub vector_count: usize,
    pub semantic_enabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GraphSchema {
    pub node_labels: Vec<String>,
    pub edge_types: Vec<String>,
    /// Edge types with at least one indexed instance in this project.
    pub implemented_edge_types: Vec<String>,
    pub tables: Vec<String>,
}

impl GraphSchema {
    pub fn standard() -> Self {
        Self {
            node_labels: vec![
                "Function".into(),
                "Class".into(),
                "Module".into(),
                "File".into(),
                "Folder".into(),
                "Project".into(),
                "Package".into(),
                "Interface".into(),
            ],
            edge_types: vec![
                "CALLS".into(),
                "IMPORTS".into(),
                "CONTAINS".into(),
                "INHERITS".into(),
                "IMPLEMENTS".into(),
                "DECORATES".into(),
                "SIMILAR_TO".into(),
                "SEMANTICALLY_RELATED".into(),
                "RUNTIME_TRACE".into(),
                "HTTP_CALLS".into(),
                "HTTP_ROUTE".into(),
            ],
            implemented_edge_types: Vec::new(),
            tables: vec![
                "symbols".into(),
                "edges".into(),
                "files".into(),
                "vectors".into(),
                "projects".into(),
                "meta".into(),
            ],
        }
    }
}
