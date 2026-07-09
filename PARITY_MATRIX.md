# CBM Rust Parity Matrix

Status key: **Done** | **Partial** | **MVP** | **Not started** | **Omitted**

Primary reference: [DeusData/codebase-memory-mcp](https://github.com/DeusData/codebase-memory-mcp) (C engine).
Secondary notes: local `knowledge-graph/` architecture docs when present.

Last updated: 2026-07-09 (OOP pass pipeline + StorePool alignment).

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
| Hybrid LSP type resolution | 9 language families | — | Not started |
| 158 tree-sitter languages | Yes | 8 (Rust, Py, JS/TS, Go, Java, C, C++) | Partial |
| FoundationDB backend | No (SQLite) | — | Omitted (SQLite only) |

## Indexing pipeline (heuristic passes marked)

| Pass / capability | Reference | Rust | Status |
|-------------------|-----------|------|--------|
| File discovery + ignore rules | Yes | Yes (`.gitignore` + `.cbmignore`) | Done |
| Tree-sitter symbol extract | Yes | Yes (Rust, Py, JS/TS, Go, Java, C, C++) | Partial |
| Regex fallback extract | Yes | Yes | MVP |
| Stable qualified names | Yes | Yes (`file::label::name@Lline`) | Done |
| Structure nodes (Project/Folder/File) | Yes | Yes | Done |
| Import edges | Yes | Regex per language | Partial (heuristic) |
| CALLS edges | Yes | AST: Rust/Py/JS/TS/Go/Java/C/C++ + regex fallback | Done (supported langs) |
| Store bulk transaction / replace edges | Yes | `bulk_index`, `replace_edges_of_type(s)` | Done |
| INHERITS / IMPLEMENTS | Yes | Regex per language | Partial (heuristic) |
| DECORATES | Yes | Attribute patterns | Partial (heuristic) |
| HTTP route pass | Yes | `HTTP_ROUTE` Py/Express/Axum patterns | MVP (framework-limited) |
| Git history / cross-repo | Yes | Git HEAD + dirty detection only | Partial |
| Community detection | Yes | Connected-components on CALLS+IMPORTS | MVP (not Leiden/Louvain) |
| Post-processing summaries | Yes | `get_architecture` + communities | Partial |

## Edge types emitted

| Edge type | Emitted | Notes |
|-----------|---------|-------|
| CONTAINS | Yes | Project → folder/file → symbols |
| IMPORTS | Yes | Regex-based (heuristic) |
| CALLS | Yes | Same-file-first; Rust AST where available |
| SIMILAR_TO | When semantic enabled | Multi-signal scoring |
| SEMANTICALLY_RELATED | When semantic enabled | Lower threshold pairs |
| RUNTIME_TRACE | Yes | Via `ingest_traces` |
| INHERITS / IMPLEMENTS / DECORATES | Yes | Regex (partial language coverage) |
| HTTP_ROUTE | Yes | Framework-limited patterns |
| HTTP_CALLS | No | Backlog |

## MCP tools

| Tool | Status | Notes |
|------|--------|-------|
| `index_repository` | Done | full/moderate/fast, incremental |
| `search_graph` | Done | Regex, relationship, degree, pagination |
| `trace_path` | Done | BFS |
| `get_code_snippet` | Done | |
| `get_graph_schema` | Done | Honest `implemented_edge_types` |
| `get_architecture` | MVP | Counts, communities (components) |
| `query_graph` | Done | SELECT-only guard |
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
| Leiden / Louvain communities | P2 | Replace connected-components MVP |
| `HTTP_CALLS` pass | P2 | Client fetch/axios/reqwest edges |
| Multi-language AST-aware CALLS | — | **Done** for Rust/Py/JS/TS/Go/Java/C/C++ (regex fallback otherwise) |
| Store bulk transaction API | — | **Done** (`bulk_index`, `replace_edges_of_type`) |
| Tree-sitter coverage gaps | P1 | Kotlin, Ruby, … |
| FoundationDB backend | — | Omitted; SQLite is canonical |
| Wrapper packaging (Go/PyPI/npm/Chocolatey/AUR) | P3 | See `packaging/DEFERRED_CHANNELS.md` |
| Full reference UI (React graph-ui) | P3 | Lightweight HTML is deliberate MVP |
| Reference-grade semantic tuning | P2 | 11 signals present; weights differ |

## Full parity blockers

A new agent should treat these as blockers before claiming equivalence with the reference C implementation:

1. Regex/heuristic graph passes remain for imports, inheritance, and unsupported languages (CALLS AST covers Rust/Py/JS/TS/Go/Java/C/C++).
2. Community detection is connected-components, not modularity optimization.
3. HTTP routes are pattern-limited; no `HTTP_CALLS`.
4. No Hybrid LSP type resolution (C reference has 9 language families).
5. FoundationDB and reference C foundation layer omitted by design.
