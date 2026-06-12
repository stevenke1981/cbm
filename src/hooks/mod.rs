//! Non-blocking agent hooks (SessionStart + PreToolUse augment).
//! Hooks never block tool calls; errors exit 0 with no stdout.

use crate::project::{project_db_path, project_name_from_path};
use crate::store::{SearchFilter, Store};
use serde_json::{json, Value};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const STDIN_CAP: usize = 256 * 1024;
const MIN_TOKEN: usize = 4;
const MAX_TOKEN: usize = 96;
const RESULT_LIMIT: usize = 5;
const MAX_WALKUP: usize = 8;
const DEADLINE_MS: u64 = 300;

pub const SESSION_REMINDER: &str = "\
CRITICAL - Code Discovery Protocol (CBRLM / cbrlm-mcp):
1. ALWAYS use cbrlm MCP tools FIRST for code exploration:
   - search_graph / rlm_filter to find functions, classes, routes
   - trace_path for call chains and data flow
   - rlm_read_symbol / get_code_snippet for exact symbol source
   - rlm_scan / rlm_peek / rlm_chunk for logs and huge non-code files
   - rlm_workflow for map-reduce phases
2. Project names use cbrlm+ prefix; shares ~/.cache/cbrlm-mcp with upstream CBM.
3. Use Grep/Glob/Read freely for text, configs, and non-code files; always Read a file before editing it.
4. If the project is not indexed yet, run index_repository FIRST.";

pub const CODEX_SESSION_REMINDER_CMD: &str = "\
echo \"Code discovery: prefer cbrlm-mcp (search_graph, trace_path, rlm_filter, rlm_read_symbol) over grep/file-read; projects use cbrlm+ prefix; run index_repository first if not indexed.\"";

pub const CODEX_HOOK_BEGIN: &str = "# >>> cbrlm-mcp SessionStart >>>";
pub const CODEX_HOOK_END: &str = "# <<< cbrlm-mcp SessionStart <<<";

pub fn hook_session_start() -> i32 {
    print!("{SESSION_REMINDER}");
    0
}

pub fn hook_augment() -> i32 {
    let deadline = Instant::now() + Duration::from_millis(DEADLINE_MS);
    let input = match read_stdin() {
        Some(s) => s,
        None => return 0,
    };
    let root: Value = match serde_json::from_str(&input) {
        Ok(v) => v,
        Err(_) => return 0,
    };
    let tool = root.get("tool_name").and_then(|v| v.as_str());
    if tool != Some("Grep") && tool != Some("Glob") {
        return 0;
    }
    let pattern = root
        .get("tool_input")
        .and_then(|v| v.get("pattern"))
        .and_then(|v| v.as_str());
    let token = match pattern.and_then(extract_token) {
        Some(t) => t,
        None => return 0,
    };
    let cwd = root
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok());
    let start = match cwd {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => return 0,
    };
    if let Some(ctx) = resolve_and_query(&start, &token, deadline) {
        emit_augment(&ctx);
    }
    0
}

pub fn extract_token(pattern: &str) -> Option<String> {
    let mut best_start = 0usize;
    let mut best_len = 0usize;
    let bytes = pattern.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            i += 1;
            while i < bytes.len() {
                let ch = bytes[i] as char;
                if ch.is_ascii_alphanumeric() || ch == '_' {
                    i += 1;
                } else {
                    break;
                }
            }
            let len = i - start;
            if len > best_len {
                best_len = len;
                best_start = start;
            }
        } else {
            i += 1;
        }
    }
    if best_len < MIN_TOKEN {
        return None;
    }
    let len = best_len.min(MAX_TOKEN);
    Some(pattern[best_start..best_start + len].to_string())
}

fn read_stdin() -> Option<String> {
    let mut buf = String::new();
    let mut handle = io::stdin().lock();
    let mut chunk = [0u8; 4096];
    let mut total = 0usize;
    loop {
        if total >= STDIN_CAP {
            break;
        }
        let n = handle.read(&mut chunk).ok()?;
        if n == 0 {
            break;
        }
        let take = n.min(STDIN_CAP - total);
        buf.push_str(&String::from_utf8_lossy(&chunk[..take]));
        total += take;
    }
    if buf.is_empty() {
        None
    } else {
        Some(buf)
    }
}

enum QueryOutcome {
    Hits(String),
    NoHits,
    NotIndexed,
    Error,
}

fn resolve_and_query(start: &Path, token: &str, deadline: Instant) -> Option<String> {
    let mut dir = start.to_path_buf();
    for _ in 0..MAX_WALKUP {
        if Instant::now() >= deadline {
            return None;
        }
        match query_project(&dir, token) {
            QueryOutcome::Hits(ctx) => return Some(ctx),
            QueryOutcome::NoHits => return None,
            QueryOutcome::NotIndexed | QueryOutcome::Error => {
                if !dir.pop() {
                    break;
                }
            }
        }
    }
    None
}

fn query_project(dir: &Path, token: &str) -> QueryOutcome {
    let project = project_name_from_path(dir);
    if !project_db_path(&project).exists() {
        return QueryOutcome::NotIndexed;
    }
    let store = match Store::open(&project) {
        Ok(s) => s,
        Err(_) => return QueryOutcome::Error,
    };
    let pattern = format!(".*{}.*", regex::escape(token));
    let hits = match store.search(&SearchFilter {
        name_pattern: Some(pattern),
        limit: RESULT_LIMIT,
        ..SearchFilter::default()
    }) {
        Ok(page) => page.symbols,
        Err(_) => return QueryOutcome::Error,
    };
    if hits.is_empty() {
        return QueryOutcome::NoHits;
    }
    QueryOutcome::Hits(format_context(&hits, token))
}

fn format_context(hits: &[crate::store::Symbol], token: &str) -> String {
    let mut text = format!(
        "[cbrlm] {} graph symbol(s) match \"{}\" \
         (structured context; your search results below are unaffected):",
        hits.len(),
        token
    );
    for hit in hits {
        let disp = if !hit.qualified_name.is_empty() {
            &hit.qualified_name
        } else {
            &hit.name
        };
        let label = if hit.label.is_empty() {
            String::new()
        } else {
            format!("  {}", hit.label)
        };
        text.push_str(&format!("\n- {disp}  {}{label}", hit.file_path));
        if text.len() > 3900 {
            break;
        }
    }
    text
}

fn emit_augment(text: &str) {
    let out = json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "additionalContext": text
        }
    });
    if let Ok(s) = serde_json::to_string(&out) {
        print!("{s}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_token_skips_short_patterns() {
        assert_eq!(extract_token("ab"), None);
        assert_eq!(extract_token("*.ts"), None);
    }

    #[test]
    fn extract_token_picks_longest_identifier() {
        assert_eq!(
            extract_token("foo.*handleAuth_bar"),
            Some("handleAuth_bar".into())
        );
        assert_eq!(extract_token("UserService"), Some("UserService".into()));
    }

    #[test]
    fn session_reminder_mentions_server() {
        assert!(SESSION_REMINDER.contains("cbrlm-mcp"));
    }
}
