mod support;

use cbm::discover::IndexMode;
use cbm::mcp::McpServer;
use cbm::pipeline::Pipeline;
use cbm::project::normalize_project_name;
use cbm::store::{SearchFilter, Store};
use serde_json::json;
use std::fs;
use std::sync::Arc;
use support::isolated_cache;
use tempfile::TempDir;

fn fixture_repo() -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("lib.rs"),
        r#"
pub fn greet(name: &str) -> String {
    let msg = format_message(name);
    msg
}

fn format_message(name: &str) -> String {
    format!("Hello, {name}!")
}

pub struct Greeter {
    prefix: String,
}

impl Greeter {
    pub fn new(prefix: String) -> Self {
        Self { prefix }
    }
    pub fn greet(&self, name: &str) -> String {
        format!("{} {}", self.prefix, name)
    }
}
"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("helper.py"),
        r#"
def process(data):
    return transform(data)

def transform(data):
    return data.upper()
"#,
    )
    .unwrap();
    dir
}

#[test]
fn indexes_fixture_repository() {
    let (_guard, _cache, _) = isolated_cache();
    let dir = fixture_repo();
    let project = "cbm+test-fixture";
    let pipeline = Pipeline::new(IndexMode::Full);
    let result = pipeline
        .run(dir.path(), Some("test-fixture"))
        .expect("index should succeed");

    assert!(result.success);
    assert!(result.files_indexed >= 2);
    assert!(result.symbols_extracted >= 4);
    assert_eq!(result.project, project);

    let store = Store::open(project).unwrap();
    let arch = store.get_architecture().unwrap();
    assert!(arch.symbol_count >= 4);
    assert!(arch.file_count >= 2);

    let _ = cbm::store::delete_project_db(project);
}

#[test]
fn search_and_trace_call_graph() {
    let (_guard, _cache, _) = isolated_cache();
    let dir = fixture_repo();
    let pipeline = Pipeline::new(IndexMode::Full);
    let index = pipeline.run(dir.path(), Some("trace-test")).unwrap();
    assert!(index.symbols_extracted > 0, "expected symbols");

    let store = Store::open(&index.project).unwrap();
    let count = store.count_symbols().unwrap();
    assert!(
        count > 0,
        "db should contain {} symbols, got count={count}",
        index.symbols_extracted
    );

    let all = store.search(&SearchFilter::default()).unwrap();
    assert!(
        !all.symbols.is_empty(),
        "search returned empty (total={}, db_count={count})",
        all.total
    );

    let search = store
        .search(&SearchFilter {
            query: Some("greet".into()),
            ..Default::default()
        })
        .unwrap();
    assert!(
        !search.symbols.is_empty(),
        "expected greet match, got total={}",
        search.total
    );

    let start = search
        .symbols
        .iter()
        .find(|s| s.name == "greet")
        .map(|s| s.qualified_name.clone())
        .expect("greet symbol");

    let trace = store.trace_path(&start, "outbound", 2).unwrap();
    assert!(!trace.nodes.is_empty());

    let snippet = store.get_snippet(&start).unwrap();
    assert!(snippet.snippet.contains("greet"));

    let _ = cbm::store::delete_project_db(&index.project);
}

#[test]
fn mcp_tools_call_index_and_search() {
    let (_guard, _cache, _) = isolated_cache();
    std::env::set_var("CBM_WATCHER", "0");
    let dir = fixture_repo();
    let server = McpServer::new();

    let index_req = json!({
        "jsonrpc": "2.0",
        "id": 10,
        "method": "tools/call",
        "params": {
            "name": "index_repository",
            "arguments": {
                "repo_path": dir.path().to_string_lossy(),
                "project": "mcp-test"
            }
        }
    });
    let resp = server
        .handle_message(&index_req.to_string())
        .unwrap()
        .unwrap();
    assert!(resp.contains("success"));

    let search_req = json!({
        "jsonrpc": "2.0",
        "id": 11,
        "method": "tools/call",
        "params": {
            "name": "search_graph",
            "arguments": {
                "project": "mcp-test",
                "query": "format"
            }
        }
    });
    let resp = server
        .handle_message(&search_req.to_string())
        .unwrap()
        .unwrap();
    assert!(resp.contains("symbols"));

    let rlm_req = json!({
        "jsonrpc": "2.0",
        "id": 12,
        "method": "tools/call",
        "params": {
            "name": "rlm_workflow",
            "arguments": { "phase": "filter" }
        }
    });
    let resp = server
        .handle_message(&rlm_req.to_string())
        .unwrap()
        .unwrap();
    assert!(resp.contains("filter"));

    let _ = cbm::store::delete_project_db(&normalize_project_name("mcp-test"));
}

