# CBRLM Rust Parity Matrix

Status key: **Done** | **Partial** | **Not started** | **Omitted**

Reference: `knowledge-graph/` (architecture, specifications, functions).

Last updated: 2026-06-12 (Sections 3–5 complete).

## Core platform

| Feature | Reference | Rust (`cbrlm`) | Status |
|---------|-----------|----------------|--------|
| MCP stdio server | Yes | Yes | Done |
| CLI tool dispatch | Yes | Yes (`cbrlm cli`) | Done |
| Agent install/uninstall | Yes | Yes (OpenCode, Codex, Claude, …) | Done |
| Hooks (augment, session-start) | Yes | Yes | Done |
| SQLite graph store | Yes | Yes | Done |
| Compressed artifact persistence | Yes | Yes (`.codebase-memory/graph.db.zst`) | Done |
| Project naming (`cbrlm+` prefix) | Yes | Yes (path hash slug) | Done |
| HTTP graph UI | Yes | Yes (edge filters, schema API) | Done |
| Watcher / auto-reindex | Yes | Yes (backoff + dirty signature) | Done |
| Graceful shutdown / cancel | Yes | Ctrl+C stops watcher/HTTP | Done |
| FoundationDB backend | Yes | — | Omitted (SQLite only) |

## Indexing pipeline

| Pass / capability | Reference | Rust | Status |
|-------------------|-----------|------|--------|
| File discovery + ignore rules | Yes | Yes (`.gitignore` + `.cbmignore`) | Done |
| Tree-sitter symbol extract | Yes | Yes (Rust, Py, JS/TS, Go, Java, C, C++) | Partial |
| Regex fallback extract | Yes | Yes | Done |
| Stable qualified names | Yes | Yes (`file::label::name@Lline`) | Done |
| Structure nodes (Project/Folder/File) | Yes | Yes | Done |
| Import edges | Yes | Yes (regex per language) | Partial |
| CALLS edges | Yes | Yes (same-file-first, unique cross-file) | Partial |
| INHERITS / IMPLEMENTS | Yes | Yes (regex per language) | Partial |
| DECORATES | Yes | Yes (attribute patterns) | Partial |
| HTTP route / config passes | Yes | `HTTP_ROUTE` regex pass (Py/JS/Rust) | Partial |
| Git history / cross-repo | Yes | Git HEAD + dirty detection only | Partial |
| Community detection (Leiden/Louvain) | Yes | Connected-components on CALLS+IMPORTS | Partial |
| Post-processing summaries | Yes | `get_architecture` only | Partial |

## Edge types emitted

| Edge type | Emitted by Rust indexer | Notes |
|-----------|-------------------------|-------|
| CONTAINS | Yes | Project → folder/file → symbols |
| IMPORTS | Yes | Regex-based module targets |
| CALLS | Yes | Heuristic call resolution |
| SIMILAR_TO | Partial | When semantic pass enabled |
| SEMANTICALLY_RELATED | Partial | When semantic pass enabled |
| RUNTIME_TRACE | Yes | Via `ingest_traces` tool |
| INHERITS | Yes | Python, Java, JS/TS |
| IMPLEMENTS | Yes | Rust, Java |
| DECORATES | Yes | Rust, Python, Java |
| HTTP_ROUTE | Yes | Python/Express/Axum patterns |
| HTTP_CALLS | No | |

`get_graph_schema` returns `implemented_edge_types` from the live project DB.

## MCP tools

| Tool | Status | Notes |
|------|--------|-------|
| `index_repository` | Done | full/moderate/fast, incremental, artifact export |
| `search_graph` | Partial | Regex name/qn, glob file, relationship, degree, pagination `has_more` |
| `trace_path` | Done | BFS inbound/outbound/both |
| `get_code_snippet` | Done | |
| `get_graph_schema` | Partial | Honest implemented edge list per project |
| `get_architecture` | Partial | Counts + top functions |
| `query_graph` | Partial | SELECT-only with string-literal aware guard |
| `search_code` | Done | |
| `list_projects` | Done | |
| `delete_project` | Done | |
| `index_status` | Done | |
| `ingest_traces` | Done | |
| `rlm_filter` | Done | Same filters as `search_graph` |
| `rlm_scan` / `rlm_chunk` / `rlm_read_symbol` | Partial | Ignore rules + byte budgets on scan |
| `rlm_workflow` | Partial | Phase hints |
| `set_adr` / `get_adr` | Done | |

