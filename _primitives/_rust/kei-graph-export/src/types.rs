use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Node {
    pub id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub category: String,
    pub tags: Vec<String>,
    pub connections: usize,
    pub extra: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Edge {
    pub source: String,
    pub target: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub weight: f32,
}

#[derive(Debug, Serialize)]
pub struct SpaceData {
    pub nodes: Vec<Node>,
    pub links: Vec<Edge>,
}

#[derive(Debug, Serialize)]
pub struct Space {
    pub name: &'static str,
    pub icon: &'static str,
    pub description: &'static str,
    pub colors: HashMap<String, String>,
    pub data: SpaceData,
}

pub fn sanitize_id(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() || "-_:/.".contains(c) { c } else { '_' })
        .collect()
}

pub fn dna_prefix(dna: &str) -> String {
    sanitize_id(&dna.chars().take(30).collect::<String>())
}

pub fn truncate_chars(s: &str, max: usize) -> &str {
    if s.chars().count() <= max { return s; }
    let end = s.char_indices().nth(max).map(|(i, _)| i).unwrap_or(s.len());
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_id_replaces_disallowed_and_keeps_allowed() {
        // space, '!', '#' -> '_'; alphanumerics and -_:/. survive as-is.
        assert_eq!(sanitize_id("a b!c"), "a_b_c");
        assert_eq!(sanitize_id("ok-id_1:2/3.v"), "ok-id_1:2/3.v");
        assert_eq!(sanitize_id("we#ird space"), "we_ird_space");
        // Unicode alphanumerics are kept (char::is_alphanumeric).
        assert_eq!(sanitize_id("café"), "café");
    }

    #[test]
    fn dna_prefix_truncates_to_30_then_sanitizes() {
        // short input: only sanitized, not truncated
        assert_eq!(dna_prefix("abc def"), "abc_def");
        // >30 chars: cut to the first 30 (all sanitized)
        let long = "0123456789012345678901234567890123456789"; // 40 chars
        let out = dna_prefix(long);
        assert_eq!(out.chars().count(), 30);
        assert_eq!(out, "012345678901234567890123456789");
    }

    #[test]
    fn truncate_chars_is_char_boundary_safe() {
        assert_eq!(truncate_chars("hello", 3), "hel");
        // shorter than max -> unchanged
        assert_eq!(truncate_chars("hi", 5), "hi");
        assert_eq!(truncate_chars("", 3), "");
        // multi-byte chars: must cut on a char boundary, never panic
        let out = truncate_chars("héllo", 3); // 'é' is 2 bytes
        assert_eq!(out, "hél");
        assert_eq!(out.chars().count(), 3);
    }
}
