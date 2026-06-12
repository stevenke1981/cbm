use crate::error::Result;
use crate::rlm::session::ScanSession;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub const SESSION_TTL_SECS: u64 = 3600;
pub const MAX_PERSISTED_SESSIONS: usize = 50;

pub fn sessions_dir() -> PathBuf {
    crate::project::default_cache_dir().join("rlm-sessions")
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn persist_session(session: &ScanSession) -> Result<()> {
    let dir = sessions_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", session.id));
    std::fs::write(path, serde_json::to_string(session)?)?;
    Ok(())
}

pub fn remove_session_file(id: &str) -> Result<()> {
    let path = sessions_dir().join(format!("{id}.json"));
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

pub fn load_persisted_sessions() -> Result<Vec<ScanSession>> {
    let dir = sessions_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut sessions = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Ok(session) = serde_json::from_str::<ScanSession>(&content) {
            sessions.push(session);
        }
    }
    Ok(sessions)
}

pub fn purge_expired(sessions: &mut std::collections::HashMap<String, ScanSession>) -> Result<()> {
    let now = unix_now();
    let expired: Vec<String> = sessions
        .iter()
        .filter(|(_, s)| now.saturating_sub(s.created_at_unix) > SESSION_TTL_SECS)
        .map(|(id, _)| id.clone())
        .collect();
    for id in expired {
        sessions.remove(&id);
        let _ = remove_session_file(&id);
    }
    Ok(())
}

pub fn trim_to_limit(sessions: &mut std::collections::HashMap<String, ScanSession>) -> Result<()> {
    if sessions.len() <= MAX_PERSISTED_SESSIONS {
        return Ok(());
    }
    let mut ids: Vec<_> = sessions
        .values()
        .map(|s| (s.id.clone(), s.created_at_unix))
        .collect();
    ids.sort_by_key(|(_, created)| *created);
    let remove_count = sessions.len() - MAX_PERSISTED_SESSIONS;
    for (id, _) in ids.into_iter().take(remove_count) {
        sessions.remove(&id);
        let _ = remove_session_file(&id);
    }
    Ok(())
}
