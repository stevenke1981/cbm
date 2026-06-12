# CBRLM — Codebase RLM Memory MCP (Rust)

Rust rewrite of `codebase-memory-mcp`: a knowledge-graph indexer and MCP server for AI coding agents (OpenCode, Codex, Claude Code, and others).

Reference docs live in [`../knowledge-graph/`](../knowledge-graph/). Implementation tracking: [`RUST_REWRITE_TODO.md`](RUST_REWRITE_TODO.md). Feature parity status: [`PARITY_MATRIX.md`](PARITY_MATRIX.md).

## Quick start

### Build

```powershell
cd D:\cbm\cbrlm
cargo build --release
```

Binary: `target/release/cbrlm.exe` (Windows) or `target/release/cbrlm` (Linux/macOS).

### Test & lint (Section 4 quality gates)

```powershell
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
```

One-shot gates + smoke checks:

```powershell
.\scripts\smoke-quality-gates.ps1
# Linux/macOS:
./scripts/smoke-quality-gates.sh
```

### Index a repository

```powershell
cargo run -- cli index_repository --json '{"repo_path":".","project":"my-app","mode":"full","persistence":false}'
```

Projects are stored as `cbrlm+<name>` in the cache directory (default: `%LOCALAPPDATA%\codebase-memory-mcp` on Windows).

### Search the graph

```powershell
cargo run -- cli search_graph --json '{"project":"my-app","query":"handler","limit":10}'
cargo run -- cli search_graph --json '{"project":"my-app","relationship":"CALLS","label":"Function"}'
cargo run -- cli search_graph --json '{"project":"my-app","name_pattern":".*Handler.*"}'
```

`name_pattern` and `qn_pattern` use **regex**. `file_pattern` uses **glob**. Responses include `has_more` for pagination.

### MCP server (stdio)

```powershell
cargo run --
# or with graph UI:
cargo run -- --ui --port 9749
```

### Install into agent config

```powershell
cargo run -- install --yes
cargo run -- install --dry-run --all
cargo run -- uninstall --yes
```

Platform scripts: `scripts/install.ps1`, `scripts/install.sh`.

### Release build

```powershell
.\scripts\build-release.ps1
# Linux/macOS:
./scripts/build-release.sh
```

GitHub Actions: `.github/workflows/ci.yml` (test + clippy + release smoke), `.github/workflows/release.yml` (multi-platform binaries).

## Environment variables

| Variable | Purpose |
|----------|---------|
| `CBRLM_CACHE_DIR` | Override SQLite cache location |
| `CBRLM_SEMANTIC_ENABLED=1` | Enable TF-IDF + RI semantic pass |
| `CBRLM_PERSISTENCE=1` | Export/import `.codebase-memory/graph.db.zst` |
| `CBRLM_WATCHER=0` | Disable background reindex watcher |
| `CBRLM_UI=1` | Enable HTTP graph UI |
| `CBRLM_PORT` | HTTP UI port (default 9749) |
| `CBRLM_PROFILE=1` | Log per-phase index timings |
| `CBRLM_MEMORY_BUDGET_MB` | Max bytes reserved during file indexing (default 512) |

## MCP tools (summary)

| Tool | Status |
|------|--------|
| `index_repository` | Full / moderate / fast modes, incremental |
| `search_graph` | Regex patterns, relationship/degree filters, vector query |
| `trace_path` | BFS over call graph |
| `get_code_snippet` | Symbol source |
| `get_graph_schema` | Labels + implemented edge types |
| `get_architecture` | Counts and top symbols |
| `query_graph` | Read-only SELECT |
| `search_code` | Full-text file search |
| `rlm_*` | RLM map-reduce workflow helpers |

See [`PARITY_MATRIX.md`](PARITY_MATRIX.md) for detailed parity vs the reference system.

## Graph model (current)

- **QN format**: `{file}::{label}::{name}@L{line}`
- **Emitted edges**: `CONTAINS`, `IMPORTS`, `CALLS`, `INHERITS`, `IMPLEMENTS`, `DECORATES`, optional `SIMILAR_TO` / `SEMANTICALLY_RELATED`, `RUNTIME_TRACE` via `ingest_traces`
- **Structure nodes**: `Project`, `Folder`, `File`

## Project layout

```
src/
  pipeline/     Index passes (discover, extract, structure, imports, calls)
  store/        SQLite graph store + search
  mcp/          JSON-RPC MCP server
  semantic/     Multi-signal similarity (TF-IDF, RI, MinHash, API sig, module, Halstead)
  rlm/          RLM scan/chunk/session workflow
  http/         Optional 3D graph UI
```

## Contributing / next work

Sections 3–5 of [`RUST_REWRITE_TODO.md`](RUST_REWRITE_TODO.md) are complete. Remaining P2/P3: Leiden communities, `HTTP_CALLS`, store bulk transactions, multi-language AST CALLS. Run `.\scripts\smoke-quality-gates.ps1` before claiming a milestone.