#[test]
fn rlm_scan_and_chunk() {
    let (_guard, _cache, _) = isolated_cache();
    std::env::set_var("CBM_WATCHER", "0");
    let dir = fixture_repo();
    let server = McpServer::new();

    let scan_req = json!({
        "jsonrpc": "2.0",
        "id": 20,
        "method": "tools/call",
        "params": {
            "name": "rlm_scan",
            "arguments": { "path": dir.path().to_string_lossy() }
        }
    });
    let resp = server
        .handle_message(&scan_req.to_string())
        .unwrap()
        .unwrap();
    assert!(resp.contains("session_id"));
}

#[test]
fn cli_rlm_scan_chunk_persists_across_invocations() {
    let (_guard, _cache, _) = isolated_cache();
    let dir = fixture_repo();
    let path = dir.path().to_string_lossy().to_string();

    let rlm1 = Arc::new(cbm::rlm::RlmEngine::new());
    let handler1 = cbm::mcp::ToolHandler::new(rlm1, None);
    let scan = handler1
        .handle("rlm_scan", &json!({ "path": path }))
        .expect("rlm_scan");
    let session_id = scan
        .get("session_id")
        .and_then(|v| v.as_str())
        .expect("session_id");

    let rlm2 = Arc::new(cbm::rlm::RlmEngine::new());
    let handler2 = cbm::mcp::ToolHandler::new(rlm2, None);
    let chunk = handler2
        .handle(
            "rlm_chunk",
            &json!({ "session_id": session_id, "offset": 0, "limit": 1 }),
        )
        .expect("rlm_chunk should load persisted session");
    assert!(chunk.get("chunks").and_then(|v| v.as_array()).is_some());
}

#[test]
fn ingest_runtime_traces() {
    let (_guard, _cache, _) = isolated_cache();
    let dir = fixture_repo();
    let pipeline = Pipeline::new(IndexMode::Full);
    let index = pipeline.run(dir.path(), Some("trace-ingest")).unwrap();

    let store = Store::open(&index.project).unwrap();
    let greet = store
        .search(&SearchFilter {
            query: Some("greet".into()),
            label: Some("Function".into()),
            ..Default::default()
        })
        .unwrap()
        .symbols
        .into_iter()
        .find(|s| s.name == "greet" && s.file_path == "lib.rs")
        .expect("greet symbol");
    let format_msg = store
        .search(&SearchFilter {
            query: Some("format_message".into()),
            ..Default::default()
        })
        .unwrap()
        .symbols
        .into_iter()
        .find(|s| s.name == "format_message")
        .expect("format_message symbol");

    let ingested = store
        .ingest_traces(&[(
            greet.qualified_name.clone(),
            format_msg.qualified_name.clone(),
        )])
        .unwrap();
    assert_eq!(ingested, 1);

    assert!(store.count_edges_by_type("RUNTIME_TRACE").unwrap() >= 1);

    let _ = cbm::store::delete_project_db(&index.project);
}

#[test]
fn query_graph_select_only() {
    let store = Store::open_memory().unwrap();
    let result = store
        .query_select("SELECT 1 AS one")
        .expect("select should work");
    assert_eq!(result.rows.len(), 1);

    let blocked = store.query_select("DELETE FROM symbols");
    assert!(blocked.is_err());

    let allowed = store.query_select("SELECT 'UPDATE' AS word");
    assert!(allowed.is_ok());
}

#[test]
fn emits_contains_imports_and_calls_edges() {
    let (_guard, _cache, _) = isolated_cache();
    let dir = fixture_repo();
    let pipeline = Pipeline::new(IndexMode::Full);
    let index = pipeline.run(dir.path(), Some("edge-types")).unwrap();
    assert!(index.edges_extracted > 0);

    let store = Store::open(&index.project).unwrap();
    assert!(store.count_edges_by_type("CONTAINS").unwrap() > 0);
    assert!(store.count_edges_by_type("CALLS").unwrap() > 0);

    let _ = cbm::store::delete_project_db(&index.project);
}

#[test]
fn search_graph_regex_name_pattern() {
    let (_guard, _cache, _) = isolated_cache();
    let dir = fixture_repo();
    let pipeline = Pipeline::new(IndexMode::Full);
    let index = pipeline.run(dir.path(), Some("regex-search")).unwrap();

    let store = Store::open(&index.project).unwrap();
    let hits = store
        .search(&SearchFilter {
            name_pattern: Some(".*greet.*".into()),
            ..Default::default()
        })
        .unwrap();
    assert!(
        !hits.symbols.is_empty(),
        "regex name_pattern should match greet"
    );

    let _ = cbm::store::delete_project_db(&index.project);
}

