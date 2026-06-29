# cbm MCP 效能驗證與基準測試

本文件定義 cbm 每項效能改善的驗證方法。分為三層：

1. **單元基準測試**（`#[bench]` 或自訂計時）— 驗證單一操作的延遲
2. **整合基準測試**（through binary/CLI）— 驗證真實工具呼叫的表現
3. **CI 品質閘門**（在 CI 中執行的可重複閘門）

---

## 測試用 fixture 專案

所有基準測試應使用以下 fixture 大小分級：

| 等級 | Symbols | Edges | Files | 說明 |
|------|---------|-------|-------|------|
| `Small` | ~50 | ~100 | ~10 | 最小的可用 fixture（現有測試用） |
| `Medium` | ~500 | ~2000 | ~50 | 中等規模，用來找出 O(n²) 問題 |
| `Large` | ~5000 | ~30000 | ~500 | 大型專案模擬，觸發真實瓶頸 |

**建立方式：** 建立一個 `tests/fixtures/` 目錄，使用腳本產生不同大小的合成 Rust/TypeScript 專案。

```powershell
# 產生 Medium fixture
cargo run -- cli index_repository --json --quiet '{
    "repo_path": "tests/fixtures/medium",
    "project": "perf-test-medium",
    "mode": "fast",
    "persistence": false
}'
```

---

## P0.1 — `search_graph` SQL WHERE 下推

### 單元基準測試

```rust
#[cfg(test)]
mod benchmarks {
    use super::*;
    
    /// 驗證 search 在有/無 graph filter 時的行為
    #[test]
    fn search_without_graph_filter_uses_sql_filtering() {
        let store = Store::open_memory().unwrap();
        // 插入 1000 個 symbols
        for i in 0..1000 {
            store.upsert_symbol(&Symbol {
                qualified_name: format!("test.rs::Function::func{i}@L{i}"),
                name: format!("func{i}"),
                label: if i % 2 == 0 { "Function".into() } else { "Class".into() },
                file_path: "test.rs".into(),
                line_start: i as i64,
                line_end: (i + 1) as i64,
                signature: None,
                properties_json: None,
            }).unwrap();
        }
        
        // 只查 Function，不涉及 graph filter
        let start = std::time::Instant::now();
        let result = store.search(&SearchFilter {
            label: Some("Function".into()),
            limit: 10,
            ..Default::default()
        }).unwrap();
        let elapsed = start.elapsed();
        
        // 應遠快於載入全部 1000 筆
        assert!(elapsed.as_millis() < 50, 
            "search took {}ms, expected <50ms", elapsed.as_millis());
        assert_eq!(result.total, 500); // 一半是 Function
    }
    
    /// 驗證使用 graph filter 時仍然正確（但不需要很快）
    #[test]
    fn search_with_graph_filter_still_correct() {
        let store = setup_graph_fixture(100);
        let result = store.search(&SearchFilter {
            relationship: Some("CALLS".into()),
            min_degree: Some(2),
            limit: 10,
            ..Default::default()
        }).unwrap();
        assert!(result.total > 0);
    }
}
```

### CLI 基準測試

```powershell
# 計時：Medium 專案，無 graph filter
Measure-Command {
    cargo run -- cli search_graph --json --quiet '{
        "project": "perf-test-medium",
        "label": "Function",
        "query": "handler",
        "limit": 10
    }'
} | Select-Object TotalMilliseconds
# 閘門：< 100ms
```

### CI 閘門

- 上述單元測試的計時 assertion
- Medium fixture 的 CLI search 必須在 200ms 內完成

---

## P0.2 — `search_code` FTS5

### 單元基準測試

```rust
#[test]
fn search_code_uses_fts() {
    let store = Store::open_memory().unwrap();
    // 插入 50 個檔案，每個 ~10KB
    for i in 0..50 {
        store.upsert_file(&SourceFile {
            path: format!("file{i}.rs"),
            content: format!("// File {i}\npub fn func{i}() {{}}\n".repeat(200)),
            language: "rust".into(),
            line_count: 400,
        }).unwrap();
    }
    
    let start = std::time::Instant::now();
    let matches = store.search_code("pub fn", 5).unwrap();
    let elapsed = start.elapsed();
    
    // 必須快於 500ms（全量載入+解壓縮在 50 個檔案上通常 > 2s）
    assert!(elapsed.as_millis() < 500);
    assert!(!matches.is_empty());
}
```

### CLI 基準測試

```powershell
Measure-Command {
    cargo run -- cli search_code --json --quiet '{
        "project": "perf-test-medium",
        "pattern": "fn handle",
        "limit": 5
    }'
} | Select-Object TotalMilliseconds
# 閘門：< 1000ms
```

---

## P1.1 — 非同步 MCP 伺服器

### 流程測試（手動 / CI）

```powershell
# 測試方案：啟動 cbm MCP 伺服器，發送兩個請求而不等待第一個回覆
# 實際測試需要能並行發送 JSON-RPC

# 簡化測試：驗證 slow tool 期間可以執行 fast tool
# 1. 在背景啟動 MCP server
$server = Start-Process -NoNewWindow -PassThru cbm -RedirectStandardInput stdin.txt

# 2. 測試方式請見 scripts/smoke-nonblocking.ps1
```

### 腳本：`scripts/smoke-nonblocking.ps1`

```powershell
param(
    [string]$BinaryPath = "cbm"
)

# 用 MCP stdio 啟動伺服器
# 發送 initialize → index_repository (large) → 發送 index_status → 確認 index_status 立即回覆
Write-Host "Non-blocking smoke: not yet implemented"
```

