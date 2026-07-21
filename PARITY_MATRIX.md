# CBM Rust Parity Matrix

Status key: **Done** | **Partial** | **MVP** | **Not started** | **Omitted**

Primary reference: [DeusData/codebase-memory-mcp](https://github.com/DeusData/codebase-memory-mcp) (C engine).
Secondary notes: local `knowledge-graph/` architecture docs when present.

Last updated: 2026-07-10 (FunctionRegistry import-map CALLS + tsconfig aliases).

## Status model

| Level | Meaning |
|-------|---------|
| **MVP** | Works for agent workflows; heuristic or framework-limited |
| **Partial** | Implemented with known precision or coverage gaps |
| **Done** | Matches reference contract for the supported scope |
| **Omitted** | Intentionally not implemented in Rust |

**Rust MVP rewrite is complete** (Sections 3–7). **Full reference parity is not** — this is not a complete reference replica. FoundationDB is omitted by design. See [Full parity backlog](#full-parity-backlog) below.

## Core platform

| Feature | Reference (DeusData C) | Rust (`CBM`) | Status |
|---------|-----------|----------------|--------|
| MCP stdio server | Yes | Yes | Done |
| CLI tool dispatch | Yes | Yes (`cbm cli --json --quiet`) | Done |
| Agent install/uninstall | 11 agents | Yes (Claude, Codex, Gemini, OpenCode, Zed, Aider, Antigravity, KiloCode, Kiro, …) | Partial |
| Hooks (augment, session-start) | Yes | Yes | Done |
| Multi-pass pipeline (`IndexPass`) | Yes (registry of passes) | Yes (`pipeline/pass.rs` trait + default sequence) | Done |
| Idle store connection cache | Yes | Yes (`StorePool`) | Done |
| SQLite graph store | Yes | Yes | Done |
| Compressed artifact persistence | Yes | Yes (`.codebase-memory/graph.db.zst`) | Done |
| Project naming (`cbm+` prefix) | Yes | Yes (path hash slug) | Done |
| HTTP graph UI | Yes (3D) | Yes (search, node details, edge filters) | MVP |
| Watcher / auto-reindex | Yes | Yes (backoff + dirty signature) | Done |
| Graceful shutdown / cancel | Yes | Ctrl+C stops watcher/HTTP | MVP |
| Background index jobs | Yes (supervised) | `index_repository background=true` + `IndexSupervisor` | Done |
| Hybrid LSP type resolution | 9 language families | — | Not started |
| Tree-sitter languages | 158 | 14 (Rust, Py, JS/TS, Go, Java, C, C++, Ruby, C#, PHP, Bash, Kotlin, Swift) | Partial |
| FoundationDB backend | No (SQLite) | — | Omitted (SQLite only) |

## Indexing pipeline (heuristic passes marked)

| Pass / capability | Reference | Rust | Status |
|-------------------|-----------|------|--------|
| File discovery + ignore rules | Yes | Yes (`.gitignore` + `.cbmignore`) | Done |
| Tree-sitter symbol extract | Yes | Yes (Rust, Py, JS/TS, Go, Java, C, C++) | Partial |
| Regex fallback extract | Yes | Yes | MVP |
| Stable qualified names | Yes | Yes (`file::label::name@Lline`) | Done |
| Structure nodes (Project/Folder/File) | Yes | Yes | Done |
| Import edges | Yes | Path + tsconfig aliases + Python package roots | Partial |
| CALLS edges | Yes | AST + `FunctionRegistry` (same_file/import_map/same_dir/unique) | Done |
| Store bulk transaction / replace edges | Yes | `bulk_index`, `replace_edges_of_type(s)` | Done |
| `search_code` FTS5 | Yes | `files_fts` virtual table + scan fallback | Done |
| INHERITS / IMPLEMENTS | Yes | AST (Py/JS/TS/Java/Rust/Go/C++/Ruby) + regex fallback | Done (supported langs) |
| DECORATES | Yes | Attribute patterns | Partial (heuristic) |
| HTTP route pass | Yes | `HTTP_ROUTE` multi-framework patterns | Partial |
| HTTP client→route | Yes | `HTTP_CALLS` exact/template/suffix matching | Partial |
| Git history / cross-repo | Yes | Git HEAD + dirty detection only | Partial |
| Community detection | Yes | Louvain modularity (default); components fallback | Done (Louvain) |
| Post-processing summaries | Yes | `get_architecture` + communities + dead code | Done |

## Edge types emitted

| Edge type | Emitted | Notes |
|-----------|---------|-------|
| CONTAINS | Yes | Project → folder/file → symbols |
| IMPORTS | Yes | Relative + tsconfig aliases + Python package roots (heuristic for bare packages) |
| CALLS | Yes | `FunctionRegistry`: same_file → import_map → same_dir → unique_name; AST where available |
| SIMILAR_TO | When semantic enabled | Multi-signal scoring |
| SEMANTICALLY_RELATED | When semantic enabled | Lower threshold pairs |
| RUNTIME_TRACE | Yes | Via `ingest_traces` |
| INHERITS / IMPLEMENTS / DECORATES | Yes | AST + regex; DECORATES still regex |
| HTTP_ROUTE | Yes | Framework-limited patterns |
| HTTP_CALLS | Yes | Exact / `:id`/`{id}` template / suffix path match |

## MCP tools

| Tool | Status | Notes |
|------|--------|-------|
| `index_repository` | Done | full/moderate/fast, incremental |
| `search_graph` | Done | Regex, relationship, degree, pagination |
| `trace_path` | Done | BFS |
| `get_code_snippet` | Done | |
| `get_graph_schema` | Done | Honest `implemented_edge_types` |
| `get_architecture` | Done | Counts, communities, dead code detection |
| `query_graph` | Done | SELECT-only guard |
| `check_index_coverage` | Done | File path coverage check |
| `rlm_scan` / `rlm_chunk` | Done | Persisted sessions across CLI invocations |
| `rlm_*` (other) | Partial | Workflow hints |

## Semantic system (11 signals — MVP scoring)

All 11 reference signals contribute to `combined` score. Thresholds: `SIMILAR_TO` ≥0.58, `SEMANTICALLY_RELATED` ≥0.38. Signal weights are heuristic, not reference-tuned.

## Quality gates

| Gate | Status |
|------|--------|
| `cargo fmt --check` | Done |
| `cargo test` green (parallel-safe) | Done — see CI |
| `cargo clippy --all-targets -- -D warnings` | Done |
| `cargo build --release` | Done |
| `scripts/smoke-quality-gates.*` | Done (includes `query_graph` edge diversity) |
| `scripts/smoke-release-artifact.ps1` | Done (Windows CI) |
| README + parity matrix accurate | Done (no hard-coded test counts) |

## Section 6 (Review hardening)

| # | Item | Status |
|---|------|--------|
| 6.1 | CLI `rlm_scan` session persistence | Done |
| 6.2 | Docs without stale test counts | Done |
| 6.3 | MVP vs full parity distinction | Done |
| 6.4 | Release artifact smoke | Done |
| 6.5 | Cross-language CALLS fixtures | Done |
| 6.6 | Smoke `query_graph` edge diversity | Done |
| 6.7 | `--quiet` + JSON stdout contract | Done |
| 6.8 | Full parity backlog section | Done |

## Section 7 (Post-MVP hardening)

| # | Item | Status |
|---|------|--------|
| 7.1 | Matrix-aware release artifact smoke | Done |
| 7.2 | `cargo fmt --check` in CI/smoke gates | Done |
| 7.3 | Process-level CLI JSON tests | Done |
| 7.4 | Atomic RLM session persistence | Done |
| 7.5 | Full-pipeline CALLS fixtures | Done |
| 7.6 | Installer + MCP smoke from release artifact | Done |
| 7.7 | MVP vs replica project language | Done |

## Full parity backlog

These are **not done** and should not be inferred from MVP completion:

| Item | Priority | Notes |
|------|----------|-------|
| Leiden / Louvain communities | — | **Done** Louvain single-level; Leiden multi-level not required |
| `HTTP_CALLS` pass | — | **Done** (path-match MVP; framework coverage limited) |
| Import path resolution | — | **Done** for relative JS/Py/Rust mod; absolute packages still external |
| `search_code` FTS5 | — | **Done** |
| Multi-language AST-aware CALLS | — | **Done** for 12 language families |
| Store bulk transaction API | — | **Done** (`bulk_index`, `replace_edges_of_type`) |
| INHERITS / IMPLEMENTS AST | — | **Done** for Py/JS/TS/Java/Rust/Go/C++/Ruby/Kotlin |
| Kotlin grammar | — | **Done** (`tree-sitter-kotlin-ng`) |
| Swift grammar | — | **Done** (`tree-sitter-swift`) |
| Semantic weight tuning | — | **Done** (`SignalWeights` + env thresholds) |
| Tree-sitter coverage gaps | P2 | … beyond current 14 families |
| FoundationDB backend | — | Omitted; SQLite is canonical |
| Wrapper packaging (Go/PyPI/npm/Chocolatey/AUR) | P3 | See `packaging/DEFERRED_CHANNELS.md` |
| Full reference UI (React graph-ui) | P3 | Lightweight HTML is deliberate MVP |
| Reference-grade semantic tuning | — | **Partial→Done**: tunable weights; defaults rebalanced for structure/API |

## Full parity blockers

A new agent should treat these as blockers before claiming equivalence with the reference C implementation:

1. Regex/heuristic remains for imports (absolute packages), decorators, and languages without AST profiles.
2. Community detection is Louvain single-level (not full Leiden multi-resolution).
3. HTTP routes/calls are pattern/path limited (segment template match only; no OpenAPI).
4. No Hybrid LSP type resolution (C reference has 9 language families).
5. FoundationDB and reference C foundation layer omitted by design.
