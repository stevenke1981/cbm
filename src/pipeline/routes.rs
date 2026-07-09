//! HTTP_ROUTE and HTTP_CALLS edge extraction.
//!
//! - `HTTP_ROUTE`: framework route registration → handler symbol
//! - `HTTP_CALLS`: client fetch/axios/reqwest call site → matching route node

use crate::store::{Edge, Symbol};
use regex::Regex;
use std::collections::HashSet;

/// Extract HTTP_ROUTE edges linking route paths to handler symbols.
pub fn extract_http_routes(
    file_path: &str,
    language: &str,
    content: &str,
    symbols: &[Symbol],
) -> Vec<Edge> {
    let mut edges = Vec::new();
    let mut seen = HashSet::new();

    let patterns: &[(&str, &str)] = match language {
        "python" => &[
            (
                r#"(?m)^\s*@(?:app|router|bp|api)\.(get|post|put|delete|patch|route)\(\s*["']([^"']+)["']"#,
                "decorator",
            ),
            (
                r#"(?m)^\s*@(?:get|post|put|delete|patch)\(\s*["']([^"']+)["']"#,
                "fastapi",
            ),
        ],
        "javascript" | "typescript" | "tsx" | "jsx" => &[(
            r#"(?m)\.(get|post|put|delete|patch)\(\s*["']([^"']+)["']"#,
            "express",
        )],
        "rust" => &[
            (r#"\.route\(\s*["']([^"']+)["']"#, "axum"),
            (
                r#"\.(get|post|put|delete|patch)\(\s*["']([^"']+)["']"#,
                "axum_method",
            ),
        ],
        "go" => &[(
            r#"(?m)\.(?:HandleFunc|GET|POST|PUT|DELETE|PATCH)\(\s*["']([^"']+)["']"#,
            "go_http",
        )],
        "java" | "kotlin" => &[(
            r#"@(?:Get|Post|Put|Delete|Patch|Request)Mapping\(\s*(?:value\s*=\s*)?["']([^"']+)["']"#,
            "spring",
        )],
        _ => &[],
    };

    for (pattern, framework) in patterns {
        let Ok(re) = Regex::new(pattern) else {
            continue;
        };
        for cap in re.captures_iter(content) {
            let path = match *framework {
                "decorator" | "express" | "axum_method" => cap.get(2).map(|m| m.as_str()),
                "fastapi" | "axum" | "go_http" | "spring" => cap.get(1).map(|m| m.as_str()),
                _ => None,
            };
            let Some(route_path) = path else { continue };
            let line = line_number(content, cap.get(0).unwrap().start());
            let Some(handler) = handler_after_line(symbols, file_path, line) else {
                continue;
            };
            let dst = format!("{file_path}::Route::{route_path}@L{line}");
            let key = (handler.clone(), dst.clone());
            if seen.insert(key) {
                edges.push(Edge {
                    src_qn: handler,
                    dst_qn: dst,
                    edge_type: "HTTP_ROUTE".into(),
                    properties_json: Some(format!(
                        r#"{{"path":"{}","framework":"{framework}"}}"#,
                        json_escape(route_path)
                    )),
                });
            }
        }
    }
    edges
}

/// Client HTTP call sites found in source (caller symbol + URL path).
#[derive(Debug, Clone)]
pub struct HttpClientCall {
    pub caller_qn: String,
    pub path: String,
    pub method: Option<String>,
    pub line: i64,
}

/// Extract client-side HTTP invocations (fetch/axios/reqwest/httpx/etc.).
pub fn extract_http_client_calls(
    file_path: &str,
    language: &str,
    content: &str,
    symbols: &[Symbol],
) -> Vec<HttpClientCall> {
    let patterns: &[(&str, Option<&str>)] = match language {
        "javascript" | "typescript" | "tsx" | "jsx" => &[
            (r#"\bfetch\s*\(\s*["'`]([^"'`]+)["'`]"#, None),
            (r#"\baxios\.(get|post|put|delete|patch)\s*\(\s*["'`]([^"'`]+)["'`]"#, Some("method_url")),
            (r#"\baxios\s*\(\s*\{\s*url\s*:\s*["'`]([^"'`]+)["'`]"#, None),
            (r#"\b(?:http|https)\.(get|post|put|delete|patch)\s*\(\s*["'`]([^"'`]+)["'`]"#, Some("method_url")),
        ],
        "python" => &[
            (r#"\brequests\.(get|post|put|delete|patch)\s*\(\s*["']([^"']+)["']"#, Some("method_url")),
            (r#"\bhttpx\.(get|post|put|delete|patch)\s*\(\s*["']([^"']+)["']"#, Some("method_url")),
            (r#"\b(?:client|session)\.(get|post|put|delete|patch)\s*\(\s*["']([^"']+)["']"#, Some("method_url")),
            (r#"\burlopen\s*\(\s*["']([^"']+)["']"#, None),
        ],
        "rust" => &[
            (r#"\breqwest::(?:get|post|put|delete|patch)\s*\(\s*["']([^"']+)["']"#, None),
            (r#"\.(?:get|post|put|delete|patch)\s*\(\s*["']([^"']+)["']"#, None),
            (r#"Client::new\(\)[\s\S]{0,80}?\.get\s*\(\s*["']([^"']+)["']"#, None),
        ],
        "go" => &[
            (r#"\bhttp\.(?:Get|Post|Put|Delete)\s*\(\s*["']([^"']+)["']"#, None),
            (r#"\bNewRequest\s*\(\s*["'](\w+)["']\s*,\s*["']([^"']+)["']"#, Some("go_newrequest")),
        ],
        "java" | "kotlin" => &[
            (r#"\.(?:get|post|put|delete|patch)\s*\(\s*["']([^"']+)["']"#, None),
            (r#"Uri\.(?:parse|of)\s*\(\s*["']([^"']+)["']"#, None),
        ],
        _ => &[],
    };

    let mut out = Vec::new();
    for (pattern, kind) in patterns {
        let Ok(re) = Regex::new(pattern) else {
            continue;
        };
        for cap in re.captures_iter(content) {
            let (path, method) = match *kind {
                Some("method_url") => (
                    cap.get(2).map(|m| m.as_str()).unwrap_or(""),
                    cap.get(1).map(|m| m.as_str().to_uppercase()),
                ),
                Some("go_newrequest") => (
                    cap.get(2).map(|m| m.as_str()).unwrap_or(""),
                    cap.get(1).map(|m| m.as_str().to_uppercase()),
                ),
                _ => (
                    cap.get(1).map(|m| m.as_str()).unwrap_or(""),
                    None,
                ),
            };
            if path.is_empty() {
                continue;
            }
            let Some(route_path) = normalize_url_path(path) else {
                continue;
            };
            let line = line_number(content, cap.get(0).unwrap().start());
            let Some(caller) = enclosing_function(symbols, file_path, line) else {
                continue;
            };
            out.push(HttpClientCall {
                caller_qn: caller,
                path: route_path,
                method,
                line,
            });
        }
    }
    out
}

/// Link client calls to HTTP_ROUTE nodes by path equality / suffix match.
pub fn link_http_calls(client_calls: &[HttpClientCall], route_edges: &[Edge]) -> Vec<Edge> {
    // route path from properties or Route qn
    let mut routes: Vec<(String, String)> = Vec::new(); // (path, route_qn)
    for edge in route_edges {
        if edge.edge_type != "HTTP_ROUTE" {
            continue;
        }
        let path = edge
            .properties_json
            .as_ref()
            .and_then(|p| serde_json::from_str::<serde_json::Value>(p).ok())
            .and_then(|v| v.get("path").and_then(|x| x.as_str()).map(str::to_string))
            .or_else(|| path_from_route_qn(&edge.dst_qn));
        if let Some(p) = path {
            routes.push((normalize_route_key(&p), edge.dst_qn.clone()));
        }
    }

    let mut edges = Vec::new();
    let mut seen = HashSet::new();
    for call in client_calls {
        let key = normalize_route_key(&call.path);
        let mut matched: Option<&String> = None;
        for (route_path, route_qn) in &routes {
            if route_path == &key || route_path.ends_with(&key) || key.ends_with(route_path) {
                matched = Some(route_qn);
                break;
            }
        }
        let Some(route_qn) = matched else {
            continue;
        };
        let edge_key = (call.caller_qn.clone(), route_qn.clone());
        if !seen.insert(edge_key) {
            continue;
        }
        let method = call.method.as_deref().unwrap_or("");
        edges.push(Edge {
            src_qn: call.caller_qn.clone(),
            dst_qn: route_qn.clone(),
            edge_type: "HTTP_CALLS".into(),
            properties_json: Some(format!(
                r#"{{"path":"{}","method":"{}","line":{},"confidence":"path_match"}}"#,
                json_escape(&call.path),
                json_escape(method),
                call.line
            )),
        });
    }
    edges
}

fn path_from_route_qn(qn: &str) -> Option<String> {
    // file::Route::{path}@Lline
    let rest = qn.split("::Route::").nth(1)?;
    let path = rest.split("@L").next()?;
    Some(path.to_string())
}

fn normalize_url_path(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    // Full URL → path
    if let Some(idx) = s.find("://") {
        let after = &s[idx + 3..];
        let path = after.find('/').map(|i| &after[i..]).unwrap_or("/");
        return Some(path.split('?').next().unwrap_or(path).to_string());
    }
    if s.starts_with('/') || s.starts_with("./") || s.starts_with("api/") || s.contains('/') {
        return Some(s.split('?').next().unwrap_or(s).to_string());
    }
    // Skip bare hostnames / package-like strings without slash
    None
}

fn normalize_route_key(path: &str) -> String {
    let mut p = path.trim().to_string();
    if p.is_empty() {
        return "/".into();
    }
    if !p.starts_with('/') && !p.starts_with('.') {
        p = format!("/{p}");
    }
    // collapse trailing slash except root
    if p.len() > 1 && p.ends_with('/') {
        p.pop();
    }
    p
}

fn enclosing_function(symbols: &[Symbol], file_path: &str, line: i64) -> Option<String> {
    symbols
        .iter()
        .filter(|s| {
            s.file_path == file_path
                && matches!(s.label.as_str(), "Function" | "Method")
                && s.line_start <= line
                && s.line_end >= line
        })
        .max_by_key(|s| s.line_start)
        .map(|s| s.qualified_name.clone())
}

fn handler_after_line(symbols: &[Symbol], file_path: &str, line: i64) -> Option<String> {
    symbols
        .iter()
        .filter(|s| {
            s.file_path == file_path
                && s.label == "Function"
                && s.line_start > line
                && s.line_start <= line + 5
        })
        .min_by_key(|s| s.line_start)
        .map(|s| s.qualified_name.clone())
}

fn line_number(content: &str, byte_offset: usize) -> i64 {
    content[..byte_offset.min(content.len())].lines().count() as i64 + 1
}

fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(file: &str, name: &str, line: i64, end: i64) -> Symbol {
        Symbol {
            qualified_name: format!("{file}::Function::{name}@L{line}"),
            name: name.into(),
            label: "Function".into(),
            file_path: file.into(),
            line_start: line,
            line_end: end,
            signature: None,
            properties_json: None,
        }
    }

    #[test]
    fn extracts_python_route() {
        let src = "@app.get(\"/users\")\ndef list_users():\n    pass\n";
        let symbols = vec![sym("api.py", "list_users", 2, 3)];
        let edges = extract_http_routes("api.py", "python", src, &symbols);
        assert!(edges.iter().any(|e| e.edge_type == "HTTP_ROUTE"));
    }

    #[test]
    fn links_fetch_to_route() {
        let route = Edge {
            src_qn: "api.js::Function::list@L2".into(),
            dst_qn: "api.js::Route::/users@L1".into(),
            edge_type: "HTTP_ROUTE".into(),
            properties_json: Some(r#"{"path":"/users","framework":"express"}"#.into()),
        };
        let client_src = "function loadUsers() {\n  fetch('/users');\n}\n";
        let symbols = vec![sym("client.js", "loadUsers", 1, 3)];
        let calls = extract_http_client_calls("client.js", "javascript", client_src, &symbols);
        assert!(!calls.is_empty(), "{calls:?}");
        let edges = link_http_calls(&calls, &[route]);
        assert!(
            edges.iter().any(|e| e.edge_type == "HTTP_CALLS"),
            "{edges:?}"
        );
    }
}
