# Rust Rewrite TODO

This file translates the current review findings into an implementation TODO list
for agents working on the Rust rewrite of `cbrlm-mcp`.

Reference material:

- `D:\cbm\knowledge-graph\architecture.md`
- `D:\cbm\knowledge-graph\functions.md`
- `D:\cbm\knowledge-graph\knowledge-graph.md`
- `D:\cbm\knowledge-graph\specifications.md`

Current Rust project:

- `D:\cbm\cbrlm`

Goal:

- Move the Rust version from "working MVP / vertical slice" toward a feature-equivalent Rust rewrite of the reference knowledge-graph system.

## Completion Levels

- `P0`: Blocks graph correctness or basic trust in query results.
- `P1`: Required for feature-equivalent Rust rewrite.
- `P2`: Important for production readiness and packaging parity.
- `P3`: Nice-to-have polish after core parity.

## Section 1 - Missing Full-Clone Features

### TODO 1.1 - Expand Pipeline Passes Beyond Discover/Extract/CALLS

Priority: `P1`

Current state:

- Rust currently has a simplified pipeline: discover files, extract symbols, store files/symbols, rebuild `CALLS`, optionally run simplified semantic pass.
- Reference pipeline includes Structure, Bulk Load, Extract, Imports, Calls, Usages, Semantic, Post-processing, Tests, Communities, HTTP links, Config, Git history, infra scans, route nodes, package maps, and cross-repo analysis.

Required work:

- Add a pass abstraction so each graph-building stage is explicit and testable.
- Add structure nodes: Project, Folder, Package, File, Module.
- Add import parsing pass.
- Add usage/type-reference pass.
- Add route/config/build metadata passes where practical.
- Add post-processing pass for graph summaries and derived edges.

Acceptance criteria:

- `index_repository` produces more than Function/Class nodes for a mixed fixture.
- `get_graph_schema` reports implemented node and edge types accurately.
- Integration tests prove at least `CONTAINS`, `IMPORTS`, and `CALLS` are emitted from one fixture repo.

### TODO 1.2 - Implement Full Edge-Type Coverage

Priority: `P1`

Current state:

- Actual indexed Rust graph currently emits only `CALLS`.
- Schema advertises many edge types, but they are mostly not produced.

Required work:

- Implement producers for these reference edge types:
  - `CONTAINS`
  - `IMPORTS`
  - `INHERITS`
  - `IMPLEMENTS`
  - `DECORATES`
  - `SIMILAR_TO`
  - `SEMANTICALLY_RELATED`
  - `HTTP_ROUTE`
  - `HTTP_CALLS`
  - `ASYNC_CALLS`
- Keep `get_graph_schema` honest: only advertise edge types that are implemented, or include implementation status.

Acceptance criteria:

- Each implemented edge type has at least one integration fixture and assertion.
- `query_graph` over `edges` shows multiple edge types for a representative test repo.
- `search_graph` can filter by relationship/edge type once relationship filtering is implemented.

### TODO 1.3 - Match `search_graph` Contract

Priority: `P1`

Current state:

- Reference spec includes filters such as `relationship`, `direction`, `min_degree`, `max_degree`, `include_connected`, and `exclude_entry_points`.
- Rust search schema currently exposes query, label, name/qn/file patterns, limit, offset, vector query.
- `name_pattern` and `qn_pattern` are implemented as glob patterns, while the reference docs describe regex-like patterns.

Required work:

- Decide and document pattern semantics: regex or glob.
- Prefer regex for `name_pattern` and `qn_pattern` to match the reference docs and agent examples.
- Add relationship filtering.
- Add degree filters.
- Add connected-symbol expansion.
- Add entry-point exclusion.
- Add `has_more` pagination metadata if keeping compatibility with the existing MCP response style.

Acceptance criteria:

- `search_graph(name_pattern=".*Handler.*")` works if regex semantics are chosen.
- `search_graph(relationship="CALLS")` returns only nodes participating in CALLS edges.
- `search_graph(min_degree=2)` returns only symbols meeting the degree threshold.
- Response shape is documented and covered by integration tests.

### TODO 1.4 - Implement Rich Semantic System

Priority: `P1`

Current state:

- Rust semantic pass has 768-dimensional vectors, TF-IDF, Random Indexing, int8 quantization, and a similarity threshold.
- Reference semantic system describes 11 signals:
  - TF-IDF
  - Random Indexing
  - MinHash structure
  - API signature vector
  - Type signature vector
  - Module proximity
  - Decorator pattern vector
  - AST structural profile
  - Approximate data flow
  - Graph diffusion
  - Halstead lightweight complexity

Required work:

