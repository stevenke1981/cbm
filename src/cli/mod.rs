use crate::agent::AgentKind;
use crate::error::Result;
use crate::http::{HttpServer, UiConfig};
use crate::install::{self, InstallOptions, UninstallOptions};
use crate::mcp::{tool_definitions, ToolHandler};
use crate::mcp::{McpServer, SERVER_NAME, SERVER_VERSION};
use crate::rlm::RlmEngine;
use serde_json::Value;
use std::sync::Arc;

pub fn print_help() {
    eprintln!(
        r#"{SERVER_NAME} v{SERVER_VERSION} — Codebase Memory MCP (Rust)

USAGE:
    cbm                          Run MCP server (stdio JSON-RPC)
    cbm [--ui] [--port=9749]         MCP server + optional graph UI
    cbm ui [--port=9749]             Graph UI only
    cbm cli [--json] [--quiet] <tool> [args_json]

CLI OUTPUT:
    --json     Machine-readable JSON on stdout; diagnostics on stderr
    --quiet    Suppress tracing logs (recommended for scripts piping stdout)
    cbm install [--dry-run] [--force] [--yes] [--all]
    cbm uninstall [--dry-run] [--yes] [--all] [--keep-binary]
    cbm hook-augment
    cbm hook-session-start
    cbm config <list|get|snippet>
    cbm --version
    cbm --help

HTTP UI:
    cbm --ui --port 9749             3D knowledge graph at http://127.0.0.1:9749
    CBM_UI=1 CBM_PORT=9749 cbm

RLM WORKFLOW:
    Phase 0: index_repository
    Phase 1: rlm_filter / search_graph
    Phase 2: rlm_read_symbol (one per call)
    Phase 3: get_architecture / rlm_workflow phase=reduce

COMPATIBLE AGENTS (aligned with DeusData/codebase-memory-mcp):
    Claude Code, Codex, Gemini CLI, Zed, OpenCode, Antigravity,
    Aider, KiloCode, VS Code, OpenClaw, Kiro
"#
    );
}

pub fn run_cli(tool: &str, args_json: Option<&str>, json_output: bool, _quiet: bool) -> Result<()> {
    let rlm = Arc::new(RlmEngine::new());
    let handler = ToolHandler::new(rlm, None);
    let args: Value = match args_json {
        Some(s) if !s.is_empty() => serde_json::from_str(s)?,
        _ => Value::Object(Default::default()),
    };
    let result = handler.handle(tool, &args)?;
    let formatted = if json_output {
        serde_json::to_string(&result)?
    } else {
        format_cli_human(tool, &result)
    };
    println!("{formatted}");
    Ok(())
}

