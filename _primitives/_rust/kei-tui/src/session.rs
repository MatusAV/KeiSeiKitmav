//! Chat-session persistence — the chat history survives a restart, and MANY
//! named sessions are kept (not just the last). Stored as JSON under
//! `~/.keisei/keiseikode/session-<id>.json`, with a `last` pointer file naming
//! the session to reload on startup. Best-effort: a cockpit must never die
//! because the disk is full.

use crate::chat::{Msg, Role};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn session_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".keisei").join("keiseikode"))
}

fn session_file(id: &str) -> Option<PathBuf> {
    Some(session_dir()?.join(format!("session-{id}.json")))
}

fn last_pointer() -> Option<PathBuf> {
    Some(session_dir()?.join("last"))
}

/// A fresh session id (seconds since the epoch). Monotonic-enough for a
/// single-user cockpit; collisions within one second reuse the same file.
pub fn new_id() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    secs.to_string()
}

/// Persist `msgs` under session `id` and mark it the last-opened session
/// (best-effort; write errors are swallowed).
pub fn save(id: &str, msgs: &[Msg]) {
    let Some(dir) = session_dir() else { return };
    let _ = std::fs::create_dir_all(&dir);
    if let (Some(path), Ok(json)) = (session_file(id), serde_json::to_string(msgs)) {
        let _ = std::fs::write(path, json);
    }
    if let Some(p) = last_pointer() {
        let _ = std::fs::write(p, id);
    }
}

/// Load one session's transcript (empty if unknown / unparsable).
pub fn load(id: &str) -> Vec<Msg> {
    session_file(id)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// The id of the last-opened session, if the pointer exists.
pub fn last_id() -> Option<String> {
    let raw = std::fs::read_to_string(last_pointer()?).ok()?;
    let id = raw.trim().to_string();
    if id.is_empty() {
        None
    } else {
        Some(id)
    }
}

/// Reload the last-opened session's transcript (empty vec if none).
pub fn load_last() -> Vec<Msg> {
    last_id().map(|id| load(&id)).unwrap_or_default()
}

/// One saved session for the `/sessions` picker.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: String,
    /// First user line (a human-readable preview), or "(empty)".
    pub preview: String,
}

/// Every saved session, newest id first, each with a one-line preview.
pub fn list() -> Vec<SessionInfo> {
    let Some(dir) = session_dir() else { return Vec::new() };
    let Ok(rd) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut out: Vec<SessionInfo> = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(id) = name.strip_prefix("session-").and_then(|s| s.strip_suffix(".json")) else {
            continue;
        };
        let msgs = load(id);
        let preview = msgs
            .iter()
            .find(|m| m.role == Role::User && !m.text.trim().is_empty())
            .map(|m| m.text.chars().take(48).collect::<String>())
            .unwrap_or_else(|| "(empty)".to_string());
        out.push(SessionInfo { id: id.to_string(), preview });
    }
    // Newest first: ids are epoch-seconds strings, so numeric-desc by parse.
    out.sort_by(|a, b| b.id.parse::<u64>().unwrap_or(0).cmp(&a.id.parse::<u64>().unwrap_or(0)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::Role;

    fn scratch_home() {
        let tmp = std::env::temp_dir().join(format!("keiseikode-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        std::env::set_var("HOME", &tmp);
    }

    // ONE test (not two) — `save`/`last` share a process-global HOME + the
    // `last` pointer file, so two parallel tests race on it. Running the whole
    // roundtrip + list sequence in a single test keeps it deterministic.
    #[test]
    fn save_load_list_and_last_roundtrip() {
        scratch_home();
        // roundtrip + last pointer
        let id = "1000000001".to_string();
        save(&id, &[
            Msg { role: Role::User, text: "hi".into(), image: None },
            Msg { role: Role::Agent, text: "hello".into(), image: None },
        ]);
        assert_eq!(load(&id).len(), 2);
        assert_eq!(last_id().as_deref(), Some(id.as_str()));
        assert_eq!(load_last()[0].text, "hi");
        // list previews + newest-first ordering
        save("1000000010", &[Msg { role: Role::User, text: "older goal".into(), image: None }]);
        save("1000000020", &[Msg { role: Role::User, text: "newer goal".into(), image: None }]);
        let ls = list();
        assert!(ls.len() >= 3);
        assert_eq!(ls[0].id, "1000000020");
        assert!(ls.iter().any(|s| s.preview.contains("older goal")));
    }
}
