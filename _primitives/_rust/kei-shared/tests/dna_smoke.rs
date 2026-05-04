//! Smoke tests for the shared DNA parser.

use kei_shared::dna::{compose_dna, is_hex16, parse_dna, DnaError};

// Wave 7C: 16-hex-char (64-bit) segments instead of 8-hex (32-bit).
const CANONICAL: &str = concat!(
    "edit-local::NG-FW-FD-CP-CG-TG-ND-RF::",
    "5435F8215435F821::AC73A6A3AC73A6A3-e9bf468de9bf468d",
);

#[test]
fn parse_valid_round_trip() {
    let parsed = parse_dna(CANONICAL).expect("canonical DNA must parse");
    assert_eq!(parsed.role, "edit-local");
    assert_eq!(parsed.caps, "NG-FW-FD-CP-CG-TG-ND-RF");
    assert_eq!(parsed.scope_sha, "5435F8215435F821");
    assert_eq!(parsed.body_sha, "AC73A6A3AC73A6A3");
    assert_eq!(parsed.nonce, "e9bf468de9bf468d");
}

#[test]
fn parse_empty_rejected() {
    assert_eq!(parse_dna("").unwrap_err(), DnaError::Empty);
}

#[test]
fn parse_missing_segments() {
    let err = parse_dna("only::two::segments").unwrap_err();
    assert!(matches!(err, DnaError::MissingSegments(_)));
}

#[test]
fn parse_missing_nonce_delim() {
    // 4 `::` segments but no '-' in tail.
    let err = parse_dna(
        "r::C::1234567812345678::AC73A6A3AC73A6A3e9bf468de9bf468d",
    )
    .unwrap_err();
    assert_eq!(err, DnaError::MissingNonceDelim);
}

#[test]
fn parse_non_hex_rejected() {
    let err = parse_dna(
        "r::C::1234567812345678::AC73A6A3AC73A6A3-ZZZZZZZZZZZZZZZZ",
    )
    .unwrap_err();
    assert!(matches!(err, DnaError::NonHex { .. }));
}

#[test]
fn parse_short_hex_rejected() {
    // scope_sha has only 4 hex chars — strict parser rejects.
    let err = parse_dna("r::C::1234::AC73A6A3AC73A6A3-e9bf468de9bf468d").unwrap_err();
    assert!(matches!(err, DnaError::HexWidth { .. }));
}

#[test]
fn parse_rejects_legacy_8_hex() {
    // Wave 7C: 8-char DNAs that previously parsed must now FAIL.
    let err = parse_dna("r::C::12345678::AC73A6A3-e9bf468d").unwrap_err();
    assert!(matches!(err, DnaError::HexWidth { .. }));
}

#[test]
fn parse_rejects_empty_role() {
    let err = parse_dna(
        "::C::1234567812345678::AC73A6A3AC73A6A3-e9bf468de9bf468d",
    )
    .unwrap_err();
    assert_eq!(err, DnaError::EmptyRole);
}

#[test]
fn parse_rejects_empty_caps() {
    let err = parse_dna(
        "r::::1234567812345678::AC73A6A3AC73A6A3-e9bf468de9bf468d",
    )
    .unwrap_err();
    assert_eq!(err, DnaError::EmptyCaps);
}

#[test]
fn compose_parse_round_trip() {
    let composed = compose_dna(
        "edit-local",
        "NG-FW-FD-CP-CG-TG-ND-RF",
        "5435F8215435F821",
        "AC73A6A3AC73A6A3",
        "e9bf468de9bf468d",
    );
    assert_eq!(composed, CANONICAL);
    let parsed = parse_dna(&composed).expect("round-trip parse");
    assert_eq!(parsed.scope_sha, "5435F8215435F821");
    assert_eq!(parsed.body_sha, "AC73A6A3AC73A6A3");
    assert_eq!(parsed.nonce, "e9bf468de9bf468d");
}

#[test]
fn is_hex16_accepts_valid_rejects_invalid() {
    assert!(is_hex16("0000000000000000"));
    assert!(is_hex16("DeAdBeEfDeAdBeEf"));
    assert!(is_hex16("abcdef01abcdef01"));
    assert!(!is_hex16("abcdefg1abcdef01"), "non-hex 'g' must be rejected");
    assert!(!is_hex16(""));
    assert!(!is_hex16("0000000000000")); // too short
    assert!(!is_hex16("00000000")); // legacy 8-char width rejected
    assert!(!is_hex16("000000000000000000")); // too long
}
