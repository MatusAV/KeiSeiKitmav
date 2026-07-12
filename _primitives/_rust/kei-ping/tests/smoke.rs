// SPDX-License-Identifier: Apache-2.0
//! Smoke tests: PingFilter decision logic (pure) + a SQLite store round-trip.

use kei_ping::model::now_epoch;
use kei_ping::sqlite_store::SqlitePingStore;
use kei_ping::{BackendKind, Heartbeat, PingFilter, PingStore};

fn hb(agent: &str, phase: &str, branch: Option<&str>, last_seen: u64) -> Heartbeat {
    Heartbeat {
        agent_id: agent.into(),
        session_id: None,
        phase: phase.into(),
        dna: None,
        branch: branch.map(str::to_string),
        cwd: None,
        last_seen_epoch: last_seen,
        note: None,
    }
}

#[test]
fn filter_ttl_default_is_90s() {
    let now = 1_000_000;
    let f = PingFilter::default();
    assert!(f.alive(&hb("a", "x", None, now), now), "fresh must be alive");
    assert!(
        f.alive(&hb("a", "x", None, now - 90), now),
        "exactly at the 90s edge is still alive"
    );
    assert!(
        !f.alive(&hb("a", "x", None, now - 91), now),
        "older than 90s must be filtered out"
    );
}

#[test]
fn filter_phase_prefix_and_branch() {
    let now = 1_000_000;
    let by_phase = PingFilter {
        phase_prefix: Some("wave-".into()),
        ..Default::default()
    };
    assert!(by_phase.alive(&hb("a", "wave-7-auth", None, now), now));
    assert!(!by_phase.alive(&hb("a", "merge-ceremony", None, now), now));

    let by_branch = PingFilter {
        branch: Some("main".into()),
        ..Default::default()
    };
    assert!(by_branch.alive(&hb("a", "x", Some("main"), now), now));
    assert!(!by_branch.alive(&hb("a", "x", Some("dev"), now), now));
    assert!(!by_branch.alive(&hb("a", "x", None, now), now), "no branch != 'main'");
}

#[test]
fn now_epoch_and_backend_kind() {
    assert!(now_epoch() > 1_600_000_000, "epoch clock looks wrong");
    assert_eq!(BackendKind::Sqlite.as_str(), "sqlite");
    assert_eq!(BackendKind::Redis.as_str(), "redis");
}

#[tokio::test]
async fn sqlite_store_round_trip() {
    // Unique temp DB so parallel test binaries don't collide; never touches ~/.claude.
    let base = std::env::temp_dir().join(format!("kei-ping-smoke-{}.sqlite", std::process::id()));
    let cleanup = || {
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{}", base.display(), suffix));
        }
    };
    cleanup(); // start clean

    let store = SqlitePingStore::open(base.clone()).expect("open temp store");
    assert_eq!(store.kind(), BackendKind::Sqlite);

    let now = now_epoch();
    store.send(&hb("agent-1", "phase-a", Some("main"), now)).await.expect("send");

    let got = store.list(&PingFilter::default()).await.expect("list");
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].agent_id, "agent-1");
    assert_eq!(got[0].phase, "phase-a");

    // Upsert: same agent_id updates in place, not a second row.
    store.send(&hb("agent-1", "phase-b", Some("main"), now)).await.expect("re-send");
    let got = store.list(&PingFilter::default()).await.expect("list");
    assert_eq!(got.len(), 1, "same agent_id must upsert, not duplicate");
    assert_eq!(got[0].phase, "phase-b");

    // Clear removes it.
    store.clear("agent-1").await.expect("clear");
    let got = store.list(&PingFilter::default()).await.expect("list");
    assert!(got.is_empty(), "cleared agent must be gone");

    cleanup();
}
