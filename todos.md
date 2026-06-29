# cbm MCP 效能改善 TODO

本文件記錄 cbm MCP 伺服器在使用時「有時很快、有時卡住」的根因與可採取的行動項目。

## 效能觀察摘要

| 情境 | 表現 | 根本原因 |
|------|------|----------|
| 小型專案（< 500 symbols）+ 基本 `search_graph` | 快速 | SQLite 全量載入仍可接受 |
| `get_code_snippet` 已知 qualified_name | 快速 | 單一 SQL 查詢 |
| `index_status` / `list_projects` | 快速 | 輕量查詢 |
| 大型專案（5000+ symbols）+ `search_graph` 含 `relationship` | **卡住** | 載入所有 symbols + edges 到記憶體 |
| 大量索引後的 `search_code()` | **卡住** | 載入並解壓縮每個檔案內容 |
| `trace_path` depth > 3 且圖大 | **卡住** | N+1 SQL（每層每鄰居獨立查詢） |
| 大型專案 `index_repository` | **卡住** | MCP 單執行緒阻塞其他請求 |
| `vector_search()` 200+ 向量 | **卡住** | N+1 查詢（每向量一次 find_symbol） |
| `get_architecture()` 大量 symbols | **卡住** | 全量載入 symbols 計算社群 |

---

## P0 — 搜尋正確性與使用體驗（先決條件）

### TODO P0.1 — `search_graph` 使用 SQL WHERE 下推過濾

**問題：** `store/search()` 先 `SELECT * FROM symbols WHERE project = ?1` 載入全部 symbols，再在 Rust 端逐筆過濾。對 5000+ symbols 的專案每次 search 都浪費大量記憶體與 CPU。

**改進方式：**

```rust
// 現在（壞）
let all: Vec<Symbol> = stmt.query_map(...)...collect();
let filtered: Vec<Symbol> = all.iter().filter(|sym| matches_filter(sym, filter)).collect();

// 改為（好）
// 將 label, query, name_pattern, qn_pattern, file_pattern 編譯成 SQL WHERE 子句
// 例如：WHERE project = ?1 AND label = ?2 AND name LIKE ?3
// 只有在需要 relationship/degree 過濾時才載入全部
```

**完成條件：**

- `search_graph(label="Function", query="foo")` 實際只從 SQLite 取得少量 row
- 專案 10000+ symbols，不使用 graph filter 時 response < 100ms
- 仍然支援 `include_connected`, `exclude_entry_points`, `min_degree`, `max_degree`（需要載入 edges 但只在必要時）

---

### TODO P0.2 — 解決 `search_code` 全量載入 + 解壓縮

**問題：** `search_code()` 載入所有 files 的 content，解壓縮（zstd/lz4），然後用 `string.contains()` 掃描。對 500+ 檔案的專案非常慢。

**改進方式：**

方案 A（推薦）：SQLite FTS5
- 在 schema 中建立 `files_fts` 虛擬表
- indexing 時自動寫入 FTS
- `search_code()` 走 FTS5 `MATCH`，不回傳 content

方案 B（最小）：批次解壓 + stream
- 只拉 path 與 content，但一次只留一個檔案在記憶體
- 找到 match 後立即回傳，不全部載入

**完成條件：**

- `search_code("fn main")` 在 100 個檔案中 < 500ms
- 大檔案（1MB+）不會造成 OOM
- 回傳結果包含正確的 line 與 preview

---

## P1 — 高優先級效能改善

### TODO P1.1 — 讓 MCP 伺服器可以非同步處理請求

**問題：** `mcp/server.rs` 的 run loop 是純同步：read → process → write，一次只服務一個請求。`index_repository` 花 30 秒時，所有其他工具呼叫排隊。

**改進方式：**