- Keep the current TF-IDF + RI implementation as the baseline.
- Add modular scoring functions for each missing signal.
- Store per-edge score breakdown in `properties_json`.
- Emit both `SIMILAR_TO` and `SEMANTICALLY_RELATED` where appropriate.
- Add deterministic tests using small fixtures with known similarity relationships.

Acceptance criteria:

- Semantic pass stores vectors and emits semantic edges with score metadata.
- `vector_query` returns stable, explainable results.
- At least 5 of the reference semantic signals are implemented before marking this item mostly complete.
- All 11 signals are implemented before claiming full semantic parity.

### TODO 1.5 - Add Community Detection

Priority: `P2`

Current state:

- Reference includes Leiden and Louvain community detection.
- Rust version has no community detection pass.

Required work:

- Implement Leiden or start with Louvain as an incremental milestone.
- Store community IDs in node `properties_json` or a separate table.
- Expose community summary through `get_architecture` and/or a dedicated tool.

Acceptance criteria:

- A fixture graph produces deterministic community assignments.
- `get_architecture` reports community count and top communities.

### TODO 1.6 - Add Graph Buffer / Bulk Index Architecture

Priority: `P2`

Current state:

- Rust writes through `Store` directly after per-file extraction.
- Reference has a graph buffer layer for staging, merging, and bulk dumping into SQLite.

Required work:

- Design a Rust `GraphBuffer` or equivalent staging structure.
- Support merge, find-by-qn, find-by-label, edge lookup, vector storage, and flush to store.
- Use bulk transaction mode for large index writes.

Acceptance criteria:

- Indexing a medium fixture uses one bulk transaction per phase instead of many small writes.
- Store tests cover graph buffer flush and merge behavior.

### TODO 1.7 - Improve Store Parity and SQLite Behavior

Priority: `P1`

Current state:

- Rust store has basic tables and WAL-ish pragmas.
- Reference store includes readonly open, integrity checks, begin/commit/rollback, bulk mode, index drop/recreate, checkpoint, mmap, vector count/search, schema counts, BFS, and community algorithms.

Required work:

- Add explicit transaction APIs.
- Add bulk write mode with safe pragma changes.
- Add readonly store opening for query paths.
- Add integrity check.
- Add schema-count reporting.
- Add migration/version metadata.

Acceptance criteria:

- Store API has tests for transaction rollback and readonly query behavior.
- `index_repository` cannot leave a partially committed graph on failure.
- `get_graph_schema` includes table counts and supported compatibility notes.

### TODO 1.8 - Match File Discovery Rules

Priority: `P1`

Current state:

- Rust discovery skips common build/cache directories and extensions.
- Reference requires gitignore, `.cbmignore`, mode-specific filtering, path traversal safety, and broader language support.

Required work:

- Add `.cbmignore` support.
- Clarify fast/moderate/full skip rules.
- Add tests for gitignore plus `.cbmignore` interaction.
- Expand language detection toward the reference language list.
- Avoid indexing generated artifacts unless explicitly requested.

Acceptance criteria:

- Fixtures prove `.gitignore` and `.cbmignore` are both respected.
- Fast mode skips large/generated/non-code files.
- Full mode still avoids dangerous/binary files.

### TODO 1.9 - Improve HTTP UI Parity

Priority: `P2`

Current state:

- Rust version embeds a lightweight `ui.html`.
- Reference has a React/Three.js graph-ui with API client, graph scene, tabs, filters, stats, node details, labels, tooltips, and project cards.

Required work:

- Decide whether to keep lightweight HTML or port the React UI.
- If keeping lightweight HTML, document it as a deliberate reduced UI.
- Add filters, node details, project selector, graph stats, edge type filtering, and search.

Acceptance criteria:

- UI can list projects, render graph, filter labels/edge types, inspect a node, and show architecture stats.
- UI behavior is smoke-tested in browser or with HTTP API tests.

### TODO 1.10 - Add Packaging Parity

Priority: `P2`

Current state:

- Rust version has scripts plus Homebrew, Scoop, Winget, GitHub workflows.
- Reference includes Go wrapper, Python/PyPI wrapper, npm, Chocolatey, AUR, Glama, and safe download/checksum behavior.

Required work:

- Add or explicitly defer these packaging channels:
  - Go wrapper
  - Python/PyPI wrapper
  - npm wrapper
  - Chocolatey package
  - AUR package
  - Glama metadata
- Add checksum verification and safe archive extraction for wrapper installers.
- Replace placeholder release hashes in package manifests.

Acceptance criteria:

- Each supported package path can install and run `cbrlm --version`.
- Wrapper installers verify checksums and block path traversal.
- Unsupported packaging channels are listed in a roadmap with status.

### TODO 1.11 - Add Foundation / Runtime Infrastructure Parity

Priority: `P2`

Current state:

