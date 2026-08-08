use crate::error::{Error, Result as CbmResult};
use crate::mcp::tools::{tool_definitions, ToolHandler};
use crate::rlm::RlmEngine;
use crate::watcher::Watcher;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, Content, Implementation, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ServerHandler, ServiceExt};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Semaphore;

pub const SERVER_NAME: &str = "codebase-memory-mcp";
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone)]
pub struct McpServer {
    handler: Arc<ToolHandler>,
    watcher: Option<Arc<Watcher>>,
    tools: Arc<Vec<Tool>>,
    workers: Arc<Semaphore>,
    worker_limit: usize,
}

impl McpServer {
    pub fn new() -> Self {
        let rlm = Arc::new(RlmEngine::new());
        let watcher = if watcher_enabled() {
            let watcher = Arc::new(Watcher::new());
            watcher.refresh_from_disk();
            Some(watcher)
        } else {
            None
        };
        let worker_limit = configured_worker_limit();

        Self {
            handler: Arc::new(ToolHandler::new(rlm, watcher.clone())),
            watcher,
            tools: Arc::new(model_tools()),
            workers: Arc::new(Semaphore::new(worker_limit)),
            worker_limit,
        }
    }

    pub fn watcher(&self) -> Option<Arc<Watcher>> {
        self.watcher.clone()
    }

    pub fn generated_tool_definitions() -> Vec<Value> {
        model_tools()
            .into_iter()
            .map(|tool| serde_json::to_value(tool).expect("rmcp tool must serialize"))
            .collect()
    }

    pub fn start_background_services(&self, shutdown: Option<Arc<crate::runtime::Shutdown>>) {
        if let Some(watcher) = &self.watcher {
            watcher.clone().spawn(shutdown);
        }
    }

    pub fn stop_services(&self) {
        if let Some(watcher) = &self.watcher {
            watcher.stop();
        }
    }

    pub fn run(&self) -> CbmResult<()> {
        self.run_until_shutdown(None)
    }

    pub fn run_until_shutdown(
        &self,
        shutdown: Option<Arc<crate::runtime::Shutdown>>,
    ) -> CbmResult<()> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| Error::Other(format!("failed to start Tokio runtime: {error}")))?;
        let server = self.clone();

        runtime.block_on(async move {
            if let Some(shutdown) = shutdown {
                tokio::select! {
                    result = server.clone().serve_stdio() => result,
                    _ = wait_for_shutdown(shutdown) => {
                        server.stop_services();
                        Ok(())
                    }
                }
            } else {
                server.serve_stdio().await
            }
        })
    }

    pub async fn serve_stdio(self) -> CbmResult<()> {
        let service = self
            .clone()
            .serve(rmcp::transport::stdio())
            .await
            .map_err(|error| Error::Other(format!("failed to start MCP stdio service: {error}")))?;
        let result = service.waiting().await;
        self.stop_services();
        result
            .map(|_| ())
            .map_err(|error| Error::Other(format!("MCP stdio service failed: {error}")))
    }

    async fn invoke(&self, name: String, args: Value) -> CallToolResult {
        let permit = match self.workers.clone().acquire_owned().await {
            Ok(permit) => permit,
            Err(_) => return tool_error("tool worker pool is shutting down"),
        };

        let handler = self.handler.clone();
        let log_name = name.clone();
        let result = tokio::task::spawn_blocking(move || handler.handle(&name, &args)).await;
        drop(permit);

        match result {
            Ok(Ok(value)) => match serde_json::to_string_pretty(&value) {
                Ok(text) => CallToolResult::success(vec![Content::text(text)]),
                Err(error) => tool_error(format!("failed to encode tool result: {error}")),
            },
            Ok(Err(error)) => tool_error(error.to_string()),
            Err(error) => {
                tracing::error!(tool = %log_name, %error, "CBM tool worker failed");
                tool_error("internal tool worker failure")
            }
        }
    }
}

