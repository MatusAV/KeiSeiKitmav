//! Phase 3 Layer 3 — STATUS-TRUTH MARKER → kei-registry pipe.
//!
//! Constructor Pattern: parse RULE 0.16 marker blocks + insert one row
//! into `cleanup_findings`. Schema is provisioned via CREATE TABLE IF
//! NOT EXISTS so the cube is self-contained — no global migration bump.

use anyhow::{anyhow, Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParsedMarker {
    pub shipped: ShippedKind,
    pub stubs_count: u32,
    pub stubs_locations: Vec<String>,
    pub cargo_check: CheckResult,
    pub behaviour_verified: BoolOrNa,
    pub follow_up_required: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ShippedKind {
    Functional,
    Partial,
    Scaffolding,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CheckResult {
    Pass,
    Fail,
    NotRun,
    NotApplicable,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum BoolOrNa {
    Yes,
    No,
    NotApplicable,
}

const ENSURE_SCHEMA_SQL: &str = "CREATE TABLE IF NOT EXISTS cleanup_findings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_sha TEXT NOT NULL,
    severity TEXT NOT NULL,
    kind TEXT NOT NULL,
    finding_json TEXT NOT NULL,
    created INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS idx_cf_kind ON cleanup_findings(kind);
CREATE INDEX IF NOT EXISTS idx_cf_ws ON cleanup_findings(workspace_sha);";

pub fn ensure_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(ENSURE_SCHEMA_SQL).context("ensure_schema")?;
    Ok(())
}

pub fn severity_of(m: &ParsedMarker) -> &'static str {
    match m.shipped {
        ShippedKind::Scaffolding => "high",
        ShippedKind::Partial => "medium",
        ShippedKind::Functional if m.stubs_count > 0 => "low",
        ShippedKind::Functional => "info",
    }
}

pub fn register(conn: &Connection, block_id: &str, m: &ParsedMarker) -> Result<bool> {
    ensure_schema(conn)?;
    let sev = severity_of(m);
    if sev == "info" {
        return Ok(false);
    }
    let json = serde_json::to_string(m).context("serialize marker")?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    conn.execute(
        "INSERT INTO cleanup_findings (workspace_sha, severity, kind, finding_json, created) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![block_id, sev, "agent_status_truth", json, now],
    )
    .context("insert cleanup_findings")?;
    Ok(true)
}

const DELIM: &str = "=== STATUS-TRUTH MARKER ===";

pub fn parse_marker(text: &str) -> Result<ParsedMarker> {
    let body = match text.find(DELIM) {
        Some(idx) => &text[idx + DELIM.len()..],
        None => text,
    };
    Ok(ParsedMarker {
        shipped: parse_shipped(body)?,
        stubs_count: parse_stubs_count(body)?,
        stubs_locations: parse_stubs_locs(body),
        cargo_check: parse_cargo_check(body)?,
        behaviour_verified: parse_behaviour(body)?,
        follow_up_required: parse_follow_up(body),
    })
}

fn line_after(body: &str, prefix: &str) -> Option<String> {
    body.lines()
        .map(str::trim)
        .find(|l| l.starts_with(prefix))
        .map(|l| l[prefix.len()..].trim().to_string())
}

fn first_word(v: &str) -> String {
    v.split_whitespace().next().unwrap_or("").to_string()
}

fn parse_shipped(body: &str) -> Result<ShippedKind> {
    let v = line_after(body, "shipped:").ok_or_else(|| anyhow!("missing 'shipped:'"))?;
    match first_word(&v).to_lowercase().as_str() {
        "functional" => Ok(ShippedKind::Functional),
        "partial" => Ok(ShippedKind::Partial),
        "scaffolding" => Ok(ShippedKind::Scaffolding),
        o => Err(anyhow!("invalid shipped: '{o}'")),
    }
}

fn parse_stubs_count(body: &str) -> Result<u32> {
    let v = line_after(body, "stubs:").ok_or_else(|| anyhow!("missing 'stubs:'"))?;
    let tok = first_word(&v);
    tok.parse().map_err(|_| anyhow!("stubs count not u32: '{tok}'"))
}

fn parse_stubs_locs(body: &str) -> Vec<String> {
    let v = line_after(body, "stubs:").unwrap_or_default();
    let mut parts = v.split_whitespace();
    let _ = parts.next();
    let rest = parts.collect::<Vec<_>>().join(" ");
    if rest.trim().is_empty() {
        return Vec::new();
    }
    rest.split(|c: char| c == ',' || c == ';')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn parse_cargo_check(body: &str) -> Result<CheckResult> {
    let v = line_after(body, "cargo-check:").ok_or_else(|| anyhow!("missing 'cargo-check:'"))?;
    match first_word(&v).to_uppercase().as_str() {
        "PASS" => Ok(CheckResult::Pass),
        "FAIL" => Ok(CheckResult::Fail),
        "NOT-RUN" | "NOT_RUN" => Ok(CheckResult::NotRun),
        "NOT-APPLICABLE" | "N/A" => Ok(CheckResult::NotApplicable),
        o => Err(anyhow!("invalid cargo-check: '{o}'")),
    }
}

fn parse_behaviour(body: &str) -> Result<BoolOrNa> {
    let v = line_after(body, "behaviour-verified:")
        .ok_or_else(|| anyhow!("missing 'behaviour-verified:'"))?;
    match first_word(&v).to_lowercase().as_str() {
        "yes" => Ok(BoolOrNa::Yes),
        "no" => Ok(BoolOrNa::No),
        "not-applicable" | "n/a" => Ok(BoolOrNa::NotApplicable),
        o => Err(anyhow!("invalid behaviour-verified: '{o}'")),
    }
}

fn parse_follow_up(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_sec = false;
    for raw in body.lines() {
        let s = raw.trim_start();
        if s.starts_with("follow-up-required:") {
            in_sec = true;
            continue;
        }
        if !in_sec {
            continue;
        }
        if let Some(item) = s.strip_prefix("- ") {
            out.push(item.trim().to_string());
        } else if !s.is_empty() && !s.starts_with('-') {
            break;
        }
    }
    out
}
