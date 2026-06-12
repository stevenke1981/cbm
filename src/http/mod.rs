mod api;

pub use api::{build_graph_payload, GraphPayload};

use axum::{
    extract::Query,
    response::{Html, IntoResponse, Json},
    routing::get,
    Router,
};
use serde::Deserialize;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use tower_http::cors::CorsLayer;
use tracing::info;

const INDEX_HTML: &str = include_str!("ui.html");

#[derive(Debug, Clone)]
pub struct UiConfig {
    pub enabled: bool,
    pub port: u16,
}

impl UiConfig {
    pub fn from_env_and_args(ui_flag: bool, port: u16) -> Self {
        let enabled = ui_flag
            || matches!(
                std::env::var("CBRLM_UI").as_deref(),
                Ok("1") | Ok("true") | Ok("yes") | Ok("on")
            );
        let port = std::env::var("CBRLM_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(port);
        Self { enabled, port }
    }
}

pub struct HttpServer {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl HttpServer {
    pub fn spawn(
        config: &UiConfig,
        shutdown: Option<Arc<crate::runtime::Shutdown>>,
    ) -> Option<Self> {
        if !config.enabled {
            return None;
        }

        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = stop.clone();
        let port = config.port;

        let handle = thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .worker_threads(2)
                .thread_name("cbrlm-http")
                .build()
                .expect("tokio runtime");

            rt.block_on(async move {
                if let Err(e) = run_server(port, stop_flag, shutdown).await {
                    tracing::error!(error = %e, "http server failed");
                }
            });
        });

        Some(Self {
            stop,
            handle: Some(handle),
        })
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }

    pub fn join(&mut self) {
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

async fn run_server(
    port: u16,
    stop: Arc<AtomicBool>,
    shutdown: Option<Arc<crate::runtime::Shutdown>>,
) -> crate::error::Result<()> {
    let app = Router::new()
        .route("/", get(index))
        .route("/api/projects", get(api_projects))
        .route("/api/graph", get(api_graph))
        .route("/api/stats", get(api_stats))
        .route("/api/schema", get(api_schema))
        .route("/api/search", get(api_search))
        .route("/api/node", get(api_node))
        .route("/api/health", get(api_health))
        .layer(CorsLayer::permissive());

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| crate::error::Error::Other(format!("bind {addr}: {e}")))?;

    info!(%addr, "HTTP graph UI listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            while !stop.load(Ordering::SeqCst) {
                if shutdown.as_ref().is_some_and(|s| s.is_triggered()) {
                    break;
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
            }
        })
        .await
        .map_err(|e| crate::error::Error::Other(e.to_string()))?;

    Ok(())
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn api_health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok", "server": "cbrlm-ui" }))
}

async fn api_projects() -> impl IntoResponse {
    match crate::store::Store::list_projects() {
        Ok(projects) => Json(serde_json::json!({ "projects": projects })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string(), "projects": [] })),
    }
}

#[derive(Deserialize)]
struct GraphQuery {
    project: String,
    limit: Option<usize>,
}

async fn api_graph(Query(q): Query<GraphQuery>) -> Json<serde_json::Value> {
    let project = crate::project::normalize_project_name(&q.project);
    let limit = q.limit.unwrap_or(500);
    let body = match api::build_graph_payload(&project, limit) {
        Ok(payload) => serde_json::to_value(payload).unwrap_or_default(),
        Err(e) => serde_json::json!({
            "error": e.to_string(),
            "project": project,
            "nodes": [],
            "edges": []
        }),
    };
    Json(body)
}

#[derive(Deserialize)]
struct StatsQuery {
    project: String,
}

async fn api_schema(Query(q): Query<StatsQuery>) -> impl IntoResponse {
    let project = crate::project::normalize_project_name(&q.project);
    match crate::store::Store::open(&project) {
        Ok(store) => Json(serde_json::to_value(store.get_schema()).unwrap_or_default()),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

async fn api_stats(Query(q): Query<StatsQuery>) -> impl IntoResponse {
    let project = crate::project::normalize_project_name(&q.project);
    match crate::store::Store::open(&project) {
        Ok(store) => match store.get_architecture() {
            Ok(arch) => Json(serde_json::to_value(arch).unwrap_or_default()),
            Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
        },
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

#[derive(Deserialize)]
struct SearchQuery {
    project: String,
    q: String,
    limit: Option<usize>,
}

async fn api_search(Query(q): Query<SearchQuery>) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(20);
    match api::search_symbols(&q.project, &q.q, limit) {
        Ok(symbols) => Json(serde_json::json!({ "symbols": symbols, "total": symbols.len() })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string(), "symbols": [] })),
    }
}

#[derive(Deserialize)]
struct NodeQuery {
    project: String,
    qn: String,
}

async fn api_node(Query(q): Query<NodeQuery>) -> impl IntoResponse {
    match api::node_detail(&q.project, &q.qn) {
        Ok(Some(detail)) => Json(serde_json::to_value(detail).unwrap_or_default()),
        Ok(None) => Json(serde_json::json!({ "error": "symbol not found" })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

#[cfg(test)]
mod tests {
    use super::api::build_graph_from_store;

    #[test]
    fn builds_graph_nodes() {
        let store = crate::store::Store::open_memory().unwrap();
        store
            .upsert_symbol(&crate::store::Symbol {
                qualified_name: "a.rs::main".into(),
                name: "main".into(),
                label: "Function".into(),
                file_path: "a.rs".into(),
                line_start: 1,
                line_end: 3,
                signature: None,
                properties_json: None,
            })
            .unwrap();
        let payload = build_graph_from_store(&store, "memory", 100).unwrap();
        assert_eq!(payload.nodes.len(), 1);
        assert_eq!(payload.nodes[0].color, "#4fc3f7");
    }
}
