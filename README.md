# CBRLM - Codebase RLM Memory MCP (Rust)

CBRLM is a Rust MCP server and CLI for indexing codebases into a local knowledge graph for AI coding agents. It is designed for OpenCode, Codex, Claude Code, and other MCP-capable developer tools.

This repository is a Rust MVP rewrite of `cbrlm-mcp`. It is usable for agent workflows today, but it is not a full reference replica. SQLite is the canonical store in this Rust version; FoundationDB is intentionally omitted.

Reference material: [`../knowledge-graph/`](../knowledge-graph/)

Implementation history: [`RUST_REWRITE_TODO.md`](RUST_REWRITE_TODO.md)

Parity status: [`PARITY_MATRIX.md`](PARITY_MATRIX.md)

## Status Snapshot

| Area | Current state |
|------|---------------|
| Rust MVP rewrite | Complete through Sections 3-7 |
| MCP stdio server | Usable |
| CLI tool dispatch | Usable with `--json --quiet` |
| Agent install hooks | OpenCode, Codex, Claude-style configs |
| MCP handoff package | `packaging/mcp/` templates and manifest |
| Graph store | SQLite + optional `.codebase-memory/graph.db.zst` export |
| Semantic edges | Optional via `CBRLM_SEMANTIC_ENABLED=1` |
| Full reference parity | Not complete; see `PARITY_MATRIX.md` backlog |

## For Humans

Use this path when you want to build, install, test, or inspect the tool yourself.

### Build

```powershell
cd D:\cbm\cbrlm
cargo build --release
```

Binary paths:

- Windows: `target\release\cbrlm.exe`
- Linux/macOS: `target/release/cbrlm`

### Run Quality Gates

```powershell
cargo fmt --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo build --release
.\scripts\smoke-quality-gates.ps1 -SkipBuild
.\scripts\smoke-release-artifact.ps1 -SkipBuild
```

Linux/macOS:

```bash
cargo fmt --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo build --release
./scripts/smoke-quality-gates.sh --skip-build
```

### Index This Repository

```powershell
.\target\release\cbrlm.exe cli index_repository --json --quiet '{"repo_path":".","project":"cbrlm-review","mode":"full","persistence":false}'
```

Project names are stored with a `cbrlm+` prefix. For example, `cbrlm-review` becomes `cbrlm+cbrlm-review`.

### Search The Graph

```powershell
.\target\release\cbrlm.exe cli search_graph --json --quiet '{"project":"cbrlm-review","query":"handler","limit":10}'
.\target\release\cbrlm.exe cli search_graph --json --quiet '{"project":"cbrlm-review","relationship":"CALLS","label":"Function"}'
.\target\release\cbrlm.exe cli trace_path --json --quiet '{"project":"cbrlm-review","function_name":"run_cli","direction":"both","depth":2}'
```

`name_pattern` and `qn_pattern` use regex. `file_pattern` uses glob. Paginated responses include `has_more`.

### Run MCP Server

```powershell
.\target\release\cbrlm.exe
```

With the optional graph UI:

```powershell
.\target\release\cbrlm.exe --ui --port 9749
```

### Install Into Agent Config

```powershell
.\target\release\cbrlm.exe install --yes
.\target\release\cbrlm.exe install --dry-run --all
.\target\release\cbrlm.exe uninstall --yes
```

Default install copies the current binary into a stable per-user location first:

```text
%USERPROFILE%\.config\cbrlm\bin\cbrlm.exe
```

Agent configs should point at that stable binary, not at a git clone's `target\release` path. This matters because `cargo clean`, deleting the clone, or rebuilding elsewhere can invalidate `target\release\cbrlm.exe`.

OpenCode notes:

- Existing `.config\opencode\opencode.jsonc` and `.config\opencode\opencode.json` files are detected even when the current shell cannot identify itself as OpenCode.
- Existing `mcp.cbm` entries are updated in place to the stable binary path.
- New OpenCode configs use the MCP server name `cbrlm-mcp`.

