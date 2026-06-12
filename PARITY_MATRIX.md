# CBRLM Rust Parity Matrix

Status key: **Done** | **Partial** | **MVP** | **Not started** | **Omitted**

Reference: `knowledge-graph/` (architecture, specifications, functions).

Last updated: 2026-06-12 (Section 7 review applied).

## Status model

| Level | Meaning |
|-------|---------|
| **MVP** | Works for agent workflows; heuristic or framework-limited |
| **Partial** | Implemented with known precision or coverage gaps |
| **Done** | Matches reference contract for the supported scope |
| **Omitted** | Intentionally not implemented in Rust |

**Rust MVP rewrite is complete** (Sections 3–7). **Full reference parity is not** — this is not a complete reference replica. FoundationDB is omitted by design. See [Full parity backlog](#full-parity-backlog) below.

## Core platform

| Feature | Reference | Rust (`cbrlm`) | Status |
|---------|-----------|----------------|--------|
| MCP stdio server | Yes | Yes | Done |
| CLI tool dispatch | Yes | Yes (`cbrlm cli --json --quiet`) | Done |
| Agent install/uninstall | Yes | Yes (OpenCode, Codex, Claude, …) | Done |
| Hooks (augment, session-start) | Yes | Yes | Done |
| SQLite graph store | Yes | Yes | Done |
| Compressed artifact persistence | Yes | Yes (`.codebase-memory/graph.db.zst`) | Done |
| Project naming (`cbrlm+` prefix) | Yes | Yes (path hash slug) | Done |
| HTTP graph UI | Yes | Yes (search, node details, edge filters) | MVP |
| Watcher / auto-reindex | Yes | Yes (backoff + dirty signature) | Done |
| Graceful shutdown / cancel | Yes | Ctrl+C stops watcher/HTTP | MVP |
| FoundationDB backend | Yes | — | Omitted (SQLite only) |

## Indexing pipeline (heuristic passes marked)

| Pass / capability | Reference | Rust | Status |
|-------------------|-----------|------|--------|
| File discovery + ignore rules | Yes | Yes (`.gitignore` + `.cbmignore`) | Done |
| Tree-sitter symbol extract | Yes | Yes (Rust, Py, JS/TS, Go, Java, C, C++) | Partial |
| Regex fallback extract | Yes | Yes | MVP |
| Stable qualified names | Yes | Yes (`file::label::name@Lline`) | Done |
| Structure nodes (Project/Folder/File) | Yes | Yes | Done |
| Import edges | Yes | Regex per language | Partial (heuristic) |
| CALLS edges | Yes | Rust AST + regex fallback | Partial (heuristic except Rust AST) |
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
| Store bulk transaction API | P1 | Rollback + bulk pragma mode |
| Multi-language AST-aware CALLS | P1 | Python, JS/TS, Go, Java, C/C++ |
| Tree-sitter coverage gaps | P1 | Kotlin, Ruby, … |
| FoundationDB backend | — | Omitted; SQLite is canonical |
| Wrapper packaging (Go/PyPI/npm/Chocolatey/AUR) | P3 | See `packaging/DEFERRED_CHANNELS.md` |
| Full reference UI (React graph-ui) | P3 | Lightweight HTML is deliberate MVP |
| Reference-grade semantic tuning | P2 | 11 signals present; weights differ |

## Full parity blockers

A new agent should treat these as blockers before claiming equivalence with the reference C implementation:

1. Regex/heuristic graph passes (imports, inheritance, most CALLS).
2. Community detection is connected-components, not modularity optimization.
3. HTTP routes are pattern-limited; no `HTTP_CALLS`.
4. Store lacks bulk-index transaction staging.
5. FoundationDB and reference C foundation layer omitted by design.