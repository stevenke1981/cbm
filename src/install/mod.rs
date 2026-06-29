use crate::agent::AgentKind;
use crate::error::{Error, Result};
use crate::hooks::{CODEX_HOOK_BEGIN, CODEX_HOOK_END, CODEX_SESSION_REMINDER_CMD};
use crate::mcp::SERVER_NAME;
use serde_json::{json, Map, Value};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub const MCP_SERVER_NAME: &str = SERVER_NAME;
pub const INSTALL_DIR_NAME: &str = "cbm";

#[derive(Debug, Clone, Default)]
pub struct InstallOptions {
    pub dry_run: bool,
    pub force: bool,
    pub yes: bool,
    pub all_agents: bool,
    pub binary: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct InstallReport {
    pub binary_path: PathBuf,
    pub configured: Vec<String>,
    pub skipped: Vec<String>,
    pub hooks_installed: bool,
}

#[derive(Debug, Clone, Default)]
pub struct UninstallOptions {
    pub dry_run: bool,
    pub yes: bool,
    pub all_agents: bool,
    pub keep_binary: bool,
}

#[derive(Debug, Clone)]
pub struct UninstallReport {
    pub removed: Vec<String>,
    pub skipped: Vec<String>,
}

pub fn default_install_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join(INSTALL_DIR_NAME)
        .join("bin")
}

pub fn installed_binary_path() -> PathBuf {
    let name = if cfg!(windows) { "cbm.exe" } else { "cbm" };
    default_install_dir().join(name)
}

pub fn hooks_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join(INSTALL_DIR_NAME)
        .join("hooks")
}

pub fn claude_hooks_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("CLAUDE_CONFIG_DIR") {
        return PathBuf::from(dir).join("hooks");
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
        .join("hooks")
}

pub fn run_install(opts: &InstallOptions) -> Result<InstallReport> {
    let source = resolve_source_binary(opts.binary.as_deref())?;
    let dest = installed_binary_path();

    if opts.dry_run {
        eprintln!(
            "[dry-run] would copy {} → {}",
            source.display(),
            dest.display()
        );
    } else {
        install_binary(&source, &dest)?;
        eprintln!("installed binary → {}", dest.display());
    }

    let targets = select_targets(opts.all_agents);
    let mut configured = Vec::new();
    let mut skipped = Vec::new();

    for target in targets {
        match configure_agent(&target, &dest, opts) {
            Ok(true) => configured.push(target.label().to_string()),
            Ok(false) => skipped.push(format!("{} (already configured)", target.label())),
            Err(e) => skipped.push(format!("{} ({e})", target.label())),
        }
    }

    if configured.is_empty() && skipped.is_empty() {
        skipped.push("no agent targets".into());
    }

    for line in &configured {
        eprintln!("configured: {line}");
    }
    for line in &skipped {
        eprintln!("skipped: {line}");
    }

    let hooks_installed = match install_hooks(&dest, opts) {
        Ok(true) => {
            eprintln!("installed hooks → {}", hooks_dir().display());
            true
        }
        Ok(false) => false,
        Err(e) => {
            eprintln!("hooks: skipped ({e})");
            false
        }
    };

    Ok(InstallReport {
        binary_path: dest,
        configured,
        skipped,
        hooks_installed,
    })
}

pub fn run_uninstall(opts: &UninstallOptions) -> Result<UninstallReport> {
    let mut removed = Vec::new();
    let mut skipped = Vec::new();

    if !opts.yes && !opts.dry_run && !confirm("uninstall cbm-mcp integration?")? {
        eprintln!("cancelled");
        return Ok(UninstallReport { removed, skipped });
    }

    let targets = if opts.all_agents {
        all_targets()
    } else {
        select_targets(true)
    };

    for target in targets {
        let path = match target.path() {
            Some(p) => p,
            None => continue,
        };
        if opts.dry_run {
            eprintln!("[dry-run] would remove MCP entry from {}", path.display());
            removed.push(target.label().to_string());
            continue;
        }
        match remove_agent_config(&path, target.format) {
            Ok(true) => removed.push(target.label().to_string()),
            Ok(false) => skipped.push(format!("{} (not configured)", target.label())),
            Err(e) => skipped.push(format!("{} ({e})", target.label())),
        }
    }

    if opts.dry_run {
        eprintln!("[dry-run] would remove hooks from Claude/Codex configs");
        eprintln!("[dry-run] would remove {}", hooks_dir().display());
    } else {
        if remove_claude_hooks().is_ok() {
            removed.push("Claude hooks".into());
        }
        if remove_codex_hooks().is_ok() {
            removed.push("Codex hooks".into());
        }
        let _ = fs::remove_dir_all(hooks_dir());
        if !opts.keep_binary {
            let bin = installed_binary_path();
            if bin.exists() {
                let _ = fs::remove_file(&bin);
                removed.push("binary".into());
            }
        }
    }

    for line in &removed {
        eprintln!("removed: {line}");
    }
    for line in &skipped {
        eprintln!("skipped: {line}");
    }

    Ok(UninstallReport { removed, skipped })
}