fn format_cli_human(tool: &str, result: &Value) -> String {
    match tool {
        "index_repository" => {
            let ok = result
                .get("success")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let project = result
                .get("project")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let files = result
                .get("files_indexed")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let symbols = result
                .get("symbols_extracted")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let edges = result
                .get("edges_extracted")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let ms = result
                .get("duration_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            format!(
                "index {project}: success={ok} files={files} symbols={symbols} edges={edges} ({ms}ms)"
            )
        }
        "search_graph" | "rlm_filter" => {
            let total = result.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
            let symbols = result.get("symbols").and_then(|v| v.as_array());
            let mut lines = vec![format!("search: {total} match(es)")];
            if let Some(rows) = symbols {
                for row in rows.iter().take(10) {
                    let name = row.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                    let label = row.get("label").and_then(|v| v.as_str()).unwrap_or("");
                    let file = row.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
                    lines.push(format!("  - {name} [{label}] {file}"));
                }
                if rows.len() > 10 {
                    lines.push(format!("  ... and {} more", rows.len() - 10));
                }
            }
            lines.join("\n")
        }
        "get_architecture" => {
            let syms = result
                .get("symbol_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let edges = result
                .get("edge_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let files = result
                .get("file_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            format!("architecture: symbols={syms} edges={edges} files={files}")
        }
        "list_projects" => {
            let n = result
                .as_array()
                .map(|a| a.len())
                .or_else(|| {
                    result
                        .get("projects")
                        .and_then(|v| v.as_array())
                        .map(|a| a.len())
                })
                .unwrap_or(0);
            format!("projects: {n} indexed")
        }
        _ => serde_json::to_string_pretty(result).unwrap_or_else(|_| result.to_string()),
    }
}

pub fn run_install(opts: InstallOptions) -> Result<()> {
    let agent = AgentKind::detect();
    eprintln!("Detected agent: {} ({:?})", agent.slug(), agent);
    if opts.dry_run {
        eprintln!("(dry-run mode)");
    }
    let report = install::run_install(&opts)?;
    eprintln!(
        "\nInstall directory: {}",
        install::default_install_dir().display()
    );
    eprintln!("Binary: {}", report.binary_path.display());
    if !report.configured.is_empty() {
        eprintln!("Restart your coding agent to load MCP changes.");
    } else if opts.dry_run {
        let snippet = agent.mcp_config_snippet();
        eprintln!(
            "\nMCP config snippet:\n{}",
            serde_json::to_string_pretty(&snippet)?
        );
    }
    Ok(())
}

pub fn run_uninstall(opts: UninstallOptions) -> Result<()> {
    install::run_uninstall(&opts)?;
    Ok(())
}

pub fn run_hook_augment() {
    std::process::exit(crate::hooks::hook_augment());
}

pub fn run_hook_session_start() {
    std::process::exit(crate::hooks::hook_session_start());
}

pub fn run_config(action: &str) -> Result<()> {
    match action {
        "list" => {
            for tool in tool_definitions() {
                println!(
                    "{}",
                    tool.get("name").and_then(|v| v.as_str()).unwrap_or("?")
                );
            }
        }
        "snippet" => {
            let agent = AgentKind::detect();
            println!(
                "{}",
                serde_json::to_string_pretty(&agent.mcp_config_snippet())?
            );
        }
        _ => {
            eprintln!("Usage: cbm config <list|snippet>");
        }
    }
    Ok(())
}

pub fn run_ui_server(port: u16) -> Result<()> {
    let shutdown = crate::runtime::Shutdown::new();
    shutdown.install_ctrlc_handler();
    let config = UiConfig {
        enabled: true,
        port,
    };
    eprintln!("graph UI: http://127.0.0.1:{port} (Ctrl+C to exit)");
    let mut http = HttpServer::spawn(&config, Some(shutdown.clone()))
        .ok_or_else(|| crate::error::Error::Other("failed to start HTTP server".into()))?;
    while !shutdown.is_triggered() {
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    http.stop();
    Ok(())
}

pub fn run_mcp_server(ui_config: UiConfig) -> Result<()> {
    let shutdown = crate::runtime::Shutdown::new();
    shutdown.install_ctrlc_handler();

    let mut http = HttpServer::spawn(&ui_config, Some(shutdown.clone()));
    if ui_config.enabled {
        eprintln!("graph UI: http://127.0.0.1:{}", ui_config.port);
    }

    let mcp = McpServer::new();
    mcp.start_background_services(Some(shutdown.clone()));

    let result = mcp.run_until_shutdown(Some(shutdown.clone()));

    if let Some(ref mut server) = http {
        server.stop();
    }
    if shutdown.is_triggered() {
        eprintln!("cbm shutdown complete");
    } else if ui_config.enabled {
        if let Some(ref mut server) = http {
            eprintln!("MCP stdin closed; graph UI still running (Ctrl+C to exit)");
            server.join();
            return Ok(());
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::format_cli_human;
    use crate::mcp::ToolHandler;
    use crate::rlm::RlmEngine;
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn human_index_summary() {
        let out = format_cli_human(
            "index_repository",
            &json!({"success": true, "project": "cbm+x", "files_indexed": 3, "symbols_extracted": 10, "edges_extracted": 5, "duration_ms": 120}),
        );
        assert!(out.contains("cbm+x"));
        assert!(out.contains("symbols=10"));
    }

    #[test]
    fn json_output_is_parseable() {
        let rlm = Arc::new(RlmEngine::new());
        let handler = ToolHandler::new(rlm, None);
        let result = handler
            .handle("list_projects", &serde_json::json!({}))
            .unwrap();
        let parsed: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&result).unwrap()).unwrap();
        assert!(parsed.is_object() || parsed.is_array());
    }

    #[test]
    fn human_search_lists_symbols() {
        let out = format_cli_human(
            "search_graph",
            &json!({"total": 1, "symbols": [{"name": "main", "label": "Function", "file_path": "lib.rs"}]}),
        );
        assert!(out.contains("main"));
        assert!(out.contains("Function"));
    }
}
