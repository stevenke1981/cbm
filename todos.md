# CBM — DeusData 功能對齊 TODO

參考：[DeusData/codebase-memory-mcp](https://github.com/DeusData/codebase-memory-mcp) v0.8.1
上次更新：2026-07-22

---

## 已完成（效能改善）

| # | 項目 | 狀態 |
|---|------|------|
| P0.1 | search_graph SQL WHERE 下推 | ✅ Done |
| P0.2 | search_code FTS5 | ✅ Done |
| P1.1 | MCP 非同步（背景索引 + tools/call 執行緒） | ✅ Done |
| P1.2 | trace_path N+1 → find_symbols_batch | ✅ Done |
| P1.3 | vector_search N+1 → find_symbols_batch | ✅ Done |
| P1.4 | SQLite pragmas（64MB cache, mmap 256MB） | ✅ Done |
| P2.1 | get_architecture SQL COUNT + community meta | ✅ Done |
| P2.2 | StorePool 連線池 | ✅ Done |
| P2.3 | WAL 自動 TRUNCATE checkpoint | ✅ Done |
| P2.4 | 語意 O(n²) quantized cosine prefilter | ✅ Done |

---

## P0 — 核心查詢能力對齊

### TODO F1 — Cypher 查詢支援（query_graph）

**DeusData：** `query_graph` 支援 openCypher 讀取子集：MATCH, WHERE, RETURN, ORDER BY, SKIP, LIMIT, DISTINCT, WITH, UNION, OPTIONAL MATCH, 變長路徑 `[*1..3]`, 聚合函數, EXISTS 子查詢。

**CBM 現況：** `query_graph` 僅支援 SQL SELECT。

**實作方式：**
- 新增 `src/cypher/` 模組：lexer → parser → planner → executor
- 支援核心子集：`MATCH (n:Label)-[:REL]->(m) WHERE ... RETURN ... ORDER BY ... LIMIT ...`
- 將 Cypher 翻譯成 SQL 查詢（SQLite 後端）
- 保留 SQL SELECT 作為 fallback（`query_graph` 自動偵測）
- 支援函數：labels(), type(), count(), collect(), toLower(), toUpper(), size()
- 支援 WHERE 運算子：=, <>, <, >, AND, OR, NOT, IN, CONTAINS, STARTS WITH, IS NULL, =~ (regex)
- 支援 EXISTS { (n)-[:TYPE]->() } 用於 dead code 偵測

**驗收：**
- `MATCH (f:Function)-[:CALLS]->(g) WHERE f.name = 'main' RETURN g.name` 正確回傳
- `MATCH (f:Function) WHERE NOT EXISTS { (f)<-[:CALLS]-() } RETURN f.name` 找到 dead code
- 不支援的語法回傳明確錯誤訊息

---

### TODO F2 — Dead Code 偵測

**DeusData：** 找到零 caller 的函數（排除 entry points），透過 Cypher `WHERE NOT EXISTS { (f)<-[:CALLS]-() }` 或 `get_architecture` 的 hotspots。

**CBM 現況：** 無。

**實作方式：**
- 在 `get_architecture` 加入 `dead_code` 欄位
- 找到所有 label=Function 且無 inbound CALLS 的 symbol
- 排除 entry points（main, handler, route handler, test）
- 或在 Cypher 支援後透過 EXISTS 子查詢實現

**驗收：**
- `get_architecture` 回傳 `dead_code` 列表
- 已知 fixture 中的 unused function 被正確識別

---

### TODO F3 — check_index_coverage 工具

**DeusData：** 檢查特定路徑/檔案是否被索引，回傳覆蓋率和遺漏的檔案。

**CBM 現況：** 無。

**實作方式：**
- 新增 MCP 工具 `check_index_coverage`
- 參數：`project`, `paths` (array of file paths)
- 回傳：每個路徑的索引狀態（indexed/missing/partial）、覆蓋率百分比
- 檢查 files 表中是否有對應記錄

**驗收：**
- 已索引檔案回傳 `indexed: true`
- 未索引檔案回傳 `indexed: false` 和原因

---

## P1 — 圖譜豐富度對齊

### TODO F4 — 更多 Edge Types

**DeusData 額外 edge types：**
- `DEFINES` / `DEFINES_METHOD`（目前 CBM 用 CONTAINS）
- `HANDLES`（route handler 關聯）
- `USAGE` / `USES_TYPE`（型別引用）
- `CONFIGURES`（設定關聯）
- `WRITES`（寫入關聯）
- `MEMBER_OF`（社群成員）
- `TESTS`（測試關聯）
- `FILE_CHANGES_WITH`（git co-change）
- `EMITS` / `LISTENS_ON`（channel 事件）
- `DATA_FLOWS`（資料流）
- `ASYNC_CALLS`（非同步呼叫）

**CBM 現況：** CONTAINS, IMPORTS, CALLS, INHERITS, IMPLEMENTS, DECORATES, HTTP_ROUTE, HTTP_CALLS, SIMILAR_TO, SEMANTICALLY_RELATED, RUNTIME_TRACE

**實作優先序：**
1. `DEFINES` / `DEFINES_METHOD` — 區分 file→class 和 class→method
2. `TESTS` — 偵測 test 函數與被测函數的關聯
3. `ASYNC_CALLS` — 偵測 async/await 呼叫
4. `EMITS` / `LISTENS_ON` — Socket.IO / EventEmitter 偵測

---

### TODO F5 — gRPC / GraphQL / tRPC 服務偵測

**DeusData：** 偵測 protobuf 定義、GraphQL schema、tRPC router，建立跨服務 edge。

**CBM 現況：** 僅 HTTP route 偵測。

**實作方式：**
- 新增 `src/pipeline/services.rs`
- 偵測 `.proto` 檔案 → gRPC service/method 節點
- 偵測 GraphQL schema → Query/Mutation 節點
- 偵測 tRPC router → procedure 節點
- 建立 `HTTP_CALLS` 或新的 `RPC_CALLS` edge

---

### TODO F6 — Infrastructure-as-Code 索引

**DeusData：** Dockerfile、K8s manifest、Kustomize overlay 作為圖譜節點。

**CBM 現況：** 無。

**實作方式：**
- 偵測 Dockerfile → `Resource` 節點（image, stage）
- 偵測 K8s YAML → `Resource` 節點（Deployment, Service, etc.）
- 偵測 Kustomize → `Module` 節點 + IMPORTS edge
- 建立 cross-reference edge

---

### TODO F7 — 套件/模組 Manifest 解析

**DeusData：** 掃描 package.json, go.mod, Cargo.toml, pyproject.toml, composer.json, pubspec.yaml, pom.xml, build.gradle, mix.exs, *.gemspec 來解析 bare specifier。

**CBM 現況：** 僅 tsconfig paths alias 和 Python package root。

**實作方式：**
- 新增 `src/pipeline/manifests.rs`
- 解析各語言的 manifest 檔案
- 將 bare import（如 `@myorg/pkg`, `github.com/foo/bar`）對應到已知 module
- 改善 IMPORTS edge 的精確度

---

### TODO F8 — BM25 FTS + camelCase 分詞器

**DeusData：** SQLite FTS5 + 自訂 `cbm_camel_split` tokenizer（camelCase / snake_case 感知）。

**CBM 現況：** FTS5 預設 tokenizer。

**實作方式：**
- 在 FTS5 建立時使用自訂 tokenizer
- 或使用 `unicode61` tokenizer + 在查詢時展開 camelCase
- 改善搜尋精確度（如搜尋 `handler` 能找到 `RequestHandler`）

---

## P1 — 運作能力對齊

### TODO F9 — config 子命令

**DeusData：** `codebase-memory-mcp config set/list/reset` 管理設定（auto_index, auto_index_limit, auto_watch）。

**CBM 現況：** 僅環境變數。

**實作方式：**
- 新增 `cbm config set <key> <value>` / `cbm config list` / `cbm config reset <key>`
- 設定存在 `CBM_CACHE_DIR/config.json`
- 支援鍵：`auto_index`, `auto_index_limit`, `auto_watch`

---

### TODO F10 — 自動索引（Auto-Index on Session Start）

**DeusData：** MCP session 開始時自動索引新專案。

**CBM 現況：** 無。

**實作方式：**
- 在 MCP `initialize` 時檢查 `auto_index` 設定
- 如果啟用且專案未索引，自動觸發背景索引
- 使用 IndexSupervisor 非阻塞執行

---

### TODO F11 — CBM_ALLOWED_ROOT 安全限制

**DeusData：** 限制 `index_repository` 只能索引指定目錄下的路徑。

**CBM 現況：** 無。

**實作方式：**
- 在 `index_repository` 處理時檢查 `CBM_ALLOWED_ROOT` 環境變數
- 解析 `repo_path` 的絕對路徑（處理 symlink 和 `..`）
- 如果路徑不在 allowed root 下，拒絕索引

---

### TODO F12 — 自訂檔案副檔名

**DeusData：** `.codebase-memory.json` 設定 `extra_extensions` 映射。

**CBM 現況：** 無。

**實作方式：**
- 在索引時讀取 `.codebase-memory.json`（專案根目錄）
- 讀取全域設定 `~/.config/cbm/config.json`
- 將額外副檔名映射到已知語言

---

### TODO F13 — 診斷日誌（CBM_DIAGNOSTICS）

**DeusData：** `CBM_DIAGNOSTICS=1` 啟用 NDJSON 軌跡日誌（rss, committed, fd, queries）。

**CBM 現況：** 無。

**實作方式：**
- 新增 `src/runtime/diagnostics.rs`
- 每 5 秒寫入一行 NDJSON（rss, query_count, uptime）
- 檔案路徑：`$TEMP/cbm-diagnostics-<pid>.ndjson`
- 超過 8MB 時輪替

---

### TODO F14 — 兩層 Artifact 匯出

**DeusData：** Best（zstd-9 + index strip + VACUUM INTO）和 Fast（zstd-3）兩層。

**CBM 現況：** 單層 zstd 匯出。

**實作方式：**
- 在 `index_repository` 完成後使用 Best 層級
- 在 watcher 增量更新後使用 Fast 層級
- 加入 VACUUM INTO 和 index strip

---

## P2 — 語言覆蓋對齊

### TODO F15 — 更多 Tree-sitter 語言

**DeusData：** 158 語言。
**CBM 現況：** 14 語言（Rust, Python, JS/TS, Go, Java, C, C++, Ruby, C#, PHP, Bash, Kotlin, Swift）。

**優先加入：**
1. Dart, Scala, Lua, Zig, Haskell, OCaml, Elixir, Erlang
2. Dockerfile, YAML, JSON, TOML, HTML, CSS, SQL, Markdown
3. GraphQL, Protobuf, HCL (Terraform)
4. 其餘按需加入

---

### TODO F16 — Hybrid LSP 型別解析

**DeusData：** 10+ 語言的型別感知呼叫解析（Python, TS/JS, PHP, C#, Go, C/C++, Java, Kotlin, Rust, Perl）。

**CBM 現況：** AST + FunctionRegistry 啟發式解析。

**實作方式（長期）：**
- 逐語言加入型別推斷
- 優先：Python（import + dotted path）、TypeScript（generics, JSX）
- 使用 per-file overlay + cross-file registry

---

## P2 — Agent 生態系對齊

### TODO F17 — 更多 Agent Surface

**DeusData：** 43 個 agent surface。
**CBM 現況：** ~10 個（Claude, Codex, Gemini, OpenCode, Zed, Aider, Antigravity, KiloCode, Kiro, Qwen）。

**優先加入：**
1. VS Code, Cursor, Windsurf, Augment
2. GitHub Copilot CLI, Amazon Q, Continue
3. Goose, Cline, Warp, Hermes
4. 其餘按需加入

---

### TODO F18 — Agent 定義（Scout / Verify / Auditor）

**DeusData：** 三層 agent 定義（Scout 快速發現、Verify 任務導向、Auditor 完整審計）。

**CBM 現況：** 無。

**實作方式：**
- 在 install 時建立三層 agent 定義
- 每層有不同的工具權限和指導原則
- Scout：search_graph, trace_path, get_code_snippet
- Verify：+ query_graph, get_architecture, detect_changes
- Auditor：+ check_index_coverage, manage_adr

---

### TODO F19 — Tool Profiles

**DeusData：** `--tool-profile scout` / `--tool-profile analysis` 限制 MCP 工具表面。

**CBM 現況：** 無。

**實作方式：**
- 新增 `--tool-profile` CLI 參數
- `scout`：7 個快速檢查工具
- `analysis`：11 個工具
- 在 `tools/list` 時根據 profile 過濾

---

## P3 — 發行與安全

### TODO F20 — 套件發行通路

**DeusData：** npm, PyPI, Homebrew, Scoop, Winget, Chocolatey, AUR, go install。
**CBM 現況：** 僅 GitHub Releases + 手動安裝。

**優先：**
1. npm wrapper（`npx cbm-mcp`）
2. Homebrew formula
3. Scoop manifest
4. 其餘按需

---

### TODO F21 — 安全發行管线

**DeusData：** SLSA Level 3, Sigstore cosign, VirusTotal, CodeQL。
**CBM 現況：** SHA-256 checksums。

**優先：**
1. Sigstore cosign 簽名
2. SLSA provenance
3. CodeQL SAST

---

### TODO F22 — 自動更新

**DeusData：** `codebase-memory-mcp update` 自動下載新版本。

**CBM 現況：** 無。

---

## 實作優先序

1. **F1** Cypher 查詢（核心差異化能力）
2. **F2** Dead code 偵測（agent 高價值）
3. **F3** check_index_coverage（agent 高價值）
4. **F11** CBM_ALLOWED_ROOT（安全）
5. **F9** config 子命令（易用性）
6. **F12** 自訂副檔名（易用性）
7. **F4** 更多 edge types（圖譜豐富度）
8. **F8** BM25 + camelCase 分詞（搜尋品質）
9. **F10** 自動索引（易用性）
10. **F13** 診斷日誌（維運）
11. **F15** 更多語言（覆蓋率）
12. **F17** 更多 agent surface（生態系）
13. 其餘 P2/P3 項目

## 驗證閘門

每個 TODO 完成後必須通過：

```powershell
cargo fmt --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo build --release
```