fn resolve_source_binary(override_path: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = override_path {
        if path.is_file() {
            return Ok(path.to_path_buf());
        }
        return Err(Error::Other(format!(
            "binary not found: {}",
            path.display()
        )));
    }
    let current = std::env::current_exe()?;
    if current.is_file() {
        return Ok(current);
    }
    Err(Error::Other("could not resolve cbm binary path".into()))
}

fn install_binary(source: &Path, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    if dest.exists() {
        let backup = dest.with_extension("old");
        let _ = fs::remove_file(&backup);
        if fs::rename(dest, &backup).is_err() {
            fs::copy(source, dest)?;
            return Ok(());
        }
    }
    fs::copy(source, dest)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(dest)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(dest, perms)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct AgentTarget {
    kind: AgentKind,
    config_path: &'static str,
    format: ConfigFormat,
    create_if_missing: bool,
}

#[derive(Debug, Clone, Copy)]
enum ConfigFormat {
    OpenCode,
    CodexToml,
    McpServersJson,
    FallbackJson,
}

impl AgentTarget {
    fn label(&self) -> &'static str {
        match self.kind {
            AgentKind::OpenCode => "OpenCode",
            AgentKind::Codex => "Codex",
            AgentKind::ClaudeCode => "Claude Code",
            AgentKind::GeminiCli => "Gemini CLI",
            AgentKind::Zed => "Zed",
            AgentKind::Aider => "Aider",
            _ => "fallback",
        }
    }

    fn path(&self) -> Option<PathBuf> {
        dirs::home_dir().map(|home| home.join(self.config_path))
    }
}

fn all_targets() -> Vec<AgentTarget> {
    vec![
        AgentTarget {
            kind: AgentKind::OpenCode,
            config_path: ".config/opencode/opencode.jsonc",
            format: ConfigFormat::OpenCode,
            create_if_missing: true,
        },
        AgentTarget {
            kind: AgentKind::OpenCode,
            config_path: ".config/opencode/opencode.json",
            format: ConfigFormat::OpenCode,
            create_if_missing: false,
        },
        AgentTarget {
            kind: AgentKind::Codex,
            config_path: ".codex/config.toml",
            format: ConfigFormat::CodexToml,
            create_if_missing: true,
        },
        AgentTarget {
            kind: AgentKind::ClaudeCode,
            config_path: ".claude/settings.json",
            format: ConfigFormat::McpServersJson,
            create_if_missing: true,
        },
        AgentTarget {
            kind: AgentKind::GeminiCli,
            config_path: ".gemini/settings.json",
            format: ConfigFormat::McpServersJson,
            create_if_missing: true,
        },
        AgentTarget {
            kind: AgentKind::Zed,
            config_path: ".config/zed/settings.json",
            format: ConfigFormat::McpServersJson,
            create_if_missing: true,
        },
        AgentTarget {
            kind: AgentKind::Unknown,
            config_path: ".config/cbm/mcp.json",
            format: ConfigFormat::FallbackJson,
            create_if_missing: true,
        },
    ]
}

fn select_targets(all_agents: bool) -> Vec<AgentTarget> {
    if all_agents {
        return all_targets();
    }
    let detected = AgentKind::detect();
    all_targets()
        .into_iter()
        .filter(|t| {
            t.kind == detected
                || t.kind == AgentKind::Unknown
                || t.path().is_some_and(|path| path.exists())
        })
        .collect()
}

