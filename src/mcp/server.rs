use crate::error::{Error, Result};
use crate::mcp::tools::{tool_definitions, ToolHandler};
use crate::mcp::transport::{read_stdin_message, write_stdout_message};
use crate::rlm::RlmEngine;
use crate::watcher::Watcher;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

pub const SERVER_NAME: &str = "cbm-mcp";
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct McpServer {
    handler: Arc<ToolHandler>,
    watcher: Option<Arc<Watcher>>,
}

impl McpServer {
    pub fn new() -> Self {
        let rlm = Arc::new(RlmEngine::new());
        let watcher = if watcher_enabled() {
            let w = Arc::new(Watcher::new());
            w.refresh_from_disk();
            Some(w)
        } else {
            None
        };
        Self {
            handler: Arc::new(ToolHandler::new(rlm, watcher.clone())),
            watcher,
        }
    }

    pub fn watcher(&self) -> Option<Arc<Watcher>> {
        self.watcher.clone()
    }

    pub fn start_background_services(&self, shutdown: Option<Arc<crate::runtime::Shutdown>>) {
        if let Some(w) = &self.watcher {
            let w = w.clone();
            w.spawn(shutdown);
        }
    }

    pub fn stop_services(&self) {
        if let Some(w) = &self.watcher {
            w.stop();
        }
    }

    pub fn run(&self) -> Result<()> {
        self.run_until_shutdown(None)
    }

    pub fn run_until_shutdown(
        &self,
        shutdown: Option<Arc<crate::runtime::Shutdown>>,
    ) -> Result<()> {
        // Mutex-protected stdout so concurrent tool threads can write responses.
        let stdout_lock = Arc::new(Mutex::new(()));

        loop {
            if shutdown.as_ref().is_some_and(|s| s.is_triggered()) {
                self.stop_services();
                break;
            }
            let Some(message) = read_stdin_message()? else {
                self.stop_services();
                break;
            };

            // Fast path: parse to check if this is a tools/call request.
            let request: Value = match serde_json::from_str(&message.body) {
                Ok(v) => v,
                Err(e) => {
                    let err = format_error(Value::Null, -32700, &e.to_string())?;
                    write_stdout_message(&err, message.framing)?;
                    continue;
                }
            };
            let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");

            if method == "tools/call" {
                // Spawn a worker thread so long-running tools don't block stdin.
                let handler = self.handler.clone();
                let framing = message.framing;
                let out = stdout_lock.clone();
                std::thread::spawn(move || {
                    let response = handle_request(&handler, &request);
                    if let Some(body) = response {
                        let _guard = out.lock().unwrap();
                        let _ = write_stdout_message(&body, framing);
                    }
                });
            } else {
                // Handle non-tool messages synchronously (initialize, tools/list, etc.)
                let response = handle_request(&self.handler, &request);
                if let Some(body) = response {
                    write_stdout_message(&body, message.framing)?;
                }
            }
        }
        Ok(())
    }

    pub fn handle_message(&self, raw: &str) -> Result<Option<String>> {
        let request: Value = serde_json::from_str(raw)?;
        Ok(handle_request(&self.handler, &request))
    }
}

fn handle_initialize() -> Value {
    let watcher_on = watcher_enabled();
    json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {
            "tools": { "listChanged": false }
        },
        "serverInfo": {
            "name": SERVER_NAME,
            "version": SERVER_VERSION
        },
        "instructions": format!(
            "CBM server. Index first with index_repository. RLM: rlm_workflow → filter → map → reduce. Projects use cbm+ prefix. Git watcher: {watcher_on}."
        )
    })
}

fn handle_tool_call(handler: &ToolHandler, request: &Value) -> Result<Value> {
    let params = request
        .get("params")
        .ok_or_else(|| Error::InvalidArgument("missing params".into()))?;
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::InvalidArgument("missing tool name".into()))?;
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    let result = handler.handle(name, &args)?;
    Ok(json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string_pretty(&result)?
        }],
        "isError": false
    }))
}

/// Dispatch a parsed JSON-RPC request. Usable from any thread.
fn handle_request(handler: &ToolHandler, request: &Value) -> Option<String> {
    let id = request.get("id").cloned();
    let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");

    let result = match method {
        "initialize" => Ok(handle_initialize()),
        "notifications/initialized" | "initialized" => return None,
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tool_definitions() })),
        "tools/call" => handle_tool_call(handler, request),
        _ => {
            id.as_ref()?;
            Err(Error::InvalidArgument(format!("unknown method: {method}")))
        }
    };

    match (id, result) {
        (None, _) => None,
        (Some(id), Ok(value)) => format_response(id, value).ok(),
        (Some(id), Err(e)) => format_error(id, -32603, &e.to_string()).ok(),
    }
}

fn watcher_enabled() -> bool {
    !matches!(
        std::env::var("CBM_WATCHER").as_deref(),
        Ok("0") | Ok("false") | Ok("off")
    )
}

fn format_response(id: Value, result: Value) -> Result<String> {
    Ok(serde_json::to_string(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    }))?)
}

fn format_error(id: Value, code: i32, message: &str) -> Result<String> {
    Ok(serde_json::to_string(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    }))?)
}

impl Default for McpServer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_initialize() {
        std::env::set_var("CBM_WATCHER", "0");
        std::env::set_var("CBM_UI", "0");
        let server = McpServer::new();
        let req = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        });
        let resp = server.handle_message(&req.to_string()).unwrap().unwrap();
        assert!(resp.contains("cbm-mcp"));
    }

    #[test]
    fn lists_tools() {
        std::env::set_var("CBM_WATCHER", "0");
        std::env::set_var("CBM_UI", "0");
        let server = McpServer::new();
        let req = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        });
        let resp = server.handle_message(&req.to_string()).unwrap().unwrap();
        assert!(resp.contains("index_repository"));
        assert!(resp.contains("rlm_workflow"));
    }
}
