//! Process-level CLI contract tests (Section 7.3).

use assert_cmd::Command;
use serde_json::Value;

fn cbrlm() -> Command {
    Command::cargo_bin("cbrlm").unwrap()
}

fn parse_stdout_json(out: &[u8]) -> Option<Value> {
    let text = std::str::from_utf8(out).ok()?.trim();
    serde_json::from_str(text).ok()
}

#[test]
fn list_projects_json_quiet_stdout_is_parseable() {
    let output = cbrlm()
        .args(["cli", "list_projects", "--json", "--quiet"])
        .output()
        .expect("spawn cbrlm");
    assert!(output.status.success(), "exit failed: {:?}", output);
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty under --quiet, got: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let parsed = parse_stdout_json(&output.stdout).expect("stdout is not valid JSON");
    assert!(parsed.get("projects").is_some());
}

#[test]
fn index_repository_json_stdout_parseable_with_stderr_diagnostics() {
    let json = r#"{"repo_path":".","project":"cli-process","mode":"fast","persistence":false}"#;
    let output = cbrlm()
        .args(["cli", "index_repository", "--json", json])
        .output()
        .expect("spawn cbrlm");
    assert!(output.status.success());
    let parsed = parse_stdout_json(&output.stdout).expect("stdout is not valid JSON");
    assert_eq!(parsed.get("success").and_then(|v| v.as_bool()), Some(true));
}

#[test]
fn json_without_quiet_keeps_stdout_parseable() {
    let json = r#"{"repo_path":".","project":"cli-process-2","mode":"fast","persistence":false}"#;
    let output = cbrlm()
        .args(["cli", "index_repository", "--json", json])
        .output()
        .expect("spawn cbrlm");
    assert!(output.status.success());
    assert!(parse_stdout_json(&output.stdout).is_some());
}