fn configure_agent(target: &AgentTarget, binary: &Path, opts: &InstallOptions) -> Result<bool> {
    let path = target
        .path()
        .ok_or_else(|| Error::Other("home directory not found".into()))?;

    if !path.exists() && !target.create_if_missing {
        return Ok(false);
    }

    if !path.exists() && !opts.force {
        if opts.dry_run {
            eprintln!(
                "[dry-run] would create {} for {}",
                path.display(),
                target.label()
            );
            return Ok(true);
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
    }

    if !opts.yes
        && !opts.dry_run
        && path.exists()
        && !opts.force
        && !confirm(&format!(
            "update MCP config at {} for {}?",
            path.display(),
            target.label()
        ))?
    {
        return Ok(false);
    }

    if opts.dry_run {
        eprintln!(
            "[dry-run] would write {} MCP entry to {}",
            MCP_SERVER_NAME,
            path.display()
        );
        return Ok(true);
    }

    match target.format {
        ConfigFormat::OpenCode => write_opencode_config(&path, binary, target.kind),
        ConfigFormat::CodexToml => write_codex_config(&path, binary, target.kind),
        ConfigFormat::McpServersJson => write_mcp_servers_json(&path, binary, target.kind),
        ConfigFormat::FallbackJson => write_fallback_config(&path, binary, target.kind),
    }
}

fn confirm(prompt: &str) -> Result<bool> {
    eprint!("{prompt} [y/N] ");
    io::stderr().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(matches!(line.trim().to_lowercase().as_str(), "y" | "yes"))
}

fn mcp_env(agent: AgentKind) -> Map<String, Value> {
    let mut env = Map::new();
    env.insert("CBM_PROJECT_PREFIX".into(), json!("cbm+"));
    env.insert("CBM_AGENT".into(), json!(agent.slug()));
    env
}

fn opencode_command(binary: &Path) -> Vec<Value> {
    vec![json!(binary.to_string_lossy().to_string())]
}

fn write_opencode_config(path: &Path, binary: &Path, agent: AgentKind) -> Result<bool> {
    let mut root: Map<String, Value> = if path.exists() {
        parse_json_config(&fs::read_to_string(path)?)?
    } else {
        Map::new()
    };
    let mcp = root
        .entry("mcp")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| Error::Other("opencode mcp field is not an object".into()))?;

    let key = if mcp.contains_key(MCP_SERVER_NAME) {
        MCP_SERVER_NAME
    } else if mcp.contains_key("cbm") {
        "cbm"
    } else {
        MCP_SERVER_NAME
    };

    mcp.insert(
        key.into(),
        json!({
            "type": "local",
            "command": opencode_command(binary),
            "enabled": true,
            "timeout": 120000,
            "environment": Value::Object(mcp_env(agent)),
        }),
    );

    write_json_pretty(path, &Value::Object(root))
}

fn write_mcp_servers_json(path: &Path, binary: &Path, agent: AgentKind) -> Result<bool> {
    let mut root: Map<String, Value> = if path.exists() {
        parse_json_config(&fs::read_to_string(path)?)?
    } else {
        Map::new()
    };
    let servers = root
        .entry("mcpServers")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| Error::Other("mcpServers field is not an object".into()))?;

    servers.insert(
        MCP_SERVER_NAME.into(),
        json!({
            "command": binary.to_string_lossy(),
            "args": [],
            "env": Value::Object(mcp_env(agent)),
        }),
    );

    write_json_pretty(path, &Value::Object(root))
}

fn write_fallback_config(path: &Path, binary: &Path, agent: AgentKind) -> Result<bool> {
    let snippet = json!({
        "mcpServers": {
            MCP_SERVER_NAME: {
                "command": binary.to_string_lossy(),
                "args": [],
                "env": Value::Object(mcp_env(agent)),
            }
        }
    });
    write_json_pretty(path, &snippet)
}

fn remove_codex_mcp_section(content: &str, server: &str) -> String {
    let header = format!("[mcp_servers.{server}]");
    let env_header = format!("[mcp_servers.{server}.env]");
    let lines: Vec<&str> = content.lines().collect();
    let mut remove = vec![false; lines.len()];

    for (idx, line) in lines.iter().enumerate() {
        if line.trim() != header {
            continue;
        }
        let mut end = idx + 1;
        while end < lines.len() && !lines[end].trim().starts_with('[') {
            end += 1;
        }
        if end < lines.len() && lines[end].trim() == env_header {
            end += 1;
            while end < lines.len() && !lines[end].trim().starts_with('[') {
                end += 1;
            }
        }
        for slot in &mut remove[idx..end] {
            *slot = true;
        }
    }

    let mut result: String = lines
        .iter()
        .zip(remove.iter())
        .filter_map(|(line, drop)| if *drop { None } else { Some(*line) })
        .collect::<Vec<_>>()
        .join("\n");
    if content.ends_with('\n') && !result.is_empty() {
        result.push('\n');
    }
    result
}

