//! Day-total token counter — the sum of ALL sessions' tokens today, read
//! straight from `~/.keisei/token-events.sqlite` (the same store kei-cortex
//! writes every turn's usage to). The in-App `day_tokens` counts only THIS
//! cockpit's tokens; this reads the whole day across every session.
//!
//! Read-only + best-effort: a missing DB / query error yields `None`, and the
//! caller shows a dash. Called from a background tick so the read never blocks
//! the UI.

use std::path::PathBuf;

/// Path to the cortex token-events DB (same `~/.keisei` convention the daemon
/// uses).
fn db_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".keisei/token-events.sqlite"))
}

/// Sum of `input_tokens + output_tokens` for events whose `ts` falls on the
/// local calendar day of `now_unix`. `None` on any error (missing DB, no table).
pub fn today_tokens(now_unix: i64) -> Option<u64> {
    today_tokens_at(&db_path()?, now_unix)
}

/// The path-explicit core of [`today_tokens`] — tested hermetically so the test
/// never mutates the process-global `HOME` (which races other threads' env
/// reads under cargo's parallel test runner — a recorded anti-pattern).
pub fn today_tokens_at(path: &std::path::Path, now_unix: i64) -> Option<u64> {
    if !path.exists() {
        return None;
    }
    // Local midnight boundaries for `now_unix`.
    let (start, end) = local_day_bounds(now_unix);
    let conn = rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .ok()?;
    let total: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(input_tokens + output_tokens), 0)
             FROM token_events WHERE ts >= ?1 AND ts < ?2",
            [start, end],
            |r| r.get(0),
        )
        .ok()?;
    Some(total.max(0) as u64)
}

/// The [start, end) unix-second bounds of the local calendar day containing
/// `now_unix`, using chrono's local offset.
fn local_day_bounds(now_unix: i64) -> (i64, i64) {
    use chrono::{Local, TimeZone};
    let dt = Local.timestamp_opt(now_unix, 0).single().unwrap_or_else(|| Local.timestamp_opt(0, 0).unwrap());
    let start_naive = dt.date_naive().and_hms_opt(0, 0, 0).unwrap();
    let start = Local
        .from_local_datetime(&start_naive)
        .single()
        .map(|d| d.timestamp())
        .unwrap_or(now_unix);
    (start, start + 86_400)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_bounds_are_86400_apart_and_contain_now() {
        let now = 1_783_600_000; // arbitrary
        let (start, end) = local_day_bounds(now);
        assert_eq!(end - start, 86_400, "a day is 24h");
        assert!(start <= now && now < end, "now falls inside the day");
    }

    #[test]
    fn today_tokens_at_a_missing_db_is_none_and_never_panics() {
        // Hermetic: point at a non-existent path directly — NO HOME mutation, so
        // this can't poison another thread's env read under parallel tests.
        let missing = std::env::temp_dir().join("kei-daytotal-does-not-exist.sqlite");
        let _ = std::fs::remove_file(&missing);
        assert!(today_tokens_at(&missing, 1_783_600_000).is_none());
    }
}