```rust
// 現在（壞）
loop {
    let message = read_stdin_message()?;     // 阻塞
    let response = self.handle_message(&msg)?; // 同步
    write_stdout_message(&body, framing)?;
}

// 改為：使用 tokio 或 thread pool
// 方案 A：使用 tokio::spawn + 獨立的 stdin/stdout channel
// 方案 B：對已知長時間的呼叫（index_repository）使用 background thread + 立即回傳
// 方案 C（務實）：檢查工具名稱，長作業 spawn 到 background thread
```

**完成條件：**

- 啟動 `index_repository` 後，可以同時執行 `index_status` 而不被阻塞
- 與 OpenCode MCP client 的相容性不受影響
- 無 race condition 或資料競爭

---

### TODO P1.2 — 解決 `trace_path` N+1 SQL 問題

**問題：** `trace_path` 對每個 BFS 層級的每個鄰居都會執行 `find_symbol()`（一次 SQL 查詢）。depth=3、fan-out=30 時可能產生 > 1000 次 SQL。

**改進方式：**

```rust
// 現在（壞）
let edge_rows = self.edges_from(&qn)?;  // SQL #1
for edge in edge_rows {
    if let Some(sym) = self.find_symbol(neighbor)? { ... }  // SQL #2, #3, #4...
}

// 改為：批次收集 + 一次 JOIN 查詢
// pub fn find_symbols_batch(&self, qns: &[&str]) -> Result<HashMap<String, Symbol>> {
//     // SELECT ... FROM symbols WHERE qualified_name IN (?1, ?2, ...) AND project = ?
// }
```

**完成條件：**

- `trace_path(function_name="run", depth=3, direction="both")` 執行 < 5 次 SQL 查詢
- 大型圖（10000+ edges）depth=3 能在 2 秒內完成
- 不改變回傳格式

---

### TODO P1.3 — 解決 `vector_search` N+1 查詢問題

**問題：** `semantic::vector_search()` 對每個 prefilter 通過的向量都呼叫 `store.find_symbol(&qn)`，又是一個 N+1。

**改進方式：**

```rust
// 現在：載入所有向量，逐個 find_symbol
let entries = store.list_vector_entries()?;  // JOIN symbols, 好
for (qn, stored, _name, _label, _file_path) in entries {
    let Some(sym) = store.find_symbol(&qn)? else { continue; };  // 但又一輪查詢！
    ...
}

// 改為：list_vector_entries 已經 JOIN symbols，直接使用回傳的 name/label/file_path
// 或者：vector search 直接使用 JOIN 結果，不需要再次查詢
```

**完成條件：**

- `vector_search` 完全不需要呼叫 `find_symbol`
- `search_graph(semantic_query="foo")` 在 500 個向量中 < 1 秒

---

### TODO P1.4 — 調校 SQLite 連線 pragma

**問題：** `apply_sqlite_pragmas` 只設定了 `journal_mode=WAL` 和 `synchronous=NORMAL`。沒有設定 `cache_size`、`temp_store`、`mmap_size`（除非環境變數有設）。預設 SQLite cache 僅 2MB，大型查詢會頻繁 page fault。

**改進方式：**

```rust
fn apply_sqlite_pragmas(conn: &Connection) -> Result<()> {
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
    conn.execute_batch("PRAGMA cache_size = -64000")?;   // 64MB cache
    conn.execute_batch("PRAGMA temp_store = MEMORY")?;    // temp 放記憶體
    conn.execute_batch("PRAGMA mmap_size = 268435456")?;  // 256MB mmap（如果平台支援）
    Ok(())
}
```

**完成條件：**

- 大型查詢（取得 10000+ rows）速度改善至少 2x
- 與現有 `CBRLM_SQLITE_MMAP_SIZE` 環境變數相容（如果使用者有設定，優先使用）
- 沒有新增 regression

---

## P2 — 中優先級改善

### TODO P2.1 — 減少 `get_architecture()` 中不必要的大量 symbol 載入

**問題：** `get_architecture()` 第 649 行呼叫 `store.list_symbols()` 載入所有 symbols，只為了掃描 `properties_json` 中的 `community_id`。

**改進方式：**

社群資訊應從 meta 表取得，或將 community 存在獨立的 `communities` 表，而非掃描所有 properties_json。