fn write_codex_config(path: &Path, binary: &Path, agent: AgentKind) -> Result<bool> {
    let content = if path.exists() {
        fs::read_to_string(path)?
    } else {
        String::new()
    };
    let content = remove_codex_mcp_section(&content, MCP_SERVER_NAME);

    let section_header = format!("[mcp_servers.{MCP_SERVER_NAME}]");
    let bin = binary.to_string_lossy().replace('\\', "/");
    let block = format!(
        "\n{section_header}\ncommand = \"{bin}\"\nargs = []\n\n[mcp_servers.{MCP_SERVER_NAME}.env]\nCBM_PROJECT_PREFIX = \"cbm+\"\nCBM_AGENT = \"{}\"\n",
        agent.slug()
    );
    let content = content.trim_end().to_string() + &block;
    fs::write(path, content)?;
    Ok(true)
}

const HOOK_GATE_PS1: &str = include_str!("../../hooks/cbm-code-discovery-gate.ps1");
const HOOK_GATE_SH: &str = include_str!("../../hooks/cbm-code-discovery-gate.sh");
const HOOK_SESSION_PS1: &str = include_str!("../../hooks/cbm-session-reminder.ps1");
const HOOK_SESSION_SH: &str = include_str!("../../hooks/cbm-session-reminder.sh");

fn install_hooks(binary: &Path, opts: &InstallOptions) -> Result<bool> {
    if opts.dry_run {
        eprintln!(
            "[dry-run] would install hook scripts to {}",
            hooks_dir().display()
        );
        configure_claude_hooks(binary, opts)?;
        configure_codex_hooks(opts)?;
        return Ok(true);
    }

    let bin_str = binary.to_string_lossy().replace('\\', "/");
    for (name, template) in [
        ("cbm-code-discovery-gate.ps1", HOOK_GATE_PS1),
        ("cbm-session-reminder.ps1", HOOK_SESSION_PS1),
        ("cbm-code-discovery-gate.sh", HOOK_GATE_SH),
        ("cbm-session-reminder.sh", HOOK_SESSION_SH),
    ] {
        let content = template.replace("{{CBM_BIN}}", &bin_str);
        let dest = hooks_dir().join(name);
        fs::create_dir_all(hooks_dir())?;
        fs::write(&dest, content)?;
        #[cfg(unix)]
        if name.ends_with(".sh") {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&dest)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&dest, perms)?;
        }
    }

    let claude_dir = claude_hooks_dir();
    fs::create_dir_all(&claude_dir)?;
    for (name, template) in [
        ("cbm-code-discovery-gate.ps1", HOOK_GATE_PS1),
        ("cbm-session-reminder.ps1", HOOK_SESSION_PS1),
        ("cbm-code-discovery-gate", HOOK_GATE_SH),
        ("cbm-session-reminder", HOOK_SESSION_SH),
    ] {
        let content = template.replace("{{CBM_BIN}}", &bin_str);
        let dest = claude_dir.join(name);
        fs::write(&dest, content)?;
        #[cfg(unix)]
        if !name.ends_with(".ps1") {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&dest)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&dest, perms)?;
        }
    }

    configure_claude_hooks(binary, opts)?;
    configure_codex_hooks(opts)?;
    Ok(true)
}

fn configure_claude_hooks(binary: &Path, opts: &InstallOptions) -> Result<()> {
    let settings = claude_settings_path();
    if opts.dry_run {
        eprintln!(
            "[dry-run] would configure Claude hooks in {}",
            settings.display()
        );
        return Ok(());
    }
    let gate = hook_command(binary, "cbm-code-discovery-gate");
    let session = hook_command(binary, "cbm-session-reminder");
    upsert_claude_hooks(&settings, &gate, &session)
}

