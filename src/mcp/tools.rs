use crate::discover::IndexMode;
use crate::error::{Error, Result};
use crate::git;
use crate::pipeline::Pipeline;
use crate::project::normalize_project_name;
use crate::rlm::RlmEngine;
use crate::semantic;
use crate::store::{delete_project_db, SearchFilter, Store};
use crate::watcher::Watcher;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

pub struct ToolHandler {
    rlm: Arc<RlmEngine>,
    watcher: Option<Arc<Watcher>>,
}

impl ToolHandler {
    pub fn new(rlm: Arc<RlmEngine>, watcher: Option<Arc<Watcher>>) -> Self {
        Self { rlm, watcher }
    }

    pub fn handle(&self, name: &str, args: &Value) -> Result<Value> {
        match name {
            "index_repository" => self.index_repository(args),
            "search_graph" => self.search_graph(args),
            "trace_path" => self.trace_path(args),
            "get_code_snippet" => self.get_code_snippet(args),
            "get_graph_schema" => Ok(json!(Store::open_memory()?.get_schema())),
            "get_architecture" => self.get_architecture(args),
            "search_code" => self.search_code(args),
            "list_projects" => self.list_projects(),
            "delete_project" => self.delete_project(args),
            "index_status" => self.index_status(args),
            "query_graph" => self.query_graph(args),
            "detect_changes" => self.detect_changes(args),
            "manage_adr" => self.manage_adr(args),
            "ingest_traces" => self.ingest_traces(args),
            "rlm_workflow" => {
                let phase = args
                    .get("phase")
                    .and_then(|v| v.as_str())
                    .unwrap_or("overview");
                Ok(self.rlm.workflow(phase))
            }
            "rlm_filter" => self.rlm_filter(args),
            "rlm_read_symbol" => self.rlm_read_symbol(args),
            "rlm_scan" => self.rlm_scan(args),
            "rlm_chunk" => self.rlm_chunk(args),
            "rlm_peek" => self.rlm_peek(args),
            "rlm_session_list" => Ok(self.rlm.session_list()),
            "rlm_session_delete" => self.rlm_session_delete(args),
            _ => Err(Error::InvalidArgument(format!("unknown tool: {name}"))),
        }
    }