- Rust uses normal Rust allocation, standard types, rayon, rusqlite, and tracing.
- Reference includes mimalloc, memory budget, structured diagnostics, profiling, platform abstraction, arena/slab allocators, string interning, and watchdog behavior.

Required work:

- Decide which C foundation pieces should map to idiomatic Rust and which should be intentionally omitted.
- Add optional allocator support if needed.
- Add memory budget checks for large scans/indexing.
- Add diagnostics/profile flags compatible with reference env vars.
- Add graceful shutdown/cancellation support across pipeline, watcher, HTTP, and MCP.

Acceptance criteria:

- `CBM_PROFILE` / `CBRLM_PROFILE` or equivalent produces useful timing data.
- Large indexing can be cancelled gracefully.
- Memory budget behavior is documented and tested at a small synthetic scale.

### TODO 1.12 - Add README and Parity Matrix

Priority: `P1`

Current state:

- Rust repo currently lacks a README.
- Other agents need a clear contract to avoid guessing what "Rust rewrite" means.

Required work:

- Add `README.md`.
- Add a parity matrix mapping reference features to Rust status:
  - Done
  - Partial
  - Not started
  - Intentionally omitted
- Link this TODO file from README.

Acceptance criteria:

- A new agent can understand current status without reading chat history.
- README includes build, test, index, MCP, install, UI, and release instructions.

## Section 2 - Key Blockers

### TODO 2.1 - Fix Qualified Name Collisions

Priority: `P0`

Problem:

- Current qualified names are built as `file_path::name`.
- This collides for symbols with the same name in the same file, such as:
  - `struct McpServer`
  - `impl McpServer`
  - `impl Default for McpServer`
  - methods named `new`, `run`, `default`, etc.
- Since `qualified_name` is part of the primary key, later symbols can overwrite earlier symbols.
- This corrupts search, snippets, architecture summaries, and call graph edges.

Required work:

- Design a stable unique QN format.
- Include enough disambiguation:
  - symbol kind
  - owner/type/module path
  - method receiver where available
  - line span or generated stable suffix when needed
- Preserve user-friendly display names separately from unique IDs.
- Add migration or reindex behavior for old DBs.

Acceptance criteria:

- `struct`, `impl`, and methods with the same display name can coexist.
- `get_code_snippet` for each QN returns the correct source span.
- Tests include same-file duplicate names and repeated method names.

### TODO 2.2 - Replace Regex-Only Call Graph With AST-Aware Resolution

Priority: `P0`

Problem:

- Current CALLS extraction scans function bodies with regex patterns like `name(` and `.name(`.
- It links every matching callee name in the whole project registry.
- This creates false edges when multiple symbols share a method name, such as `spawn`, `run`, `stop`, or `new`.

Required work:

- Extract call expressions from tree-sitter AST where grammar support exists.
- Track method receivers and lexical context.
- Resolve local functions before cross-file/global symbols.
- Use imports/modules to constrain candidate resolution.
- Store confidence in edge `properties_json`.
- Keep a fallback regex pass only for unsupported languages, marked low confidence.

Acceptance criteria:

- Fixture with `HttpServer::spawn` and `Watcher::spawn` resolves only the correct target when receiver is known.
- Common control keywords and macros do not become graph edges.
- CALLS precision tests prevent broad "same name, all targets" behavior from returning.

### TODO 2.3 - Make Tests Deterministic Under Parallel Execution

Priority: `P0`

Problem:

- `cargo test` currently fails under default parallel execution.
- `RUST_TEST_THREADS=1 cargo test` passes.
- Failures involve global environment variables, shared cache paths, fixed project names, and Windows SQLite file locking.

Required work:

- Add a shared test isolation helper.
- Use unique project names per test.
- Scope environment variable changes and restore them after tests.
- Avoid deleting SQLite DB files while open connections may still exist.
- Serialize tests that must mutate process-wide env.

Acceptance criteria:

- `cargo test` passes without `RUST_TEST_THREADS=1`.
- Tests pass repeatedly on Windows.
- CI uses the default test runner and remains green.

### TODO 2.4 - Make Clippy Clean

Priority: `P1`

Problem:

- `cargo clippy --all-targets -- -D warnings` fails.
- Current lint failures include:
  - identical `if` branches in CLI output
  - collapsible if/match
  - derivable default impl
  - needless range loop
  - needless borrow
  - manual `is_multiple_of`
  - cloned ref to slice ref
  - type complexity
  - item after test module
  - useless vec in tests

Required work:

- Fix lints directly where simple.
- For real design issues, add small types or helper structs instead of suppressing.
- Avoid blanket `allow` unless there is a clear reason.

Acceptance criteria:

- `cargo clippy --all-targets -- -D warnings` passes.
- CI runs clippy with warnings denied.

