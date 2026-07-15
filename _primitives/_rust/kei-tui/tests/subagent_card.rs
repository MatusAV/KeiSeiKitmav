//! A sub-agent the oracle spawns must appear as a sidebar card — driven by the
//! cortex activity stream (`RunEvent::Activity`), the real nested-run view.
//!
//! kei-cortex registers every sub-agent in its RunRegistry with a `parent_id`
//! (the oracle run) and projects it via `/api/v1/cortex/activity/stream`. The
//! cockpit filters that snapshot to sub-agents whose parent is one of THIS
//! TUI's oracle runs and renders each as a card with its live `current_step`.

use kei_tui::agents::AgentStatus;
use kei_tui::app::App;
use kei_tui::runs::{RunEvent, RunView};

fn oracle_started(app: &mut App, id: &str) {
    app.apply_run_event(RunEvent::Started {
        id: id.to_string(),
        label: "oracle".into(),
        role: "chat".into(),
        task: "do a thing".into(),
    });
}

fn subagent(id: &str, parent: &str, status: &str, step: &str) -> RunView {
    RunView {
        id: id.into(),
        kind: "subagent".into(),
        parent_id: Some(parent.into()),
        model: "researcher".into(),
        status: status.into(),
        current_step: step.into(),
    }
}

#[test]
fn an_activity_subagent_under_this_tuis_oracle_becomes_a_named_card() {
    let mut app = App::new(std::env::temp_dir()).expect("init app");
    app.right_collapsed = true;
    oracle_started(&mut app, "run_oracle_1");

    app.apply_run_event(RunEvent::Activity(vec![subagent(
        "subagent-abc",
        "run_oracle_1",
        "running",
        "running tool: bash",
    )]));

    assert_eq!(app.agents.cards.len(), 1, "the sub-agent shows as a card");
    let c = &app.agents.cards[0];
    assert_eq!(c.label, "researcher", "labelled by its subagent_type (RunView.model)");
    assert_eq!(c.status, AgentStatus::Running);
    assert_eq!(c.last_tool.as_deref(), Some("running tool: bash"), "live current_step shows");
    assert!(!app.right_collapsed, "the sidebar is revealed so it's visible");
}

#[test]
fn a_subagent_under_a_foreign_parent_is_ignored() {
    let mut app = App::new(std::env::temp_dir()).expect("init app");
    oracle_started(&mut app, "run_oracle_1");
    // parent is some OTHER session's run (or the web morda's) — not ours.
    app.apply_run_event(RunEvent::Activity(vec![subagent(
        "subagent-foreign",
        "run_someone_else",
        "running",
        "running tool: read",
    )]));
    assert!(app.agents.cards.is_empty(), "only THIS TUI's sub-agents get cards (I3)");
}

#[test]
fn the_card_updates_status_and_step_across_snapshots() {
    let mut app = App::new(std::env::temp_dir()).expect("init app");
    oracle_started(&mut app, "run_oracle_1");
    app.apply_run_event(RunEvent::Activity(vec![subagent(
        "subagent-abc", "run_oracle_1", "running", "running tool: bash",
    )]));
    // next 1s snapshot: same sub-agent, new step, then terminal.
    app.apply_run_event(RunEvent::Activity(vec![subagent(
        "subagent-abc", "run_oracle_1", "running", "running tool: write",
    )]));
    app.apply_run_event(RunEvent::Activity(vec![subagent(
        "subagent-abc", "run_oracle_1", "done", "done",
    )]));

    assert_eq!(app.agents.cards.len(), 1, "the same sub-agent stays ONE card");
    let c = &app.agents.cards[0];
    assert_eq!(c.status, AgentStatus::Done);
    assert!(c.log.iter().any(|l| l.contains("write")), "each step is logged");
    assert!(c.log.iter().any(|l| l == "✓ done"), "terminal state logged once");
}

#[test]
fn the_oracle_run_is_never_itself_a_card() {
    let mut app = App::new(std::env::temp_dir()).expect("init app");
    oracle_started(&mut app, "run_oracle_1");
    // A snapshot that also lists the oracle run itself (kind="run") — not a card.
    app.apply_run_event(RunEvent::Activity(vec![RunView {
        id: "run_oracle_1".into(),
        kind: "run".into(),
        parent_id: None,
        model: "glm-4.6".into(),
        status: "running".into(),
        current_step: "generating response".into(),
    }]));
    assert!(app.agents.cards.is_empty(), "the oracle streams into chat, never a card");
}
