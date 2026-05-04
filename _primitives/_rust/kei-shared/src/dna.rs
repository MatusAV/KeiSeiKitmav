//! DNA wire format: `<role>::<caps>::<sha16-scope>::<sha16-body>-<hex16-nonce>`.
//!
//! SSoT for the substrate identity string. Any format-level change lands
//! here and propagates to consumers (kei-agent-runtime, kei-dna-index)
//! through re-export, not duplication.
//!
//! Wave 7C width bump: hex segments widened from 8→16 chars (32→64 bits).
//! At 32-bit per segment the birthday-bound collision threshold for
//! agent fingerprints is ~65k creations; at 64-bit it is ~4 billion.
//! 587-block substrate × growth horizon makes 32-bit unsafe.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Parsed DNA fields. Widths on hash segments are validated by `parse_dna`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedDna {
    pub role: String,
    pub caps: String,
    pub scope_sha: String,
    pub body_sha: String,
    pub nonce: String,
}

/// Strict parse errors. Consumers that need looser semantics (e.g. legacy
/// 4-hex rolling-upgrade acceptance in kei-agent-runtime) keep their own
/// parser and error type.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DnaError {
    #[error("DNA empty")]
    Empty,
    #[error("missing segments (expected 4 '::' separators, got {0})")]
    MissingSegments(usize),
    #[error("missing '-' between body and nonce")]
    MissingNonceDelim,
    #[error("empty role segment")]
    EmptyRole,
    #[error("empty caps segment")]
    EmptyCaps,
    #[error("invalid hex16 width for {field} (got {got})")]
    HexWidth { field: &'static str, got: usize },
    #[error("non-hex character in {field}")]
    NonHex { field: &'static str },
}

/// `true` iff `s` is exactly 16 ASCII hex characters.
pub fn is_hex16(s: &str) -> bool {
    s.len() == 16 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Backward-compat shim: returns `false`. Old name kept so external
/// callers compiling against the pre-Wave-7C API receive a hard
/// rejection (rather than silent acceptance of legacy 8-char DNAs).
#[deprecated(note = "Wave 7C: width bumped to 16; use is_hex16")]
pub fn is_hex8(_s: &str) -> bool {
    false
}

/// Strict parse. Requires 4 `::` segments, `<body>-<nonce>` tail, and
/// 16-hex width on `scope_sha`, `body_sha`, `nonce`. Rejects empty role/caps.
pub fn parse_dna(s: &str) -> Result<ParsedDna, DnaError> {
    if s.is_empty() {
        return Err(DnaError::Empty);
    }
    let parts: Vec<&str> = s.split("::").collect();
    if parts.len() != 4 {
        return Err(DnaError::MissingSegments(parts.len()));
    }
    let (body_sha, nonce) = parts[3]
        .split_once('-')
        .ok_or(DnaError::MissingNonceDelim)?;
    check_non_empty(parts[0], parts[1])?;
    check_hex16("scope_sha", parts[2])?;
    check_hex16("body_sha", body_sha)?;
    check_hex16("nonce", nonce)?;
    Ok(ParsedDna {
        role: parts[0].to_string(),
        caps: parts[1].to_string(),
        scope_sha: parts[2].to_string(),
        body_sha: body_sha.to_string(),
        nonce: nonce.to_string(),
    })
}

/// Render the canonical wire format. Deterministic — no I/O, no randomness.
pub fn compose_dna(
    role: &str,
    caps: &str,
    scope_sha: &str,
    body_sha: &str,
    nonce: &str,
) -> String {
    format!("{role}::{caps}::{scope_sha}::{body_sha}-{nonce}")
}

fn check_non_empty(role: &str, caps: &str) -> Result<(), DnaError> {
    if role.is_empty() {
        return Err(DnaError::EmptyRole);
    }
    if caps.is_empty() {
        return Err(DnaError::EmptyCaps);
    }
    Ok(())
}

fn check_hex16(field: &'static str, value: &str) -> Result<(), DnaError> {
    if value.len() != 16 {
        return Err(DnaError::HexWidth {
            field,
            got: value.len(),
        });
    }
    if !value.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(DnaError::NonHex { field });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compose_matches_manual_format() {
        let s = compose_dna(
            "r", "C",
            "1234567812345678",
            "ABCDEF01ABCDEF01",
            "deadbeefdeadbeef",
        );
        assert_eq!(
            s,
            "r::C::1234567812345678::ABCDEF01ABCDEF01-deadbeefdeadbeef"
        );
    }

    #[test]
    fn is_hex16_basic() {
        assert!(is_hex16("1234567812345678"));
        assert!(is_hex16("AbCdEf01AbCdEf01"));
        assert!(!is_hex16("12345678")); // old 8-char width rejected
        assert!(!is_hex16("123456781234567"));
        assert!(!is_hex16("12345678123456789"));
        assert!(!is_hex16("123456781234567Z"));
    }

    #[test]
    fn deprecated_is_hex8_rejects_all() {
        // Wave 7C: kept as `#[deprecated]` tombstone so callers still
        // compiled against the old API hard-fail rather than silently
        // accept legacy 8-char DNAs that would parse-fail downstream.
        #[allow(deprecated)]
        {
            assert!(!is_hex8("12345678"));
            assert!(!is_hex8("1234567812345678"));
        }
    }
}
