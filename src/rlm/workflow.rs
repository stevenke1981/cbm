use serde_json::{json, Value};

pub fn workflow_guidance(phase: &str) -> Value {
    match phase {
        "filter" => json!({
            "phase": "filter",
            "description": "RLM Phase 1 — narrow context via graph search",
            "tools": ["rlm_filter", "search_graph", "search_code"],
            "steps": [
                "index_repository if not indexed",
                "rlm_filter or search_graph with query + label",
                "collect qualified_names for map phase"
            ],
            "rules": [
                "Never load 10+ files into root context",
                "Prefer graph tools over rg when indexed"
            ]
        }),
        "map" => json!({
            "phase": "map",
            "description": "RLM Phase 2 — parallel symbol reads",
            "tools": ["rlm_read_symbol", "trace_path", "get_code_snippet", "rlm_chunk"],
            "steps": [
                "rlm_read_symbol — one qualified_name per call",
                "trace_path for call chains (direction=both, depth=3)",
                "rlm_chunk for log/CSV blobs (non-code only)"
            ],
            "rules": [
                "One symbol per rlm_read_symbol call",
                "Use trace_path before reading unrelated files"
            ]
        }),
        "reduce" => json!({
            "phase": "reduce",
            "description": "RLM Phase 3 — synthesize architecture summary",
            "tools": ["get_architecture", "detect_changes", "query_graph"],
            "steps": [
                "get_architecture for project overview",
                "detect_changes for git delta",
                "Reduce to structured JSON before final answer"
            ]
        }),
        _ => json!({
            "phase": "overview",
            "description": "Recursive Language Model workflow for large codebases",
            "paper": "https://arxiv.org/pdf/2512.24601",
            "phases": ["filter", "map", "reduce"],
            "prerequisite": "index_repository(repo_path='.')",
            "project_naming": "CBRLM indexes use cbrlm+ prefix",
            "agents": ["opencode", "codex", "claude-code", "gemini-cli", "zed", "aider"],
            "loop": "filter → map (parallel) → reduce"
        }),
    }
}