use crate::error::{Error, Result};
use crate::mcp::tools::{tool_definitions, ToolHandler};
use crate::mcp::transport::{read_stdin_message, write_stdout_message};
use crate::rlm::RlmEngine;
use crate::watcher::Watcher;
use serde_json::{json, Value};
use std::sync::Arc;

pub const SERVER_NAME: &str = "cbm-mcp";
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct McpServer {
    handler: ToolHandler,
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
            handler: ToolHandler::new(rlm, watcher.clone()),
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
        loop {
            if shutdown.as_ref().is_some_and(|s| s.is_triggered()) {
                self.stop_services();
                break;
            }
            let Some(message) = read_stdin_message()? else {
                self.stop_services();
                break;
            };
            let response = self.handle_message(&message.body)?;
            if let Some(body) = response {
                write_stdout_message(&body, message.framing)?;
            }
        }
        Ok(())
    }

    pub fn handle_message(&self, raw: &str) -> Result<Option<String>> {
        let request: Value = serde_json::from_str(raw)?;
        let id = request.get("id").cloned();
        let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");

        let result = match method {
            "initialize" => Ok(self.handle_initialize(&request)),
            "notifications/initialized" | "initialized" => return Ok(None),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tool_definitions() })),
            "tools/call" => self.handle_tool_call(&request),
            _ => {
                if id.is_none() {
                    return Ok(None);
                }
                Err(Error::InvalidArgument(format!("unknown method: {method}")))
            }
        };

        match (id, result) {
            (None, _) => Ok(None),
            (Some(id), Ok(value)) => Ok(Some(format_response(id, value)?)),
            (Some(id), Err(e)) => Ok(Some(format_error(id, -32603, &e.to_string())?)),
        }
    }

    fn handle_initialize(&self, request: &Value) -> Value {
        let _params = request.get("params");
        let watcher_on = self.watcher.is_some();
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

    fn handle_tool_call(&self, request: &Value) -> Result<Value> {
        let params = request
            .get("params")
            .ok_or_else(|| Error::InvalidArgument("missing params".into()))?;
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::InvalidArgument("missing tool name".into()))?;
        let args = params.get("arguments").cloned().unwrap_or(json!({}));
        let result = self.handler.handle(name, &args)?;
        Ok(json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&result)?
            }],
            "isError": false
        }))
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
