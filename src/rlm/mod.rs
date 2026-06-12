mod session;
mod workflow;

pub use session::*;
pub use workflow::*;

use crate::error::Result;
use crate::project::normalize_project_name;
use crate::store::{SearchFilter, Store};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

/// RLM orchestrator: filter → map → reduce over the knowledge graph.
pub struct RlmEngine {
    sessions: Arc<Mutex<SessionStore>>,
}

impl RlmEngine {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(SessionStore::new())),
        }
    }

    pub fn workflow(&self, phase: &str) -> Value {
        workflow_guidance(phase)
    }

    pub fn filter(&self, project: &str, filter: SearchFilter) -> Result<Value> {
        let project = normalize_project_name(project);
        let store = Store::open(&project)?;
        let result = store.search(&filter)?;
        Ok(json!({
            "phase": "filter",
            "project": project,
            "total": result.total,
            "symbols": result.symbols,
            "hint": "Use rlm_read_symbol for each qualified_name (one per call)"
        }))
    }

    pub fn read_symbol(&self, project: &str, qualified_name: &str) -> Result<Value> {
        let project = normalize_project_name(project);
        let store = Store::open(&project)?;
        let snippet = store.get_snippet(qualified_name)?;
        let outbound = store.trace_path(qualified_name, "outbound", 1)?;
        Ok(json!({
            "phase": "map",
            "symbol": snippet.symbol,
            "snippet": snippet.snippet,
            "calls": outbound.edges,
            "hint": "One symbol per call — do not batch"
        }))
    }

    pub fn scan(&self, path: &str) -> Result<Value> {
        let session = self.sessions.lock().unwrap().create_from_path(path)?;
        Ok(json!({
            "session_id": session.id,
            "file_count": session.chunks.len(),
            "total_bytes": session.total_bytes,
            "hint": "Use rlm_chunk or rlm_peek to read chunks"
        }))
    }

    pub fn chunk(&self, session_id: &str, offset: usize, limit: usize) -> Result<Value> {
        let store = self.sessions.lock().unwrap();
        let session = store.get(session_id)?;
        let chunks: Vec<_> = session
            .chunks
            .iter()
            .skip(offset)
            .take(limit)
            .cloned()
            .collect();
        Ok(json!({
            "session_id": session_id,
            "offset": offset,
            "limit": limit,
            "total": session.chunks.len(),
            "chunks": chunks
        }))
    }

    pub fn peek(&self, session_id: &str, query: &str) -> Result<Value> {
        let store = self.sessions.lock().unwrap();
        let session = store.get(session_id)?;
        let matches: Vec<_> = session
            .chunks
            .iter()
            .filter(|c| c.content.contains(query) || c.path.contains(query))
            .take(20)
            .cloned()
            .collect();
        Ok(json!({
            "session_id": session_id,
            "query": query,
            "matches": matches
        }))
    }

    pub fn session_list(&self) -> Value {
        let store = self.sessions.lock().unwrap();
        json!({ "sessions": store.list() })
    }

    pub fn session_delete(&self, session_id: &str) -> Result<Value> {
        self.sessions.lock().unwrap().delete(session_id)?;
        Ok(json!({ "deleted": session_id }))
    }

    pub fn reduce(&self, project: &str) -> Result<Value> {
        let project = normalize_project_name(project);
        let store = Store::open(&project)?;
        let arch = store.get_architecture()?;
        Ok(json!({
            "phase": "reduce",
            "architecture": arch,
            "hint": "Synthesize findings into structured JSON before final answer"
        }))
    }
}

impl Default for RlmEngine {
    fn default() -> Self {
        Self::new()
    }
}