## Semantic system (11 reference signals)

| Signal | Status |
|--------|--------|
| TF-IDF | Done |
| Random Indexing | Done |
| MinHash structure | Done |
| API signature vector | Done (from signature tokens) |
| Module proximity | Done (path prefix similarity) |
| Halstead complexity | Done (lightweight operator/operand) |
| Type signature vector | Done |
| Decorator pattern vector | Done |
| AST structural profile | Done |
| Approximate data flow | Done |
| Graph diffusion | Done |

Baseline: 768-dim vectors, int8 quantization, multi-signal `combined` score. Edges emit `SIMILAR_TO` (≥0.75) and `SEMANTICALLY_RELATED` (≥0.55) with per-signal `properties_json`. `vector_query` returns `score_breakdown`.

## Search contract (`search_graph`)

| Parameter | Status |
|-----------|--------|
| `query`, `label` | Done |
| `name_pattern`, `qn_pattern` (regex) | Done |
| `file_pattern` (glob) | Done |
| `relationship` | Done |
| `direction` (inbound/outbound/any) | Done |
| `min_degree` / `max_degree` | Done |
| `include_connected` | Done |
| `exclude_entry_points` | Done |
| `limit` / `offset` / `has_more` | Done |
| `vector_query` | Done (requires `CBRLM_SEMANTIC_ENABLED`) |

## Packaging & CI

| Item | Status |
|------|--------|
| `cargo test` green (parallel) | Done |
| `cargo clippy -D warnings` | Done |
| GitHub CI (Windows + Linux) | Done |
| Release workflow (win/linux/mac + arm64) | Done |
| Install scripts (ps1/sh) | Done |
| Homebrew formula | Done (livecheck + caveats) |
| Install checksum verification | Done (win/linux scripts) |
| Deferred channels doc | Done (`packaging/DEFERRED_CHANNELS.md`) |

## Quality gates (Section 4)

| Gate | Status |
|------|--------|
| `cargo test` (65 tests, parallel-safe) | Done |
| `cargo clippy --all-targets -- -D warnings` | Done |
| `cargo build --release` | Done |
| Smoke: `index_repository` / `search_graph` / `get_architecture` | Done (`scripts/smoke-quality-gates.*`) |
| CI runs gates + graph smoke on Windows/Linux | Done |
| README + parity matrix reflect reality | Done |

Parity prerequisites (Section 4):

- Graph correctness: integration fixtures for CALLS, CONTAINS, IMPORTS, INHERITS
- Multiple edge types emitted: verified in smoke + `get_architecture`
- Search contract: regex/glob, relationship, degree, pagination tests
- Default test + clippy green: yes

## Section 5 (Post-MVP parity)

| # | Item | Status |
|---|------|--------|
| 1 | `.cbmignore` + shared walker | Done |
| 2 | Store readonly + integrity + schema v2 | Done |
| 3 | HTTP route pass | Done (partial frameworks) |
| 4 | All 11 semantic signals | Done |
| 5 | Community detection | Done (components MVP) |
| 6 | HTTP UI search + node details | Done |
| 7 | Packaging checksums + deferred doc | Done |
| 8 | `CBRLM_PROFILE` + memory budget | Done |
| 9 | AST-aware CALLS (Rust) | Done |

## Roadmap pointer

Sections 3–5 are complete. Remaining: Leiden/Louvain refinement, HTTP_CALLS pass, store bulk transactions, multi-language AST CALLS, FoundationDB (omitted).