# CBRLM MCP package

This directory is the handoff package for agents and MCP clients that want to use CBRLM as `cbrlm-mcp`.

Server name: `cbrlm-mcp`

Transport: stdio

Binary: `cbrlm` or an absolute path to `cbrlm.exe` / `cbrlm`

## Fast path

Build or install the binary first:

```powershell
cargo build --release
.\target\release\cbrlm.exe install --yes --all
```

Then restart the target agent.

The installer writes native config for OpenCode, Codex, Claude-style `mcpServers` clients, Gemini CLI, Zed, and a fallback MCP JSON file.

## Manual config

Use these templates when an agent cannot run the installer:

| Template | Target |
|----------|--------|
| `generic-mcp.json` | Claude-style `mcpServers` clients, Gemini CLI, Zed, Aider-like clients |
| `codex-config.toml` | Codex `config.toml` snippet |
| `opencode.json` | OpenCode `opencode.json` snippet |
| `claude-settings.json` | Claude Code / Claude Desktop-style settings |
| `manifest.json` | Machine-readable package summary for agents |

Replace `{{CBRLM_BINARY}}` with an absolute binary path.

Windows example:

```text
C:\Users\you\.config\cbrlm\bin\cbrlm.exe
```

Unix example:

```text
/home/you/.config/cbrlm/bin/cbrlm
```

`opencode.json` uses a direct command array. If your OpenCode setup keeps the server under an existing `cbm` key, update that key's `command` value instead of adding a second server.

## Required environment

All templates include:

```json
{
  "CBRLM_PROJECT_PREFIX": "cbrlm+",
  "CBRLM_AGENT": "generic"
}
```

Agents may change `CBRLM_AGENT` to their own slug, for example `codex`, `opencode`, or `claude-code`.

## Smoke test

After wiring an MCP client, verify the server exposes tools:

```powershell
.\target\release\cbrlm.exe --version
.\target\release\cbrlm.exe cli list_projects --json --quiet "{}"
.\scripts\smoke-release-artifact.ps1 -SkipBuild
```

The release smoke includes a minimal MCP `initialize` and `tools/list` round trip.

## Tool contract

Primary discovery tools:

1. `index_repository`
2. `search_graph`
3. `trace_path`
4. `get_code_snippet`
5. `query_graph`
6. `get_architecture`

RLM helpers:

1. `rlm_scan`
2. `rlm_chunk`
3. `rlm_peek`
4. `rlm_filter`
5. `rlm_read_symbol`
6. `rlm_workflow`

Use graph tools before broad file search whenever a project is indexed.