    fn require_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
        args.get(key)
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::InvalidArgument(format!("missing {key}")))
    }

    fn index_repository(&self, args: &Value) -> Result<Value> {
        let repo_path = Self::require_str(args, "repo_path")?;
        let mode = args.get("mode").and_then(|v| v.as_str()).unwrap_or("full");
        let project = args.get("project").and_then(|v| v.as_str());
        let incremental = args
            .get("incremental")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let persistence = args
            .get("persistence")
            .and_then(|v| v.as_bool())
            .unwrap_or_else(crate::persistence::env_enabled);
        let pipeline = Pipeline::new(IndexMode::parse(mode)).set_export_artifact(persistence);
        let path = std::path::Path::new(repo_path);
        let _guard = self
            .watcher
            .as_ref()
            .map(|w| PipelineGuard::new(w.pipeline_busy()));
        let result = if incremental {
            pipeline.run_smart(path, project, true)?
        } else {
            pipeline.run(path, project)?
        };

        let project_name = &result.project;
        if let Some(w) = &self.watcher {
            w.register(
                project_name,
                path.canonicalize().unwrap_or_else(|_| path.to_path_buf()),
            );
        }

        Ok(serde_json::to_value(result)?)
    }

    fn search_graph(&self, args: &Value) -> Result<Value> {
        let project = normalize_project_name(Self::require_str(args, "project")?);
        let store = Store::open(&project)?;

        if let Some(vector_query) = args
            .get("vector_query")
            .or_else(|| args.get("semantic_query"))
            .and_then(|v| v.as_str())
        {
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;
            let result = semantic::vector_search(&store, vector_query, limit)?;
            return Ok(serde_json::to_value(result)?);
        }

        let filter = parse_search_filter(args);
        let result = store.search(&filter)?;
        Ok(serde_json::to_value(result)?)
    }

    fn rlm_filter(&self, args: &Value) -> Result<Value> {
        let project = Self::require_str(args, "project")?;
        let filter = parse_search_filter(args);
        self.rlm.filter(project, filter)
    }

    fn trace_path(&self, args: &Value) -> Result<Value> {
        let project = normalize_project_name(Self::require_str(args, "project")?);
        let function_name = Self::require_str(args, "function_name")?;
        let direction = args
            .get("direction")
            .and_then(|v| v.as_str())
            .unwrap_or("both");
        let depth = args.get("depth").and_then(|v| v.as_u64()).unwrap_or(3) as usize;
        let store = Store::open(&project)?;
        let result = store.trace_path(function_name, direction, depth)?;
        Ok(serde_json::to_value(result)?)
    }

    fn get_code_snippet(&self, args: &Value) -> Result<Value> {
        let project = args
            .get("project")
            .and_then(|v| v.as_str())
            .map(normalize_project_name)
            .unwrap_or_default();
        let qn = Self::require_str(args, "qualified_name")?;
        let store = if project.is_empty() {
            find_symbol_any_project(qn)?
        } else {
            Store::open(&project)?
        };
        let snippet = store.get_snippet(qn)?;
        Ok(serde_json::to_value(snippet)?)
    }

    fn rlm_read_symbol(&self, args: &Value) -> Result<Value> {
        let project = Self::require_str(args, "project")?;
        let qn = Self::require_str(args, "qualified_name")?;
        self.rlm.read_symbol(project, qn)
    }

    fn get_architecture(&self, args: &Value) -> Result<Value> {
        let project = normalize_project_name(Self::require_str(args, "project")?);
        let store = Store::open(&project)?;
        let arch = store.get_architecture()?;
        Ok(serde_json::to_value(arch)?)
    }

    fn search_code(&self, args: &Value) -> Result<Value> {
        let project = normalize_project_name(Self::require_str(args, "project")?);
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .or_else(|| args.get("query").and_then(|v| v.as_str()))
            .unwrap_or("");
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;
        let store = Store::open(&project)?;
        let matches = store.search_code(pattern, limit)?;
        Ok(json!({ "matches": matches }))
    }

    fn list_projects(&self) -> Result<Value> {
        let projects = Store::list_projects()?;
        Ok(json!({ "projects": projects }))
    }

    fn delete_project(&self, args: &Value) -> Result<Value> {
        let project = normalize_project_name(Self::require_str(args, "project")?);
        if let Ok(store) = Store::open(&project) {
            store.delete_project()?;
        }
        delete_project_db(&project)?;
        Ok(json!({ "deleted": project }))
    }

    fn index_status(&self, args: &Value) -> Result<Value> {
        let project = normalize_project_name(Self::require_str(args, "project")?);
        let store = Store::open(&project)?;
        let status = store.index_status()?;
        let mut value = serde_json::to_value(status)?;
        if let Some(watcher) = &self.watcher {
            let projects = watcher.project_status();
            if let Some(w) = projects.iter().find(|p| p.project == project) {
                if let Some(obj) = value.as_object_mut() {
                    obj.insert("watcher".into(), serde_json::to_value(w)?);
                }
            }
        }
        Ok(value)
    }

    fn query_graph(&self, args: &Value) -> Result<Value> {
        let query = Self::require_str(args, "query")?;
        let project = args
            .get("project")
            .and_then(|v| v.as_str())
            .map(normalize_project_name);
        let store = match project {
            Some(p) => Store::open(&p)?,
            None => Store::open_memory()?,
        };
        let result = store.query_select(query)?;
        Ok(serde_json::to_value(result)?)
    }

    fn detect_changes(&self, args: &Value) -> Result<Value> {
        let project = normalize_project_name(Self::require_str(args, "project")?);
        let store = Store::open(&project)?;
        let info = store.get_project()?;
        let repo = PathBuf::from(&info.repo_path);
        let indexed_head = store.get_meta("git_head")?;

        match git::status(&repo) {
            Ok(st) => Ok(json!({
                "project": project,
                "dirty": st.dirty,
                "head": st.head,
                "indexed_head": indexed_head,
                "head_changed": indexed_head.as_ref().zip(st.head.as_ref()).map(|(a, b)| a != b).unwrap_or(false),
                "changed_files": st.changed_files,
                "deleted_files": st.deleted_files,
            })),
            Err(e) => Ok(json!({
                "project": project,
                "dirty": false,
                "changed_files": [],
                "note": e.to_string()
            })),
        }
    }

    fn ingest_traces(&self, args: &Value) -> Result<Value> {
        let project = normalize_project_name(Self::require_str(args, "project")?);
        let store = Store::open(&project)?;
        let traces = args
            .get("traces")
            .and_then(|v| v.as_array())
            .ok_or_else(|| Error::InvalidArgument("traces array required".into()))?;

        let mut pairs = Vec::new();
        for item in traces {
            let src = item
                .get("caller")
                .or_else(|| item.get("from"))
                .or_else(|| item.get("src"))
                .and_then(|v| v.as_str());
            let dst = item
                .get("callee")
                .or_else(|| item.get("to"))
                .or_else(|| item.get("dst"))
                .and_then(|v| v.as_str());
            if let (Some(s), Some(d)) = (src, dst) {
                pairs.push((s.to_string(), d.to_string()));
            }
        }

        let ingested = store.ingest_traces(&pairs)?;
        Ok(json!({
            "success": true,
            "project": project,
            "ingested": ingested,
            "edge_type": "RUNTIME_TRACE"
        }))
    }

    fn manage_adr(&self, args: &Value) -> Result<Value> {
        let project = normalize_project_name(Self::require_str(args, "project")?);
        let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("get");
        let store = Store::open(&project)?;
        match action {
            "set" => {
                let content = Self::require_str(args, "content")?;
                store.set_adr(content)?;
                Ok(json!({ "action": "set", "length": content.len() }))
            }
            "delete" => {
                store.set_meta("adr", "")?;
                Ok(json!({ "action": "delete" }))
            }
            _ => {
                let adr = store.get_adr()?;
                Ok(json!({ "action": "get", "content": adr }))
            }
        }
    }

    fn rlm_scan(&self, args: &Value) -> Result<Value> {
        let path = Self::require_str(args, "path")?;
        self.rlm.scan(path)
    }

    fn rlm_chunk(&self, args: &Value) -> Result<Value> {
        let session_id = Self::require_str(args, "session_id")?;
        let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(3) as usize;
        self.rlm.chunk(session_id, offset, limit)
    }

    fn rlm_peek(&self, args: &Value) -> Result<Value> {
        let session_id = Self::require_str(args, "session_id")?;
        let query = Self::require_str(args, "query")?;
        self.rlm.peek(session_id, query)
    }

    fn rlm_session_delete(&self, args: &Value) -> Result<Value> {
        let session_id = Self::require_str(args, "session_id")?;
        self.rlm.session_delete(session_id)
    }
}

