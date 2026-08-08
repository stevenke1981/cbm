# CBM + cbm-mcp integration and CPU-load fix

This branch folds the operational parts of `stevenke1981/cbm-mcp` into the main
`cbm` binary instead of shipping a second copy of the same indexer.

## What is integrated

- Official Rust MCP SDK transport (`rmcp 1.7.0`) and stdio lifecycle.
- The existing `cbm` graph tools, background indexing, semantic search, 14
  tree-sitter language families, and integrated RLM tools.
- One server process and one installed `cbm` executable.
- MCP tool schemas are adapted from the existing `cbm` definitions, so current
  clients keep the same argument names while gaining the official SDK transport.
- The `codebase-memory` agent skill is stored under
  `skills/codebase-memory/SKILL.md`.

The combined server exposes 23 tools: 15 graph/index tools and 8 `rlm_*` tools.

## CPU root causes

The former implementation had three independent sources of avoidable load:

1. Every indexed repository ran `git status` every five seconds even after it
   had been clean for hours.
2. Every watcher tick re-read the full project list from SQLite.
3. Every `tools/call` request created a new operating-system thread with no
   concurrency limit.

The old Git status path also launched two Git processes per poll: one for
`rev-parse HEAD` and one for `status --porcelain`.

## New scheduling behavior

Idle repository polling now uses exponential backoff:

```text
5s -> 10s -> 20s -> 40s -> 60s
```

A detected change or successful incremental index resets that repository to
five seconds. The project registry is refreshed once per minute instead of once
per poll. The minimum watcher wake interval is one second, so contention cannot
fall into the former 250 ms retry loop.

Git status and HEAD are collected in one process with
`git status --porcelain=v2 --branch`.

## Tool concurrency

Tool calls run through Tokio's blocking pool and a semaphore. The default
parallelism is:

```text
min(logical CPU count, 4), with a minimum of 1
```

Override it when required:

```powershell
$env:CBM_MAX_TOOL_WORKERS = "2"
cbm
```

```bash
CBM_MAX_TOOL_WORKERS=2 cbm
```

The watcher remains enabled by default. Disable it for strictly on-demand
indexing:

```powershell
$env:CBM_WATCHER = "0"
cbm
```

```bash
CBM_WATCHER=0 cbm
```

## Validation

The branch includes:

- unit tests for idle backoff and the one-minute cap;
- parser tests for Git porcelain v2 output;
- an official `rmcp` client/server duplex test;
- checks that graph and RLM tools are both advertised.

Run the full repository gates:

```bash
cargo fmt --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo build --release
```

For an idle-load comparison on Linux:

```bash
CBM_WATCHER=1 cargo run --release &
pid=$!
pidstat -p "$pid" 5 12
kill "$pid"
```

For Windows PowerShell:

```powershell
$env:CBM_WATCHER = "1"
$p = Start-Process .\target\release\cbm.exe -PassThru
Get-Counter '\Process(cbm)\% Processor Time' -SampleInterval 5 -MaxSamples 12
Stop-Process -Id $p.Id
```

Use several indexed repositories when comparing before and after; the old
poller cost scaled with the number of projects.