#[test]
fn search_graph_relationship_and_degree_filters() {
    let (_guard, _cache, _) = isolated_cache();
    let dir = fixture_repo();
    let pipeline = Pipeline::new(IndexMode::Full);
    let index = pipeline.run(dir.path(), Some("graph-filter")).unwrap();
    let store = Store::open(&index.project).unwrap();

    let calls_only = store
        .search(&SearchFilter {
            relationship: Some("CALLS".into()),
            label: Some("Function".into()),
            limit: 50,
            ..Default::default()
        })
        .unwrap();
    assert!(
        !calls_only.symbols.is_empty(),
        "expected CALLS participants"
    );
    assert!(calls_only.symbols.iter().all(|s| s.label == "Function"));

    let contains_only = store
        .search(&SearchFilter {
            relationship: Some("CONTAINS".into()),
            limit: 50,
            ..Default::default()
        })
        .unwrap();
    assert!(!contains_only.symbols.is_empty());

    let schema = store.get_schema();
    assert!(schema.implemented_edge_types.contains(&"CALLS".to_string()));
    assert!(schema
        .implemented_edge_types
        .contains(&"CONTAINS".to_string()));

    let _ = cbm::store::delete_project_db(&index.project);
}

#[test]
fn search_graph_has_more_pagination() {
    let (_guard, _cache, _) = isolated_cache();
    let dir = fixture_repo();
    let pipeline = Pipeline::new(IndexMode::Full);
    let index = pipeline.run(dir.path(), Some("pagination")).unwrap();
    let store = Store::open(&index.project).unwrap();

    let page = store
        .search(&SearchFilter {
            limit: 2,
            offset: 0,
            ..Default::default()
        })
        .unwrap();
    if page.total > 2 {
        assert!(page.has_more);
        assert_eq!(page.symbols.len(), 2);
    }

    let _ = cbm::store::delete_project_db(&index.project);
}

#[test]
fn semantic_pass_emits_edges_with_signal_breakdown() {
    let (_guard, _cache, _) = isolated_cache();
    std::env::set_var("CBM_SEMANTIC_ENABLED", "1");

    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("users.rs"),
        "pub fn fetch_user(id: u64) {}\npub fn fetch_user_profile(id: u64) {}\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("ui.rs"), "pub fn render_page() {}\n").unwrap();

    let pipeline = Pipeline::new(IndexMode::Full);
    let index = pipeline.run(dir.path(), Some("semantic-edges")).unwrap();
    assert!(index.vectors_stored >= 2);
    assert!(index.semantic_edges > 0, "expected semantic edges");

    let store = Store::open(&index.project).unwrap();
    let similar = store.count_edges_by_type("SIMILAR_TO").unwrap();
    let related = store.count_edges_by_type("SEMANTICALLY_RELATED").unwrap();
    assert!(similar + related > 0);

    let edges = store.list_edges_limited(20).unwrap();
    assert!(edges
        .iter()
        .any(|e| { e.edge_type == "SIMILAR_TO" || e.edge_type == "SEMANTICALLY_RELATED" }));
    assert!(edges.iter().any(|e| {
        e.properties_json
            .as_ref()
            .is_some_and(|p| p.contains("signals"))
    }));

    std::env::remove_var("CBM_SEMANTIC_ENABLED");
    let _ = cbm::store::delete_project_db(&index.project);
}

#[test]
fn emits_inherits_edge_for_python_class() {
    let (_guard, _cache, _) = isolated_cache();
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("models.py"),
        "class Base:\n    pass\n\nclass Child(Base):\n    pass\n",
    )
    .unwrap();

    let pipeline = Pipeline::new(IndexMode::Full);
    let index = pipeline.run(dir.path(), Some("inherits")).unwrap();
    let store = Store::open(&index.project).unwrap();
    assert!(store.count_edges_by_type("INHERITS").unwrap() >= 1);

    let _ = cbm::store::delete_project_db(&index.project);
}

#[test]
fn emits_http_route_for_python_handler() {
    let (_guard, _cache, _) = isolated_cache();
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("api.py"),
        "@app.get(\"/users\")\ndef list_users():\n    return []\n",
    )
    .unwrap();

    let pipeline = Pipeline::new(IndexMode::Full);
    let index = pipeline.run(dir.path(), Some("http-routes")).unwrap();
    let store = Store::open(&index.project).unwrap();
    assert!(store.count_edges_by_type("HTTP_ROUTE").unwrap() >= 1);

    let _ = cbm::store::delete_project_db(&index.project);
}

#[test]
fn architecture_reports_communities() {
    let (_guard, _cache, _) = isolated_cache();
    let dir = fixture_repo();
    let pipeline = Pipeline::new(IndexMode::Full);
    let index = pipeline.run(dir.path(), Some("communities")).unwrap();
    let store = Store::open(&index.project).unwrap();
    let arch = store.get_architecture().unwrap();
    assert!(arch.community_count >= 1);

    let _ = cbm::store::delete_project_db(&index.project);
}

#[test]
fn store_readonly_and_integrity_check() {
    let (_guard, _cache, _) = isolated_cache();
    let dir = fixture_repo();
    let pipeline = Pipeline::new(IndexMode::Fast);
    let index = pipeline.run(dir.path(), Some("readonly-store")).unwrap();

    let ro = Store::open_readonly(&index.project).unwrap();
    assert!(ro.integrity_check().unwrap().eq_ignore_ascii_case("ok"));
    assert!(ro.count_symbols().unwrap_or(0) > 0);

    let _ = cbm::store::delete_project_db(&index.project);
}
