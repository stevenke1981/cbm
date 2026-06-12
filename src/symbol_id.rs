//! Stable unique qualified names — disambiguate same display name in one file.

pub fn qualified_name(file_path: &str, label: &str, name: &str, line_start: i64) -> String {
    format!("{file_path}::{label}::{name}@L{line_start}")
}

pub fn display_name(qn: &str) -> &str {
    qn.rsplit("::")
        .next()
        .and_then(|s| s.split('@').next())
        .unwrap_or(qn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disambiguates_same_name() {
        let a = qualified_name("mcp.rs", "Class", "McpServer", 10);
        let b = qualified_name("mcp.rs", "Function", "McpServer", 40);
        assert_ne!(a, b);
        assert!(a.contains("@L10"));
        assert!(b.contains("@L40"));
    }
}