fn parse_search_filter(args: &Value) -> SearchFilter {
    SearchFilter {
        query: args
            .get("query")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        label: args
            .get("label")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        name_pattern: args
            .get("name_pattern")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        qn_pattern: args
            .get("qn_pattern")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        file_pattern: args
            .get("file_pattern")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        relationship: args
            .get("relationship")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        direction: args
            .get("direction")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        min_degree: args
            .get("min_degree")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize),
        max_degree: args
            .get("max_degree")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize),
        include_connected: args
            .get("include_connected")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        exclude_entry_points: args
            .get("exclude_entry_points")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        limit: args.get("limit").and_then(|v| v.as_u64()).unwrap_or(200) as usize,
        offset: args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize,
    }
}

fn find_symbol_any_project(qn: &str) -> Result<Store> {
    for project in Store::list_projects()? {
        let store = Store::open(&project.name)?;
        if store.find_symbol(qn)?.is_some() {
            return Ok(store);
        }
    }
    Err(Error::SymbolNotFound(qn.to_string()))
}

pub fn tool_definitions() -> Vec<Value> {
    vec![
        tool_def(
            "index_repository",
            "Index a repository into the knowledge graph.",
            json!({
                "type": "object",
                "required": ["repo_path"],
                "properties": {
                    "repo_path": { "type": "string" },
                    "project": { "type": ["string", "null"] },
                    "mode": { "type": ["string", "null"], "enum": ["full", "moderate", "fast"] },
                    "incremental": { "type": ["boolean", "null"], "default": false },
                    "persistence": { "type": ["boolean", "null"] }
                }
            }),
        ),
        tool_def(
            "search_graph",
            "Search the code knowledge graph.",
            search_schema(),
        ),
        tool_def(
            "trace_path",
            "Trace call paths via BFS.",
            json!({
                "type": "object",
                "required": ["project", "function_name"],
                "properties": {
                    "project": { "type": "string" },
                    "function_name": { "type": "string" },
                    "direction": { "type": "string", "default": "both" },
                    "depth": { "type": "integer", "default": 3 }
                }
            }),
        ),
        tool_def(
            "get_code_snippet",
            "Read source code for a symbol.",
            json!({
                "type": "object",
                "required": ["qualified_name"],
                "properties": {
                    "project": { "type": "string" },
                    "qualified_name": { "type": "string" }
                }
            }),
        ),
        tool_def(
            "get_graph_schema",
            "Return graph schema.",
            json!({ "type": "object", "properties": {} }),
        ),
        tool_def(
            "get_architecture",
            "Architecture overview.",
            json!({
                "type": "object",
                "required": ["project"],
                "properties": { "project": { "type": "string" } }
            }),
        ),
        tool_def(
            "search_code",
            "Full-text code search.",
            json!({
                "type": "object",
                "required": ["project"],
                "properties": {
                    "project": { "type": "string" },
                    "pattern": { "type": "string" },
                    "query": { "type": "string" },
                    "limit": { "type": "integer", "default": 20 }
                }
            }),
        ),
        tool_def(
            "list_projects",
            "List indexed projects.",
            json!({ "type": "object", "properties": {} }),
        ),
        tool_def(
            "delete_project",
            "Delete project index.",
            json!({
                "type": "object",
                "required": ["project"],
                "properties": { "project": { "type": "string" } }
            }),
        ),
        tool_def(
            "index_status",
            "Index status query.",
            json!({
                "type": "object",
                "required": ["project"],
                "properties": { "project": { "type": "string" } }
            }),
        ),
        tool_def(
            "query_graph",
            "SQL SELECT on graph tables.",
            json!({
                "type": "object",
                "required": ["query"],
                "properties": {
                    "query": { "type": "string" },
                    "project": { "type": "string" }
                }
            }),
        ),
        tool_def(
            "detect_changes",
            "Detect git changes.",
            json!({
                "type": "object",
                "required": ["project"],
                "properties": { "project": { "type": "string" } }
            }),
        ),
        tool_def(
            "manage_adr",
            "Architecture Decision Record CRUD.",
            json!({
                "type": "object",
                "required": ["project"],
                "properties": {
                    "project": { "type": "string" },
                    "action": { "type": "string", "enum": ["get", "set", "delete"] },
                    "content": { "type": "string" }
                }
            }),
        ),
        tool_def(
            "ingest_traces",
            "Ingest runtime traces as RUNTIME_TRACE edges.",
            json!({
                "type": "object",
                "required": ["project", "traces"],
                "properties": {
                    "project": { "type": "string" },
                    "traces": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "caller": { "type": "string" },
                                "callee": { "type": "string" },
                                "from": { "type": "string" },
                                "to": { "type": "string" }
                            }
                        }
                    }
                }
            }),
        ),
        tool_def(
            "rlm_workflow",
            "RLM workflow guidance.",
            json!({
                "type": "object",
                "properties": { "phase": { "type": "string", "default": "overview" } }
            }),
        ),
        tool_def(
            "rlm_filter",
            "RLM filter via graph search.",
            search_schema(),
        ),
        tool_def(
            "rlm_read_symbol",
            "RLM map unit — one symbol.",
            json!({
                "type": "object",
                "required": ["project", "qualified_name"],
                "properties": {
                    "project": { "type": "string" },
                    "qualified_name": { "type": "string" }
                }
            }),
        ),
        tool_def(
            "rlm_scan",
            "Scan directory into RLM session.",
            json!({
                "type": "object",
                "required": ["path"],
                "properties": { "path": { "type": "string" } }
            }),
        ),
        tool_def(
            "rlm_chunk",
            "Read RLM session chunks.",
            json!({
                "type": "object",
                "required": ["session_id"],
                "properties": {
                    "session_id": { "type": "string" },
                    "offset": { "type": "integer", "default": 0 },
                    "limit": { "type": "integer", "default": 3 }
                }
            }),
        ),
        tool_def(
            "rlm_peek",
            "Search within RLM session.",
            json!({
                "type": "object",
                "required": ["session_id", "query"],
                "properties": {
                    "session_id": { "type": "string" },
                    "query": { "type": "string" }
                }
            }),
        ),
        tool_def(
            "rlm_session_list",
            "List RLM scan sessions.",
            json!({ "type": "object", "properties": {} }),
        ),
        tool_def(
            "rlm_session_delete",
            "Delete RLM session.",
            json!({
                "type": "object",
                "required": ["session_id"],
                "properties": { "session_id": { "type": "string" } }
            }),
        ),
    ]
}