**完成條件：**

- `get_architecture()` 不需載入所有 symbols
- `community_count` 與 `top_communities` 資訊仍然正確

---

### TODO P2.2 — 加入連線池 / Prepared Statement 快取

**問題：** 每個 MCP 工具呼叫都 `Store::open(&project)?`，建立新的 SQLite 連線並重新執行 pragma。沒有 prepared statement 快取。

**改進方式：**

方案：引入 `StorePool`，依據 project name 快取連線。

```rust
pub struct StorePool {
    stores: Mutex<HashMap<String, Store>>,
}
impl StorePool {
    pub fn get(&self, project: &str) -> Result<StoreGuard> { ... }
}
```

**完成條件：**

- 連續 10 次 `search_graph` 只產生 1 次 pragma 設定
- 記憶體使用量不會顯著增加
- 執行緒安全

---

### TODO P2.3 — 控制 WAL 檔案大小

**問題：** `checkpoint()` 使用 PASSIVE 模式，在有讀取器時不會強制 checkpoint。多次增量索引後 WAL 檔案可能非常大，拖慢讀取。

**改進方式：**

- 在索引完成後檢查 WAL 大小
- 如果 WAL > 100MB，用 TRUNCATE 模式 checkpoint
- 或在背景定期執行 checkpoint

**完成條件：**

- 索引後 WAL 檔案 < 10MB
- 不會因為 checkpoint 阻塞讀取器
- 不影響正在進行的查詢

---

### TODO P2.4 — 語意傳遞 O(n²) 加入近似搜尋

**問題：** `compute_semantic_edges()` 對所有 symbols 進行 pairwise 比較（O(n²)）。500 個 symbols 約 125K 次比較。

**改進方式：**

使用近似最近鄰居（ANN）或分桶策略：
- 先用 quantized i8 向量的 cosine 做粗篩
- 只對粗篩通過的 pair 進行完整 scoring
- 或使用 HNSW 索引

**完成條件：**

- 語意傳遞在 1000+ symbols 時速度提升至少 5x
- `SIMILAR_TO` 與 `SEMANTICALLY_RELATED` recall 不下降超過 5%

---

## P3 — 低優先級/長期改善

### TODO P3.1 — 加入效能指標與自我監控

**問題：** 目前無法知道哪個工具呼叫花了多少時間、哪個 SQL 查詢最慢。

**改進方式：**

- 在 `Store` 加入查詢時間統計
- 提供一個 `diagnostics` 工具回傳最近 N 個請求的延遲分佈
- 使用 tracing event 記錄慢查詢（> 500ms）

---

### TODO P3.2 — 為長工具加入進度回報

**問題：** `index_repository` 執行期間 agent 完全不知道進度。

**改進方式：**

- 使用 MCP `notifications/progress`（MCP 2025 規範）
- 或定時寫入可查詢的進度狀態

---

### TODO P3.3 — 索引 pipeline 平行化

**問題：** `finalize_index` 中的 edge extraction（structure、imports、calls、routes、inheritance）依序執行。

**改進方式：**

沒有依賴的階段可以平行處理：
- structure graph（CONTAINS）
- import 解析
- 可以在不同 thread 上同時進行

---

## 執行順序建議

1. **P0.1** + **P0.2** — 搜尋回應速度的核心改善
2. **P1.1** — 解決 MCP 伺服器阻塞
3. **P1.2** + **P1.3** — 解決 N+1 SQL 問題
4. **P1.4** — SQLite pragma 調校（快速見效）
5. **P2.1** — 減少不必要載入
6. **P2.2** — 連線池
7. **P2.3** — WAL 管理
8. **P2.4** — 語意傳遞最佳化
9. **P3.x** — 長期改善

## 驗證閘門

每個 TODO 項目完成後必須通過：

- `cargo test --all-targets`（現有測試不應 regression）
- `cargo clippy --all-targets -- -D warnings`
- 對應的基準測試（詳見 `test.md`）