### CI 閘門

- `smoke-nonblocking.ps1` 加入 CI 定期執行
- 驗證 `index_repository` 跟 `index_status` 可以交錯執行

---

## P1.2 — `trace_path` N+1 解決

### 單元基準測試

```rust
#[test]
fn trace_path_batch_resolve() {
    let store = setup_graph_fixture(500); // 500 symbols, ~2000 edges
    
    let start = std::time::Instant::now();
    let result = store.trace_path("test.rs::Function::func0@L0", "outbound", 3).unwrap();
    let elapsed = start.elapsed();
    
    // 應使用批次查詢，depth=3 不應產生大量 SQL
    assert!(elapsed.as_millis() < 500, 
        "trace_path took {}ms", elapsed.as_millis());
    assert!(!result.nodes.is_empty());
}
```

### SQL 查詢計數測試

```rust
/// 需要 mock 或 wrapper 來計算 SQL 查詢次數
#[test]
fn trace_path_sql_count() {
    let store = setup_graph_fixture(100);
    let sql_count_before = store.sql_query_count();  // 需要加入此功能
    let _result = store.trace_path("test.rs::Function::main@L0", "outbound", 2).unwrap();
    let sql_count_after = store.sql_query_count();
    
    // depth=2，batch 查詢後應 < 3 次 SQL
    assert!(sql_count_after - sql_count_before < 3, 
        "trace_path used {} SQL queries", sql_count_after - sql_count_before);
}
```

### CLI 基準測試

```powershell
Measure-Command {
    cargo run -- cli trace_path --json --quiet '{
        "project":"perf-test-medium",
        "function_name":"test.rs::Function::main@L1",
        "direction":"both",
        "depth":3
    }'
} | Select-Object TotalMilliseconds
# 閘門：< 2000ms
```

---

## P1.3 — `vector_search` N+1 解決

### 單元基準測試

```rust
#[test]
fn vector_search_no_nplus1() {
    // 插入 300 個 symbols + vectors
    let store = setup_semantic_fixture(300);
    
    // 驗證不呼叫 find_symbol
    let start = std::time::Instant::now();
    let result = semantic::vector_search(&store, "fetch_user_handler", 10).unwrap();
    let elapsed = start.elapsed();
    
    assert!(elapsed.as_millis() < 1000, 
        "vector_search took {}ms", elapsed.as_millis());
    assert!(!result.matches.is_empty());
}
```

---

## P1.4 — SQLite Pragma 調校

### 基準測試

```rust
#[test]
fn sqlite_pragma_cache_effect() {
    let store = Store::open_memory().unwrap();
    
    // 插入 10000 個 symbols
    for i in 0..10000 {
        store.upsert_symbol(&Symbol { ... }).unwrap();
    }
    
    let start = std::time::Instant::now();
    let all = store.list_symbols().unwrap();
    let elapsed = start.elapsed();
    
    // 64MB cache + mmap 後應 < 200ms
    assert!(elapsed.as_millis() < 200);
    assert_eq!(all.len(), 10000);
}
```

---

## P2.4 — 語意傳遞 O(n²) 改善

### 基準測試

```rust
#[test]
fn semantic_pass_1000_symbols() {
    let store = setup_semantic_fixture(1000);
    
    let start = std::time::Instant::now();
    let result = semantic::run_semantic_pass(&store).unwrap();
    let elapsed = start.elapsed();
    
    // 改善前 1000 symbols 約 10-15 秒
    // 改善後應 < 3 秒
    assert!(elapsed.as_secs() < 3, 
        "semantic pass took {}s for 1000 symbols", elapsed.as_secs());
    assert!(result.vectors_stored > 0);
}
```

---

## 現有測試不應 Regression

每個 TODO 完成後必須執行：

```powershell
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```

特別要注意的測試（原有行為必須維持）：

| 測試檔案 | 測試名稱 | 涵蓋 |
|----------|----------|------|
| `src/store/mod.rs` | `search_returns_indexed_symbols` | search 基本正確性 |
| `src/store/mod.rs` | `search_filters_by_relationship_and_degree` | graph filter 正確性 |
| `src/store/mod.rs` | `roundtrips_compressed_file_content` | content 壓縮/解壓縮 |
| `src/pipeline/mod.rs` | `persists_symbols_to_store` | 索引流程 |
| `src/semantic/mod.rs` | `similar_symbols_score_high` | 語意分數正確性 |

---

## 自動化效能迴歸測試

在 CI 中加入一個 `perf` 工作：

```yaml
perf:
  runs-on: windows-latest
  steps:
    - uses: actions/checkout@v4
    - run: cargo build --release
    - name: Setup medium fixture
      run: |
        # 建立或還原 benchmark fixture
        cargo run --release -- cli index_repository --json --quiet '{
          "repo_path": "tests/fixtures/medium",
          "project": "perf-ci-medium",
          "mode": "fast",
          "persistence": false
        }'
    - name: Run perf benchmarks
      run: cargo test --release perf -- --nocapture
    # 如果超過閘門值則標記為 failure
```

---

## 快速上手指令

```powershell
# 執行所有單元測試
cargo test --all-targets

# 只執行效能基準測試
cargo test --release -- --test-threads=1 perf

# 手動 CLI 基準測試（Medium fixture）
Measure-Command {
    cargo run --release -- cli search_graph --json --quiet '{
        "project": "perf-test-medium",
        "label": "Function",
        "limit": 10
    }'
}

# 執行所有品質閘門
.\scripts\smoke-quality-gates.ps1
```
