use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentKind {
    ClaudeCode,
    Codex,
    GeminiCli,
    OpenCode,
    Zed,
    Aider,
    Antigravity,
    KiloCode,
    Kiro,
    Unknown,
}

impl AgentKind {
    pub fn slug(&self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::GeminiCli => "gemini-cli",
            Self::OpenCode => "opencode",
            Self::Zed => "zed",
            Self::Aider => "aider",
            Self::Antigravity => "antigravity",
            Self::KiloCode => "kilo-code",
            Self::Kiro => "kiro",
            Self::Unknown => "unknown",
        }
    }

    pub fn detect() -> Self {
        if std::env::var("CLAUDE_CODE").is_ok() || std::env::var("CLAUDE_SESSION").is_ok() {
            return Self::ClaudeCode;
        }
        if std::env::var("CODEX_HOME").is_ok() || std::env::var("OPENAI_API_KEY").is_ok() {
            return Self::Codex;
        }
        if std::env::var("GEMINI_CLI").is_ok() {
            return Self::GeminiCli;
        }
        if std::env::var("OPENCODE").is_ok() || std::env::var("OPENCODE_CONFIG").is_ok() {
            return Self::OpenCode;
        }
        if std::env::var("ZED_AGENT").is_ok() {
            return Self::Zed;
        }
        if std::env::var("AIDER").is_ok() {
            return Self::Aider;
        }
        Self::Unknown
    }

    pub fn config_dir(&self) -> Option<PathBuf> {
        dirs::home_dir().map(|home| match self {
            Self::ClaudeCode => home.join(".claude"),
            Self::Codex => home.join(".codex"),
            Self::GeminiCli => home.join(".gemini"),
            Self::OpenCode => home.join(".opencode"),
            Self::Zed => home.join(".zed"),
            Self::Aider => home.join(".aider"),
            _ => home.join(".config").join("cbrlm"),
        })
    }

    pub fn mcp_config_snippet(&self) -> serde_json::Value {
        let bin = std::env::current_exe()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "cbrlm".into());
        serde_json::json!({
            "mcpServers": {
                "cbrlm-mcp": {
                    "command": bin,
                    "args": [],
                    "env": {
                        "CBRLM_AGENT": self.slug(),
                        "CBRLM_PROJECT_PREFIX": "cbrlm+"
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_unknown_by_default() {
        assert_eq!(AgentKind::detect(), AgentKind::Unknown);
    }
}