impl ServerHandler for McpServer {
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResult, rmcp::ErrorData> {
        let name = request.name.to_string();
        if !self
            .tools
            .iter()
            .any(|tool| tool.name.as_ref() == name.as_str())
        {
            return Ok(tool_error(format!("unknown tool: {name}")));
        }

        let args = Value::Object(request.arguments.unwrap_or_default());
        Ok(self.invoke(name, args).await)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListToolsResult, rmcp::ErrorData> {
        Ok(ListToolsResult {
            tools: self.tools.as_ref().clone(),
            meta: None,
            next_cursor: None,
        })
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tools
            .iter()
            .find(|tool| tool.name.as_ref() == name)
            .cloned()
    }

    fn get_info(&self) -> ServerInfo {
        let watcher_on = self.watcher.is_some();
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(SERVER_NAME, SERVER_VERSION))
            .with_instructions(format!(
                "CBM graph + RLM server. Index with index_repository, query with search_graph/trace_path/query_graph, and use rlm_* for long-context map-reduce. Git watcher: {watcher_on}. Tool worker limit: {}.",
                self.worker_limit
            ))
    }
}

async fn wait_for_shutdown(shutdown: Arc<crate::runtime::Shutdown>) {
    while !shutdown.is_triggered() {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

fn model_tools() -> Vec<Tool> {
    tool_definitions()
        .into_iter()
        .map(tool_from_value)
        .collect()
}

fn tool_from_value(raw: Value) -> Tool {
    let name = raw
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let description = raw
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let mut schema = raw
        .get("inputSchema")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({"type": "object", "properties": {}}));
    normalize_json_schema_node(&mut schema);
    let schema = schema.as_object().cloned().unwrap_or_default();

    Tool::new(name, description, Arc::new(schema))
}

fn normalize_json_schema_node(value: &mut Value) {
    match value {
        Value::Bool(_) => *value = Value::Object(Default::default()),
        Value::Object(object) => {
            for key in ["properties", "patternProperties", "$defs", "definitions"] {
                if let Some(Value::Object(children)) = object.get_mut(key) {
                    for child in children.values_mut() {
                        normalize_json_schema_node(child);
                    }
                }
            }
            for key in [
                "items",
                "additionalProperties",
                "contains",
                "not",
                "if",
                "then",
                "else",
                "propertyNames",
            ] {
                if let Some(child) = object.get_mut(key) {
                    normalize_json_schema_node(child);
                }
            }
            for key in ["allOf", "anyOf", "oneOf", "prefixItems"] {
                if let Some(Value::Array(items)) = object.get_mut(key) {
                    for item in items {
                        normalize_json_schema_node(item);
                    }
                }
            }
        }
        _ => {}
    }
}

fn tool_error(message: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![Content::text(message.into())])
}

fn watcher_enabled() -> bool {
    let value = std::env::var("CBM_WATCHER")
        .or_else(|_| std::env::var("CBRLM_WATCHER"))
        .unwrap_or_default();
    !matches!(value.as_str(), "0" | "false" | "off")
}

fn configured_worker_limit() -> usize {
    std::env::var("CBM_MAX_TOOL_WORKERS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|value| value.get())
                .unwrap_or(2)
                .clamp(1, 4)
        })
}

impl Default for McpServer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_server_handler<T: ServerHandler>() {}

    #[test]
    fn uses_official_rmcp_server_handler() {
        assert_server_handler::<McpServer>();
        assert_eq!(SERVER_NAME, "codebase-memory-mcp");
    }

    #[test]
    fn exposes_graph_and_rlm_tools() {
        let names: Vec<String> = McpServer::generated_tool_definitions()
            .into_iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str).map(str::to_string))
            .collect();

        assert!(names.iter().any(|name| name == "index_repository"));
        assert!(names.iter().any(|name| name == "rlm_workflow"));
        assert!(names.iter().any(|name| name == "check_index_coverage"));
    }

    #[test]
    fn worker_limit_is_bounded_by_default() {
        std::env::remove_var("CBM_MAX_TOOL_WORKERS");
        assert!((1..=4).contains(&configured_worker_limit()));
    }
}