Platform helper scripts:

- `scripts\install.ps1`
- `scripts/install.sh`

### Package As MCP For Other Agents

CBRLM is packaged as the MCP server `cbrlm-mcp`.

Automatic install:

```powershell
.\target\release\cbrlm.exe install --yes --all
```

Manual handoff package:

```text
packaging/mcp/
  README.md
  manifest.json
  generic-mcp.json
  codex-config.toml
  opencode.json
  claude-settings.json
```

Use `packaging/mcp/manifest.json` when an agent wants a machine-readable package summary. Use the config templates when an MCP client needs manual setup. Replace `{{CBRLM_BINARY}}` with the absolute path to the built or installed binary.

### Release Packaging

```powershell
.\scripts\build-release.ps1
.\scripts\smoke-release-artifact.ps1 -SkipBuild
```

The release smoke verifies the packaged archive, checksum, extracted binary, CLI indexing, install dry-run, and a minimal MCP `initialize` / `tools/list` round trip.

## For Agents And LLMs

Read this section before changing code. It is written as an operational contract for coding agents.

### What To Believe

- Treat this repo as a Rust MVP rewrite, not a complete reference replica.
- Do not claim full parity unless the backlog in [`PARITY_MATRIX.md`](PARITY_MATRIX.md#full-parity-backlog) is closed.
- FoundationDB is omitted by design; do not reintroduce it unless the project direction changes.
- Regex and heuristic graph passes are useful but still have precision limits.

### Discovery Order

Prefer graph-native discovery over broad text search:

1. `index_repository` to refresh the project graph.
2. `search_graph` to find symbols, tools, handlers, and modules.
3. `trace_path` to inspect callers/callees.
4. `get_code_snippet` or `rlm_read_symbol` to read one target symbol.
5. `query_graph` for read-only SQL-style graph checks.
6. Fall back to `rg` for docs, configs, scripts, literal strings, and when graph results are insufficient.

Recommended local index command:

```powershell
.\target\release\cbrlm.exe cli index_repository --json --quiet '{"repo_path":".","project":"cbrlm-local","mode":"full","persistence":false}'
```

### Required Verification Before Claiming Done

For most code changes, run:

```powershell
cargo fmt --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```

For graph, CLI, packaging, install, release, or MCP changes, also run:

```powershell
cargo build --release
.\scripts\smoke-quality-gates.ps1 -SkipBuild
.\scripts\smoke-release-artifact.ps1 -SkipBuild
```

For Linux/macOS-only edits, use the `.sh` smoke script where appropriate.

### Common Task Map

| Task | Start here |
|------|------------|
| MCP tool behavior | `src/mcp/tools.rs`, `src/mcp/server.rs` |
| CLI flags/output | `src/main.rs`, `src/cli/mod.rs`, `tests/cli_process_test.rs` |
| Index pipeline | `src/pipeline/` |
| CALLS precision | `src/pipeline/calls.rs`, `src/pipeline/calls_ast.rs`, `tests/calls_*_test.rs` |
| RLM scan/chunk | `src/rlm/`, especially `session.rs` and `persistence.rs` |
| Store/query behavior | `src/store/` |
| Semantic edges | `src/semantic/` |
| Installer behavior | `src/install/mod.rs`, `scripts/install.*`, `packaging/` |
| Release smoke | `scripts/smoke-release-artifact.ps1`, `.github/workflows/release.yml` |

### Safe Commit Rules

- Do not commit `target/`, `dist/`, cache directories, or local temp files.
- Keep docs honest about MVP vs full reference parity.
- If you change README claims, update `PARITY_MATRIX.md` or `RUST_REWRITE_TODO.md` when the claim affects parity/status.
- If you add a new supported behavior, add a regression test or smoke gate near the behavior.
- If you change the MCP server name, command shape, or environment contract, update `packaging/mcp/` and the installer together.

## CLI Output Contract

- `--json` writes machine-readable JSON to stdout.
- Diagnostics and index progress logs go to stderr.
- `--quiet` suppresses tracing logs and is recommended for scripts.

```powershell
cbrlm cli index_repository --json --quiet '{"repo_path":".","project":"x","mode":"fast"}' 2>$null
```

`rlm_scan` sessions persist under `%LOCALAPPDATA%\cbrlm-mcp\rlm-sessions` or `CBRLM_CACHE_DIR`, so `rlm_chunk` works across separate CLI invocations.

## Environment Variables

| Variable | Purpose |
|----------|---------|
| `CBRLM_CACHE_DIR` | Override SQLite/cache location |
| `CBRLM_SEMANTIC_ENABLED=1` | Enable semantic vector pass and semantic edges |
| `CBRLM_PERSISTENCE=1` | Export/import `.codebase-memory/graph.db.zst` |
| `CBRLM_WATCHER=0` | Disable background reindex watcher |
| `CBRLM_UI=1` | Enable HTTP graph UI |
| `CBRLM_PORT` | HTTP UI port, default `9749` |
| `CBRLM_PROFILE=1` | Log per-phase index timings |
| `CBRLM_MEMORY_BUDGET_MB` | Max memory budget for file indexing, default `512` |

## MCP Tools

| Tool | Purpose |
|------|---------|
| `index_repository` | Build or refresh a project graph |
| `index_status` | Check indexed state |
| `search_graph` / `rlm_filter` | Search symbols with regex, glob, relationship, degree, pagination |
| `trace_path` | Trace call paths inbound/outbound/both |
| `get_code_snippet` / `rlm_read_symbol` | Read source for one symbol |
| `query_graph` | Read-only `SELECT` queries |
| `get_graph_schema` | Labels, edge types, schema summary |
| `get_architecture` | Counts, top symbols, communities |
| `search_code` | Literal code search inside indexed files |
| `rlm_scan` / `rlm_chunk` / `rlm_peek` | Chunk large non-code blobs/logs for RLM workflows |
| `detect_changes` | Git-aware change summary |
| `manage_adr` | Store architecture decision notes |
| `ingest_traces` | Add runtime trace edges |

## Graph Model

- Qualified name format: `{file}::{label}::{name}@L{line}`
- Structure nodes: `Project`, `Folder`, `File`
- Core edges: `CONTAINS`, `IMPORTS`, `CALLS`, `INHERITS`, `IMPLEMENTS`, `DECORATES`, `HTTP_ROUTE`
- Optional edges: `SIMILAR_TO`, `SEMANTICALLY_RELATED`, `RUNTIME_TRACE`
- Not emitted yet: `HTTP_CALLS`

## Project Layout

```text
src/
  pipeline/     Index passes: discover, extract, structure, imports, calls, routes
  store/        SQLite graph store, search, schema, query helpers
  mcp/          JSON-RPC MCP server and tool dispatch
  semantic/     Multi-signal similarity and vector scoring
  rlm/          RLM scan/chunk/session workflow and persistence
  http/         Optional graph UI
  install/      Agent config installation and uninstall
tests/          Integration, CLI process, CALLS precision, hook tests
scripts/        Build, install, package, and smoke scripts
packaging/      Deferred package manager metadata and installers
```

## Next Work

The main project contract is now: keep the Rust MVP stable while closing full-parity gaps deliberately.

Start with:

1. [`PARITY_MATRIX.md`](PARITY_MATRIX.md) for current claims and blockers.
2. [`RUST_REWRITE_TODO.md`](RUST_REWRITE_TODO.md) for historical implementation slices.
3. `tests/calls_pipeline_test.rs` and `tests/cli_process_test.rs` before changing graph precision or CLI behavior.

High-value backlog areas:

- Leiden/Louvain-grade communities.
- `HTTP_CALLS` client-edge pass.
- Store bulk transaction API and rollback tests.
- Multi-language AST-aware CALLS beyond Rust.
- Reference-grade semantic tuning.