fn configure_codex_hooks(opts: &InstallOptions) -> Result<()> {
    let config = dirs::home_dir()
        .map(|h| h.join(".codex").join("config.toml"))
        .ok_or_else(|| Error::Other("home directory not found".into()))?;
    if opts.dry_run {
        eprintln!(
            "[dry-run] would configure Codex SessionStart hooks in {}",
            config.display()
        );
        return Ok(());
    }
    if config.exists() {
        upsert_codex_session_hooks(&config)?;
    }
    Ok(())
}

fn claude_settings_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
        .join("settings.json")
}

fn hook_command(_binary: &Path, script_base: &str) -> String {
    if cfg!(windows) {
        let script = claude_hooks_dir().join(format!("{script_base}.ps1"));
        format!("pwsh -NoProfile -File \"{}\"", script.display())
    } else {
        claude_hooks_dir().join(script_base).display().to_string()
    }
}

fn upsert_claude_hooks(settings_path: &Path, gate_cmd: &str, session_cmd: &str) -> Result<()> {
    let mut root: Map<String, Value> = if settings_path.exists() {
        serde_json::from_str(&fs::read_to_string(settings_path)?)?
    } else {
        Map::new()
    };
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| Error::Other("hooks field is not an object".into()))?;

    let pre: Vec<Value> = hooks
        .get("PreToolUse")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter(|entry| {
                    let cmd = entry
                        .get("hooks")
                        .and_then(|h| h.as_array())
                        .and_then(|a| a.first())
                        .and_then(|h| h.get("command"))
                        .and_then(|c| c.as_str())
                        .unwrap_or("");
                    !cmd.contains("cbm-code-discovery-gate")
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let mut pre = pre;
    pre.push(json!({
        "matcher": "Grep|Glob",
        "hooks": [{
            "type": "command",
            "command": gate_cmd,
            "timeout": 5
        }]
    }));
    hooks.insert("PreToolUse".into(), Value::Array(pre));

    let session: Vec<Value> = hooks
        .get("SessionStart")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter(|entry| {
                    let cmd = entry
                        .get("hooks")
                        .and_then(|h| h.as_array())
                        .and_then(|a| a.first())
                        .and_then(|h| h.get("command"))
                        .and_then(|c| c.as_str())
                        .unwrap_or("");
                    !cmd.contains("cbm-session-reminder")
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let mut session = session;
    for matcher in ["startup", "resume", "clear", "compact"] {
        session.push(json!({
            "matcher": matcher,
            "hooks": [{
                "type": "command",
                "command": session_cmd
            }]
        }));
    }
    hooks.insert("SessionStart".into(), Value::Array(session));

    if let Some(parent) = settings_path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_json_pretty(settings_path, &Value::Object(root))?;
    Ok(())
}

fn upsert_codex_session_hooks(config_path: &Path) -> Result<()> {
    let mut content = fs::read_to_string(config_path)?;
    let block = format!(
        "\n{CODEX_HOOK_BEGIN}\n[[hooks.SessionStart]]\nmatcher = \"startup|resume|clear|compact\"\n\n[[hooks.SessionStart.hooks]]\ntype = \"command\"\ncommand = '{CODEX_SESSION_REMINDER_CMD}'\n{CODEX_HOOK_END}\n"
    );
    content = remove_codex_hook_block(&content);
    content = content.trim_end().to_string() + &block;
    fs::write(config_path, content)?;
    Ok(())
}

fn remove_codex_hook_block(content: &str) -> String {
    let begin = regex::escape(CODEX_HOOK_BEGIN);
    let end = regex::escape(CODEX_HOOK_END);
    let pattern = format!(r"(?s)\n?{begin}.*?{end}\n?");
    regex::Regex::new(&pattern)
        .map(|re| re.replace(content, "").to_string())
        .unwrap_or_else(|_| content.to_string())
}

fn remove_claude_hooks() -> Result<()> {
    let path = claude_settings_path();
    if !path.exists() {
        return Ok(());
    }
    let mut root: Map<String, Value> = serde_json::from_str(&fs::read_to_string(&path)?)?;
    let Some(hooks) = root.get_mut("hooks").and_then(|v| v.as_object_mut()) else {
        return Ok(());
    };
    if let Some(pre) = hooks.get_mut("PreToolUse").and_then(|v| v.as_array_mut()) {
        pre.retain(|entry| {
            let cmd = entry
                .get("hooks")
                .and_then(|h| h.as_array())
                .and_then(|a| a.first())
                .and_then(|h| h.get("command"))
                .and_then(|c| c.as_str())
                .unwrap_or("");
            !cmd.contains("cbm-code-discovery-gate")
        });
    }
    if let Some(session) = hooks.get_mut("SessionStart").and_then(|v| v.as_array_mut()) {
        session.retain(|entry| {
            let cmd = entry
                .get("hooks")
                .and_then(|h| h.as_array())
                .and_then(|a| a.first())
                .and_then(|h| h.get("command"))
                .and_then(|c| c.as_str())
                .unwrap_or("");
            !cmd.contains("cbm-session-reminder")
        });
    }
    write_json_pretty(&path, &Value::Object(root))?;
    Ok(())
}

fn remove_codex_hooks() -> Result<()> {
    let config = dirs::home_dir()
        .map(|h| h.join(".codex").join("config.toml"))
        .ok_or_else(|| Error::Other("home directory not found".into()))?;
    if !config.exists() {
        return Ok(());
    }
    let content = remove_codex_hook_block(&fs::read_to_string(&config)?);
    fs::write(&config, content)?;
    Ok(())
}

fn remove_agent_config(path: &Path, format: ConfigFormat) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    match format {
        ConfigFormat::CodexToml => {
            let content = remove_codex_mcp_section(&fs::read_to_string(path)?, MCP_SERVER_NAME);
            fs::write(path, content)?;
            Ok(true)
        }
        ConfigFormat::OpenCode => {
            let mut root: Map<String, Value> = parse_json_config(&fs::read_to_string(path)?)?;
            let removed = root
                .get_mut("mcp")
                .and_then(|v| v.as_object_mut())
                .map(|mcp| {
                    let primary = mcp.remove(MCP_SERVER_NAME).is_some();
                    let alias = mcp.remove("cbm").is_some();
                    primary || alias
                })
                .unwrap_or(false);
            if removed {
                write_json_pretty(path, &Value::Object(root))?;
            }
            Ok(removed)
        }
        ConfigFormat::McpServersJson | ConfigFormat::FallbackJson => {
            let mut root: Map<String, Value> = parse_json_config(&fs::read_to_string(path)?)?;
            let removed = root
                .get_mut("mcpServers")
                .and_then(|v| v.as_object_mut())
                .map(|mcp| {
                    let primary = mcp.remove(MCP_SERVER_NAME).is_some();
                    let alias = mcp.remove("cbm").is_some();
                    primary || alias
                })
                .unwrap_or(false);
            if removed {
                write_json_pretty(path, &Value::Object(root))?;
            }
            Ok(removed)
        }
    }
}

fn parse_json_config(content: &str) -> Result<Map<String, Value>> {
    match serde_json::from_str::<Value>(content) {
        Ok(Value::Object(root)) => Ok(root),
        Ok(_) => Err(Error::Other("config root is not an object".into())),
        Err(first) => match serde_json::from_str::<Value>(&normalize_jsonc(content)) {
            Ok(Value::Object(root)) => Ok(root),
            Ok(_) => Err(Error::Other("config root is not an object".into())),
            Err(_) => Err(first.into()),
        },
    }
}

fn normalize_jsonc(content: &str) -> String {
    strip_trailing_json_commas(&strip_json_comments(content))
}

fn strip_json_comments(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let mut chars = content.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;
    while let Some(ch) = chars.next() {
        if in_string {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        if ch == '"' {
            in_string = true;
            out.push(ch);
            continue;
        }

        if ch == '/' {
            match chars.peek().copied() {
                Some('/') => {
                    chars.next();
                    for next in chars.by_ref() {
                        if next == '\n' {
                            out.push('\n');
                            break;
                        }
                    }
                    continue;
                }
                Some('*') => {
                    chars.next();
                    let mut prev = '\0';
                    for next in chars.by_ref() {
                        if prev == '*' && next == '/' {
                            break;
                        }
                        prev = next;
                    }
                    continue;
                }
                _ => {}
            }
        }

        out.push(ch);
    }
    out
}

fn strip_trailing_json_commas(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let mut chars = content.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;
    while let Some(ch) = chars.next() {
        if in_string {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        if ch == '"' {
            in_string = true;
            out.push(ch);
            continue;
        }

        if ch == ',' {
            let mut lookahead = chars.clone();
            while matches!(lookahead.peek(), Some(c) if c.is_whitespace()) {
                lookahead.next();
            }
            if matches!(lookahead.peek(), Some('}' | ']')) {
                continue;
            }
        }

        out.push(ch);
    }
    out
}

fn write_json_pretty(path: &Path, value: &Value) -> Result<bool> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(value)? + "\n";
    fs::write(path, text)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn merges_opencode_mcp_entry() {
        let dir = TempDir::new().unwrap();
        let cfg = dir.path().join("opencode.jsonc");
        fs::write(&cfg, r#"{"model":"test"}"#).unwrap();
        let bin = dir.path().join("cbm.exe");
        fs::write(&bin, b"").unwrap();

        write_opencode_config(&cfg, &bin, AgentKind::OpenCode).unwrap();
        let parsed: Value = serde_json::from_str(&fs::read_to_string(&cfg).unwrap()).unwrap();
        assert!(parsed["mcp"][MCP_SERVER_NAME]["enabled"].as_bool().unwrap());
        assert_eq!(parsed["model"], "test");
    }

    #[test]
    fn updates_existing_opencode_cbm_alias_from_jsonc() {
        let dir = TempDir::new().unwrap();
        let cfg = dir.path().join("opencode.jsonc");
        fs::write(
            &cfg,
            r#"{
  // OpenCode accepts jsonc; keep parsing tolerant.
  "mcp": {
    "cbm": {
      "type": "local",
      "command": ["C:\\repo\\target\\release\\cbm.exe"],
      "enabled": true,
      "timeout": 120000
    }
  },
}"#,
        )
        .unwrap();
        let bin = dir.path().join("stable").join("cbm.exe");
        fs::create_dir_all(bin.parent().unwrap()).unwrap();
        fs::write(&bin, b"").unwrap();

        write_opencode_config(&cfg, &bin, AgentKind::OpenCode).unwrap();
        let parsed: Value = serde_json::from_str(&fs::read_to_string(&cfg).unwrap()).unwrap();
        assert!(parsed["mcp"]["cbm"].is_object());
        assert!(parsed["mcp"].get(MCP_SERVER_NAME).is_none());
        assert_eq!(
            parsed["mcp"]["cbm"]["command"][0].as_str(),
            Some(bin.to_string_lossy().as_ref())
        );
        assert_eq!(
            parsed["mcp"]["cbm"]["environment"]["CBM_AGENT"].as_str(),
            Some("opencode")
        );
    }

    #[test]
    fn removes_existing_codex_section() {
        let input = "model = \"gpt\"\n\n[mcp_servers.cbm-mcp]\ncommand = \"old\"\n\n[features]\nhooks = true\n";
        let out = remove_codex_mcp_section(input, MCP_SERVER_NAME);
        assert!(!out.contains("old"));
        assert!(!out.contains("[mcp_servers.cbm-mcp]"));
        assert!(out.contains("model = \"gpt\""));
        assert!(out.contains("[features]"));
    }

    #[test]
    fn merges_codex_toml_section() {
        let dir = TempDir::new().unwrap();
        let cfg = dir.path().join("config.toml");
        fs::write(&cfg, "model = \"gpt\"\n").unwrap();
        let bin = dir.path().join("cbm");
        fs::write(&bin, b"").unwrap();

        write_codex_config(&cfg, &bin, AgentKind::Codex).unwrap();
        let text = fs::read_to_string(&cfg).unwrap();
        assert!(text.contains(&format!("[mcp_servers.{MCP_SERVER_NAME}]")));
        assert!(text.contains("CBM_PROJECT_PREFIX"));
        assert!(text.contains("model = \"gpt\""));
    }

    #[test]
    fn merges_claude_mcp_servers() {
        let dir = TempDir::new().unwrap();
        let cfg = dir.path().join("settings.json");
        fs::write(&cfg, r#"{"hooks":{}}"#).unwrap();
        let bin = dir.path().join("cbm.exe");
        fs::write(&bin, b"").unwrap();

        write_mcp_servers_json(&cfg, &bin, AgentKind::ClaudeCode).unwrap();
        let parsed: Value = serde_json::from_str(&fs::read_to_string(&cfg).unwrap()).unwrap();
        assert!(parsed["mcpServers"][MCP_SERVER_NAME].is_object());
        assert!(parsed["hooks"].is_object());
    }

    #[test]
    fn default_install_dir_under_config() {
        let dir = default_install_dir();
        assert!(dir.to_string_lossy().contains("cbm"));
        assert!(dir.ends_with("bin"));
    }
}