### TODO 2.5 - Fix `search_graph` Pattern Semantics

Priority: `P1`

Problem:

- Reference docs and agent examples use regex-like patterns such as `.*OrderHandler.*`.
- Rust implementation uses glob matching, so `.*run_cli.*` does not match `run_cli`; `*run_cli*` does.
- This makes agents think the graph is empty or broken.

Required work:

- Prefer regex support for `name_pattern` and `qn_pattern`.
- Decide whether `file_pattern` remains glob or also supports regex.
- Validate invalid regex errors cleanly.
- Update schemas and docs.

Acceptance criteria:

- Regex examples from AGENTS-style instructions work.
- Glob behavior is either removed, renamed, or documented clearly.
- Tests cover both successful and invalid patterns.

### TODO 2.6 - Harden `query_graph`

Priority: `P1`

Problem:

- Current query guard blocks any SELECT containing words like `UPDATE`, even inside string literals.
- Example: `SELECT 'UPDATE' AS word` is rejected.
- String scanning is not a reliable readonly SQL policy.

Required work:

- Use SQLite prepare/readonly APIs if available through rusqlite/libsqlite.
- Or use a stricter parser/authorizer strategy.
- Open query connections readonly when possible.
- Preserve the "SELECT only" contract.

Acceptance criteria:

- Valid readonly SELECT statements containing words like `UPDATE` in strings are allowed.
- Mutating statements are blocked.
- Multiple-statement attempts are blocked.
- Tests cover allowed and rejected query cases.

### TODO 2.7 - Fix `rlm_scan` Resource Safety

Priority: `P1`

Problem:

- `rlm_scan` uses raw recursive `WalkDir`.
- It does not reuse repo ignore rules.
- It can scan `.git`, `target`, `dist`, large generated folders, or binary-ish text files.
- It reads file contents into memory without a practical session budget.

Required work:

- Use `ignore::WalkBuilder` and shared skip rules.
- Add max file size and max total bytes.
- Add optional include/exclude filters.
- Return skipped file counts and reasons.
- Avoid storing empty chunks for unreadable/binary files.

Acceptance criteria:

- Scanning a Rust repo does not include `target` or `.git`.
- Large files are skipped with a clear summary.
- `rlm_scan` cannot accidentally load an entire build cache into memory.

### TODO 2.8 - Fix Default Project Name Collisions

Priority: `P1`

Problem:

- Default project name is based mostly on drive letter plus repo basename.
- Different repos with the same folder name can collide, for example:
  - `D:\foo\app`
  - `D:\bar\app`
- Collisions overwrite cache DBs and project metadata.

Required work:

- Derive project name from the full canonical path.
- Use a readable slug plus short hash.
- Preserve `cbrlm+` prefix behavior.
- Add compatibility handling for old names if needed.

Acceptance criteria:

- Two repos with the same basename produce different project names.
- Existing explicit `project` parameter still works.
- Tests cover Windows drive paths and non-Windows paths.

### TODO 2.9 - Fix Watcher Reindex Loop and Backoff

Priority: `P1`

Problem:

- Watcher sleeps on a fixed base interval.
- Per-project `interval_ms` exists but is not used for scheduling.
- Any dirty repo triggers reindex every poll unless the dirty state changes.

Required work:

- Track last indexed dirty file set or last status signature.
- Use adaptive backoff per project.
- Avoid repeated reindex when the dirty set has not changed.
- Add watcher tests around dirty repo behavior.

Acceptance criteria:

- A dirty repo is indexed once, then not repeatedly reindexed until the dirty set or HEAD changes.
- Backoff state is observable in logs or status.

### TODO 2.10 - Fix CLI Output Contract

Priority: `P2`

Problem:

- CLI exposes `--json`, but both branches print pretty JSON.
- Reference CLI distinguishes JSON output and progress/log output.

Required work:

- Decide default CLI format:
  - human-readable summary by default, raw JSON with `--json`, or
  - always JSON and remove the flag.
- Add `--progress` if matching reference CLI.
- Ensure logs go to stderr and machine-readable output goes to stdout.

Acceptance criteria:

- CLI output behavior is documented.
- Tests cover `--json` and default output.
- Tool output can be piped into JSON consumers reliably.

### TODO 2.11 - Add Signal Handling and Graceful Shutdown

Priority: `P2`

Problem:

- Reference main handles SIGTERM/SIGINT, stops watcher/HTTP, cancels pipeline, and closes stdin.
- Rust version has basic stdio loop and thread stop calls, but no equivalent full shutdown/cancel contract.

Required work:

- Add cancellation token shared by MCP, pipeline, watcher, and HTTP server.
- Handle Ctrl+C/SIGTERM where supported.
- Ensure long indexing can stop cleanly.

Acceptance criteria:

- Manual Ctrl+C during indexing exits without corrupting DB.
- Tests or smoke scripts cover graceful shutdown where feasible.

## Section 3 - Suggested Execution Order

1. Fix test isolation so `cargo test` is trustworthy.
2. Fix qualified name collisions.
3. Fix CALLS precision enough to stop false graph edges.
4. Align `search_graph` semantics and response contract.
5. Harden `query_graph`.
6. Fix `rlm_scan` resource safety.
7. Add README plus parity matrix.
8. Expand graph passes: CONTAINS, IMPORTS, then richer language-specific edges.
9. Expand semantic system and vector search.
10. Improve watcher scheduling/backoff.
11. Improve HTTP UI and packaging parity.
12. Add foundation/runtime parity features where Rust equivalents are useful.

## Section 4 - Quality Gates

Before marking any major milestone complete, run:

```powershell
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
```

Recommended smoke checks:

```powershell
cargo run -- cli index_repository --json '{"repo_path":".","project":"smoke-review","mode":"fast","persistence":false}'
cargo run -- cli search_graph --json '{"project":"smoke-review","query":"run_cli","limit":3}'
cargo run -- cli get_architecture --json '{"project":"smoke-review"}'
```

Do not claim full Rust rewrite parity until:

- Graph correctness issues are fixed.
- Multiple edge types are actually emitted.
- Search contract matches the reference docs.
- Default `cargo test` and clippy are green.
- README/parity matrix reflects reality.

## Section 5 - Post-MVP Parity Execution Order

1. Add `.cbmignore` support and shared walk builder (TODO 1.8).
2. Store readonly open, integrity check, schema version metadata (TODO 1.7 slice).
3. HTTP route pass emitting `HTTP_ROUTE` edges (TODO 1.2 slice).
4. Implement remaining semantic signals — all 11 reference signals (TODO 1.4).
5. Community detection pass with architecture summary (TODO 1.5).
6. HTTP UI search, node details panel, architecture stats wiring (TODO 1.9).
7. Packaging checksum verification and deferred-channels documentation (TODO 1.10).
8. Runtime profiling (`CBRLM_PROFILE`) and memory budget (`CBRLM_MEMORY_BUDGET_MB`) (TODO 1.11).
9. AST-aware CALLS for Rust with regex fallback (TODO 2.2 slice).

Quality gates from Section 4 still apply after each slice.

## Section 6 - Review Notes After Composer 2.5 Implementation