fn search_schema() -> Value {
    json!({
        "type": "object",
        "required": ["project"],
        "properties": {
            "project": { "type": "string" },
            "query": { "type": ["string", "null"] },
            "vector_query": { "type": ["string", "null"], "description": "Semantic vector search (requires CBM_SEMANTIC_ENABLED=1)" },
            "semantic_query": { "type": ["string", "null"] },
            "label": { "type": ["string", "null"] },
            "name_pattern": { "type": ["string", "null"], "description": "Regex pattern matched against symbol name" },
            "qn_pattern": { "type": ["string", "null"], "description": "Regex pattern matched against qualified_name" },
            "file_pattern": { "type": ["string", "null"], "description": "Glob pattern matched against file_path" },
            "relationship": { "type": ["string", "null"], "description": "Edge type filter, e.g. CALLS, IMPORTS, CONTAINS" },
            "direction": { "type": ["string", "null"], "enum": ["inbound", "outbound", "any"], "default": "any" },
            "min_degree": { "type": ["integer", "null"] },
            "max_degree": { "type": ["integer", "null"] },
            "include_connected": { "type": "boolean", "default": false },
            "exclude_entry_points": { "type": "boolean", "default": false },
            "limit": { "type": "integer", "default": 200 },
            "offset": { "type": "integer", "default": 0 }
        }
    })
}

struct PipelineGuard {
    busy: Arc<std::sync::atomic::AtomicBool>,
}

impl PipelineGuard {
    fn new(busy: Arc<std::sync::atomic::AtomicBool>) -> Self {
        busy.store(true, std::sync::atomic::Ordering::SeqCst);
        Self { busy }
    }
}

impl Drop for PipelineGuard {
    fn drop(&mut self) {
        self.busy.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

fn tool_def(name: &str, description: &str, schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": schema
    })
}
