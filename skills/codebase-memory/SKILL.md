---
name: codebase-memory
description: Use the integrated cbm graph and RLM tools before broad text search when exploring, tracing, reviewing, editing, or reducing large codebases and logs.
compatibility: opencode, claude-code, codex
---

# Codebase Memory workflow

`cbm` now provides both the code knowledge graph and the RLM long-context tools from one MCP server. Prefer graph tools for code structure and symbol relationships; prefer `rlm_*` tools for large logs, generated output, long documents, or chunked map-reduce work.

## Quick decision matrix

| Question | Start with |
| --- | --- |
| Is this repository indexed? | `list_projects`, then `index_status` |
| Where is a function, type, route, or class? | `search_graph` |
| What calls this or what does it call? | `trace_path` |
| What is the exact source for a symbol? | `get_code_snippet` |
| What is the high-level architecture? | `get_architecture` |
| Which files changed since the index? | `detect_changes` |
| I need an exact graph query | `get_graph_schema`, then `query_graph` |
| I need source-text matches | `search_code` |
| I need to verify requested files are indexed | `check_index_coverage` |
| I need to inspect a very large text corpus | `rlm_scan`, then `rlm_peek` or `rlm_chunk` |
| I need guidance for map/filter/reduce | `rlm_workflow` |

## Graph-first exploration

1. Run `list_projects` and locate the repository. Project names normally use the `cbm+` prefix.
2. Run `index_repository` when the repository is missing or stale. Use `background=true` for a large index and poll `index_status` with the returned `job_id`.
3. Use `search_graph` to identify symbols with `query`, `label`, `name_pattern`, `qn_pattern`, or `file_pattern`.
4. Use `trace_path` for callers and callees, and `get_code_snippet` before editing an exact symbol.
5. Use `get_architecture` for module boundaries and hotspots.
6. After edits, run `detect_changes` and refresh the index incrementally when needed.

## RLM workflow

1. Call `rlm_workflow` for phase guidance.
2. Use `rlm_filter` for graph-backed narrowing when the material is indexed code.
3. Use `rlm_scan` for large directories or text corpora that do not belong in the graph.
4. Use `rlm_peek` to find relevant passages without reading every chunk.
5. Use `rlm_chunk` to process selected chunks in bounded batches.
6. Use `rlm_session_list` and `rlm_session_delete` to manage temporary scan sessions.

## Tool reference

### Graph and indexing

- `index_repository`: build or refresh a repository graph; supports background jobs.
- `index_status`: inspect graph and background-index status.
- `search_graph`: search symbols and connected graph context.
- `trace_path`: trace call paths.
- `get_code_snippet`: read source for a qualified symbol.
- `get_graph_schema`: inspect graph tables and fields.
- `get_architecture`: summarize repository architecture.
- `query_graph`: run read-only graph SQL.
- `search_code`: full-text source search.
- `list_projects`: list indexed repositories.
- `delete_project`: remove an obsolete index.
- `detect_changes`: compare the worktree and indexed Git state.
- `manage_adr`: read, write, or delete architecture decision records.
- `ingest_traces`: add runtime trace edges.
- `check_index_coverage`: verify that requested paths are indexed.

### RLM

- `rlm_workflow`: map/filter/reduce guidance.
- `rlm_filter`: graph-backed filtering.
- `rlm_read_symbol`: bounded symbol-level map unit.
- `rlm_scan`: create a session from a large directory or corpus.
- `rlm_chunk`: read bounded session chunks.
- `rlm_peek`: search within a session.
- `rlm_session_list`: list sessions.
- `rlm_session_delete`: delete a session.

## Quality and safety

- Graph output is an index, not a substitute for reading the file that will be changed.
- Inspect `get_graph_schema` before writing a new `query_graph` statement.
- Use normal file reads for configuration, documentation, generated files, lockfiles, and assets.
- Reindex when graph results are stale.
- Keep tool calls focused. The server limits concurrent blocking work to avoid CPU saturation; `CBM_MAX_TOOL_WORKERS` may tune that limit.
- The background watcher uses adaptive idle backoff. Set `CBM_WATCHER=0` only when strictly on-demand indexing is preferred.