**Status: complete (#all)** — items 6.1–6.8 done; see `PARITY_MATRIX.md` Section 6 table.

Review date: 2026-06-12

Reviewer: Codex

Context:

- Composer 2.5 implemented a large part of the original TODO list.
- Section 6 hardening applied (RLM session persistence, CALLS fixtures, release smoke, `--quiet`, docs).
- Local validation passes:
  - `cargo test` (81 cases)
  - `cargo clippy --all-targets -- -D warnings`
  - `cargo build --release`
  - `.\scripts\smoke-quality-gates.ps1 -SkipBuild`
  - `.\scripts\smoke-release-artifact.ps1 -SkipBuild`
- Smoke checks confirm `search_graph`, `query_graph` edge diversity, semantic edges, and `get_architecture`.

MVP rewrite is complete (Sections 3–6). Full reference parity remains in `PARITY_MATRIX.md` backlog (Leiden, HTTP_CALLS, bulk tx, multi-lang AST CALLS, …).

### TODO 6.1 - Fix CLI `rlm_scan` Session Usability

Status: **Done** (2026-06-12) — disk persistence under `CBRLM_CACHE_DIR/rlm-sessions`, TTL/size limits, integration test.

Priority: `P1`

Observation:

- `rlm_scan` works inside one running process, which is appropriate for MCP server sessions.
- In CLI mode, `cbrlm cli rlm_scan ...` returns a `session_id`, but a later `cbrlm cli rlm_chunk ...` cannot read it because each CLI call creates a new in-memory `RlmEngine`.
- Repro:
  - Run `cbrlm cli rlm_scan --json '{"path":"."}'`.
  - Copy the returned `session_id`.
  - Run `cbrlm cli rlm_chunk --json '{"session_id":"<id>","limit":1}'`.
  - Result: `session not found`.

Recommended work:

- Choose one behavior:
  - Persist RLM scan sessions to a temp/cache store so CLI chunks work across invocations.
  - Or mark `rlm_scan`/`rlm_chunk` as MCP-session-only and make CLI output say that clearly.
- If persisting, add TTL cleanup and size limits.

Acceptance criteria:

- CLI documentation and actual behavior match.
- A smoke test proves either persistent CLI scan/chunk works, or the CLI fails early with a clear "MCP-session-only" message.

### TODO 6.2 - Keep README and Parity Matrix Numerically Accurate

Status: **Done** (2026-06-12) — no hard-coded test counts; CI is source of truth.

Priority: `P2`

Observation:

- `PARITY_MATRIX.md` says `cargo test` is "65 tests".
- Current local run executed 55 lib tests, 2 hook integration tests, and 15 integration tests, for 72 total test cases.
- These numbers will keep changing, so fixed counts drift quickly.

Recommended work:

- Avoid hard-coded test counts in docs unless generated by a script.
- Prefer wording like "default cargo test is green" or link to CI output.
- If counts are important, add a script that updates the count automatically.

Acceptance criteria:

- README and parity matrix do not contain stale test-count claims.
- CI remains the source of truth for gate status.

### TODO 6.3 - Split "Done" From "Heuristic / Partial" More Carefully

Status: **Done** (2026-06-12) — MVP vs full parity model, heuristic labels, full parity blockers in `PARITY_MATRIX.md`.

Priority: `P1`

Observation:

- The implementation now emits `CONTAINS`, `IMPORTS`, `CALLS`, `IMPLEMENTS`, and semantic edges.
- Some parity items are still heuristic:
  - Import edges are regex-based.
  - Inheritance/implements/decorates are partial by language.
  - HTTP routes cover selected Python/JS/Rust patterns.
  - Community detection is connected-components MVP, not Leiden/Louvain.
  - Rust AST-aware CALLS exists, but multi-language AST-aware CALLS is still future work.
- `PARITY_MATRIX.md` does call some of these "Partial", but a few summary lines such as "Sections 3-5 are complete" can sound stronger than the actual state.

Recommended work:

- Add a "MVP complete vs full parity" status distinction.
- Mark connected-components community detection as "MVP", not full community parity.
- Mark HTTP route extraction as framework-limited.
- Mark all regex-based passes as heuristic.

Acceptance criteria:

- A new agent cannot mistake the current state for full equivalence with the reference C implementation.
- `PARITY_MATRIX.md` has a clear "Full parity blockers" subsection.

### TODO 6.4 - Add Release-Artifact Verification Beyond Local Build

Status: **Done** (2026-06-12) — `scripts/smoke-release-artifact.ps1`; wired in CI and release workflows.

Priority: `P2`

Observation:

- `cargo build --release` passes.
- Packaging scripts and manifests exist.
- The current smoke gate does not verify the final archives or installer scripts end-to-end.

Recommended work:

- Add a release smoke command that:
  - Builds release artifacts.
  - Extracts the produced archive into a temp directory.
  - Runs `cbrlm --version`.
  - Runs one CLI smoke command from the extracted binary.
  - Verifies checksum files match the archive.
- On Windows, also smoke `packaging/windows/install.ps1` against a local artifact if possible.

Acceptance criteria:

- `scripts/smoke-release-artifact.ps1` or equivalent exists.
- Release workflow runs the artifact smoke before publishing.

### TODO 6.5 - Add Cross-Language CALLS Precision Fixtures

Status: **Done** (2026-06-12) — `tests/calls_precision_test.rs` (Py/JS/Go/Java local calls, ambiguity, regex metadata).

Priority: `P1`

Observation:

- Rust AST-aware CALLS and ambiguity tests are now present.
- Reference parity still needs stronger confidence across Python, JS/TS, Go, Java, C, and C++.
- Regex fallback is useful, but it should be measured and constrained.

Recommended work:

- Add fixture suites per language with same-name functions/methods in different scopes.
- Assert that obvious local calls resolve.
- Assert ambiguous calls do not produce many false positive edges.
- Add edge confidence metadata checks for fallback-resolved calls.

Acceptance criteria:

- Each supported major language has at least one CALLS precision fixture.
- False-positive regression cases are locked in tests.

### TODO 6.6 - Extend Smoke Gates To Assert Edge-Type Diversity From Query Layer

Status: **Done** (2026-06-12) — `query_graph` edge-type GROUP BY in smoke-quality-gates scripts.

Priority: `P2`

Observation:

- `get_architecture` smoke checks for `CALLS`, `CONTAINS`, and `IMPORTS`.
- A direct `query_graph` check over `edges` gives better coverage of the storage/query layer.

Recommended work:

- Add a smoke query:
  - `SELECT edge_type, COUNT(*) AS count FROM edges GROUP BY edge_type ORDER BY count DESC`
- Assert at least `CALLS`, `CONTAINS`, and `IMPORTS` are present after indexing this repo.
- When semantic smoke is enabled, assert `SIMILAR_TO` or `SEMANTICALLY_RELATED`.

Acceptance criteria:

- Smoke gates catch regressions where architecture summaries still work but edge storage/query output is wrong.

### TODO 6.7 - Decide Whether CLI Logs Should Be Quiet Under `--json`

Status: **Done** (2026-06-12) — `--quiet` defers tracing; README documents stdout JSON / stderr logs; `json_output_is_parseable` test.

Priority: `P2`

Observation:

- Machine-readable JSON is printed, while tracing/progress logs may still appear on stderr.
- This is acceptable for normal CLI conventions, but scripts often capture combined streams.

Recommended work:

- Document that `--json` guarantees stdout JSON only, while logs go to stderr.
- Add a test or script assertion that stdout is valid JSON when `--json` is used.
- Consider a `--quiet` option for scripts that want no diagnostics.

Acceptance criteria:

- `cbrlm cli ... --json > out.json` always creates parseable JSON.
- Combined-stream behavior is documented for PowerShell users.

### TODO 6.8 - Add Full-Parity Backlog Items Explicitly

Status: **Done** (2026-06-12) — Full parity backlog + blockers in `PARITY_MATRIX.md`.

Priority: `P2`

Observation:

- The roadmap mentions Leiden/Louvain, `HTTP_CALLS`, store bulk transactions, multi-language AST CALLS, and FoundationDB omitted.
- These should remain visible as backlog items so future agents do not treat them as forgotten or done.

Recommended work:

- Add a dedicated "Full parity backlog" section in `PARITY_MATRIX.md`.
- Include:
  - Leiden/Louvain community detection.
  - `HTTP_CALLS` pass.
  - Store bulk transaction API and rollback tests.
  - Multi-language AST-aware CALLS.
  - Optional FoundationDB backend decision record.
  - Go/Python/npm/Chocolatey/AUR wrapper parity, if desired.

Acceptance criteria:

- Full-parity gaps are easy to find without reading all TODO history.

## Section 7 - Post-Composer 2.5 Review Notes

**Status: complete (#all)** — items 7.1–7.7 done; see `PARITY_MATRIX.md` Section 7 table.

Review date: 2026-06-12

Reviewer: Codex

Context:

- Composer 2.5 completed the Section 6 implementation pass.
- Local validation passed after reviewer formatting:
  - `cargo fmt --check`
  - `cargo test --all-targets`
  - `cargo clippy --all-targets -- -D warnings`
  - `cargo build --release`
  - `.\scripts\smoke-quality-gates.ps1 -SkipBuild`
  - `.\scripts\smoke-release-artifact.ps1 -SkipBuild`
- A real two-process CLI smoke also passed:
  - `cbrlm cli rlm_scan --json --quiet ...`
  - `cbrlm cli rlm_chunk --json --quiet ...`

The Rust MVP now behaves much more like a usable CBM replacement. The remaining items below are not blockers for the current MVP, but they are important before claiming stronger full-reference parity or release-grade automation.

### TODO 7.1 - Make Release Artifact Smoke Matrix-Aware

Status: **Done** (2026-06-12) — `-ArtifactName`, `-BinaryPath`, `-ArchivePath`, `-SkipPackage`; release.yml uses matrix archive.

Priority: `P1`

Observation:

- `scripts/smoke-release-artifact.ps1` works locally after `cargo build --release`.
- In `.github/workflows/release.yml`, the Windows matrix builds `target/${{ matrix.target }}/release/cbrlm.exe`.
- The smoke script currently assumes `target\release\cbrlm.exe` and hard-codes `cbrlm-windows-x64`.
- This can make the release workflow repackage or look for the wrong binary path when run inside the target matrix.

Recommended work:

- Add parameters such as:
  - `-ArtifactName`
  - `-BinaryPath`
  - `-ArchivePath`
  - `-SkipPackage`
- In the release workflow, smoke the archive that was just produced by the packaging step.
- Keep the no-argument local mode for developer convenience.

Acceptance criteria:

- Windows release workflow validates `dist/${{ matrix.artifact }}.zip` without depending on `target\release\cbrlm.exe`.
- Local `.\scripts\smoke-release-artifact.ps1 -SkipBuild` still works.

### TODO 7.2 - Add `cargo fmt --check` To CI And Smoke Gates

Status: **Done** (2026-06-12) — CI, smoke-quality-gates, README.

Priority: `P1`

Observation:

- The implementation passed tests and clippy, but `cargo fmt --check` failed before reviewer formatting.
- Current CI gates do not appear to enforce formatting.
- Without this gate, future generated patches can drift into large formatting-only churn.

Recommended work:

- Add a CI step before clippy:
  - `cargo fmt --check`
- Optionally add it to `scripts/smoke-quality-gates.ps1` / `.sh`.
- Document formatting as a required pre-commit gate in `README.md`.

Acceptance criteria:

- CI fails on unformatted Rust code.
- Section quality gates list `cargo fmt --check`.

### TODO 7.3 - Add Process-Level CLI JSON Tests

Status: **Done** (2026-06-12) — `tests/cli_process_test.rs` with `assert_cmd`.

Priority: `P2`

Observation:

- The new `json_output_is_parseable` unit test verifies serialized handler output, but it does not execute the compiled CLI.
- Smoke scripts cover process behavior, but a focused test should assert stdout/stderr behavior across the actual binary invocation.

Recommended work:

- Add an integration test using `assert_cmd` or equivalent.
- Verify:
  - `cbrlm cli list_projects --json --quiet` writes parseable JSON to stdout.
  - stderr is empty or explicitly documented.
  - normal `--json` keeps stdout parseable even if diagnostics go to stderr.

Acceptance criteria:

- A regression that prints logs to stdout under `--json` fails tests.
- PowerShell and Unix script examples match tested behavior.

### TODO 7.4 - Harden RLM Session Persistence Writes

Status: **Done** (2026-06-12) — atomic temp+rename, corrupt file removal, persistence tests.

Priority: `P2`

Observation:

- Persisted RLM sessions now survive across separate CLI invocations.
- `persist_session` writes JSON directly to the final path.
- A crash, interrupted write, or concurrent writer could leave a partial JSON file.
- `load_persisted_sessions` skips unreadable/corrupt files, but does not clean them up or report counts.

Recommended work:

- Write sessions atomically:
  - write to a temp file in the same directory
  - flush
  - rename to `<session_id>.json`
- Consider a lightweight lock or unique temp filenames for concurrent scans.
- Track skipped corrupt/expired sessions in debug logs or metadata.
- Add tests for corrupt session files and concurrent-ish writes.

Acceptance criteria:

- Interrupted or corrupt session files do not break future scans.
- Corrupt files are either quarantined, removed, or counted.
- Session persistence remains Windows-safe.

### TODO 7.5 - Deepen CALLS Precision Fixtures Beyond Direct Resolver Tests

Status: **Done** (2026-06-12) — `tests/calls_pipeline_test.rs` full-pipeline fixtures.

Priority: `P1`

Observation:

- `tests/calls_precision_test.rs` adds useful language coverage.
- Most new checks call `resolve_calls_with_registry` directly with hand-built symbols.
- This catches resolver behavior, but not extraction + indexing + store/query behavior for realistic files.
- Method calls, imports/aliases, classes, modules, overload-like patterns, and nested scopes are still shallowly covered.

Recommended work:

- Add fixture repositories per language and run the full pipeline.
- Assert graph output through `search_graph`, `trace_path`, or direct store queries.
- Include negative cases:
  - same function name in different modules
  - method calls vs free functions
  - imported aliases
  - nested functions/classes
  - common framework callback names

Acceptance criteria:

- Full pipeline tests catch false-positive CALLS edges, not only resolver-unit regressions.
- `properties_json` clearly identifies `method=rust_ast`, `method=regex`, or future language AST methods.

### TODO 7.6 - Add Installer And MCP Protocol Smoke From Release Artifact

Status: **Done** (2026-06-12) — install dry-run + MCP initialize/tools/list in smoke-release-artifact.ps1.

Priority: `P2`

Observation:

- The release smoke extracts the binary, runs `--version`, and indexes a small project.
- It does not validate install scripts from the release artifact path.
- It also does not run a minimal MCP JSON-RPC initialize/list-tools round trip from the extracted binary.

Recommended work:

- Add release smoke steps for:
  - `scripts\install.ps1 -DryRun` or `packaging\windows\install.ps1` against the extracted binary.
  - MCP stdio `initialize` and `tools/list` using the extracted binary.
- Keep side effects isolated to a temp `CBRLM_CACHE_DIR` and temp config HOME when possible.

Acceptance criteria:

- A packaged binary is proven to work as CLI and MCP server before release.
- Installer dry-run validates paths and config snippets without touching the real user config.

### TODO 7.7 - Clarify "MVP Complete" vs "Rust Replica" In Project Language

Status: **Done** (2026-06-12) — consistent "Rust MVP rewrite complete" wording in README and PARITY_MATRIX.

Priority: `P2`

Observation:

- README and parity matrix now state MVP completion and full parity gaps.
- The phrase "Rust rewrite complete" can still be misread as "complete reference replica".
- Current implementation intentionally keeps SQLite canonical and omits FoundationDB.

Recommended work:

- Use consistent wording:
  - "Rust MVP rewrite complete"
  - "Full reference parity backlog remains"
  - "FoundationDB omitted by design"
- Avoid saying "complete rewrite" without the MVP qualifier in docs, release notes, and future task lists.

Acceptance criteria:

- A new agent can tell which claims are release-ready and which are only MVP-ready without reading all historical TODO sections.
