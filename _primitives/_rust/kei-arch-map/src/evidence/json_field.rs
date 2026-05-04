use super::{path_resolve, regex_match};
use serde_json::Value;
use std::path::Path;

/// Walk dotted path (no wildcards) into JSON value.
fn walk<'a>(v: &'a Value, dotted: &str) -> Option<&'a Value> {
    let mut cur = v;
    for seg in dotted.split('.') {
        if seg.is_empty() {
            return None;
        }
        cur = cur.as_object()?.get(seg)?;
    }
    Some(cur)
}

/// Stringify the leaf JSON value for comparison.
/// String -> raw text; numbers/bools -> their JSON form; objects/arrays -> reject.
fn stringify_leaf(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Null => Some("null".to_string()),
        Value::Object(_) | Value::Array(_) => None,
    }
}

pub fn check(file: &Path, dotted: &str, expected: &str, root: &Path) -> (bool, String) {
    let resolved = path_resolve::resolve(file, root);
    let contents = match regex_match::read_capped(&resolved) {
        Ok(s) => s,
        Err(e) => return (false, e),
    };
    let parsed: Value = match serde_json::from_str(&contents) {
        Ok(v) => v,
        Err(e) => return (false, format!("invalid JSON in {}: {}", resolved.display(), e)),
    };
    let leaf = match walk(&parsed, dotted) {
        Some(v) => v,
        None => return (false, format!("path `{}` not found in {}", dotted, resolved.display())),
    };
    let actual = match stringify_leaf(leaf) {
        Some(s) => s,
        None => return (false, format!("path `{}` is object/array (not scalar)", dotted)),
    };
    if actual == expected {
        (true, String::new())
    } else {
        (
            false,
            format!("json `{}`.{} = `{}` (expected `{}`)", resolved.display(), dotted, actual, expected),
        )
    }
}
