//! Ratatui rendering. Delegates each pane to its own module; owns only the
//! shared 3-column layout (`regions`, reused by the mouse hit-test) and the
//! bottom status bar.

use crate::app::{App, CenterMode};
use crate::types::Pane;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

/// Split the left sidebar column into `[tree_body, settings_tab]` — the tab is
/// the bottom 1-row strip (t22). Single source of truth for draw + hit-test.
pub fn left_split(tree_col: Rect) -> [Rect; 2] {
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(tree_col);
    [parts[0], parts[1]]
}

/// Screen regions: `[tree, center, agents, modebar, status]`. Single source of
/// truth shared by `draw` and the runner's mouse hit-test.
///
/// Two deliberate geometry choices:
/// * The mode bar is a FULL-WIDTH row of its own (below all three columns), so
///   the center chat box is the SAME height as the sidebars — they line up.
///   (Previously the mode bar was carved out of the center only, so the chat
///   box was 1 row short and looked mis-aligned against the right sidebar.)
/// * Adjacent columns OVERLAP by 1 cell so their borders coincide into a SINGLE
///   shared line — no double border / gap between panes ("примыкают по одной
///   линии"). All panes use the same grid color, so the overwrite is seamless.
pub fn regions(area: Rect, tree_collapsed: bool, right_collapsed: bool) -> [Rect; 6] {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // top-right readout: model · context-window · effort
            Constraint::Min(3),    // content columns
            Constraint::Length(1), // mode bar
            Constraint::Length(1), // status
        ])
        .split(area);
    let (header, content, modebar, status) = (rows[0], rows[1], rows[2], rows[3]);
    // A collapsed side is HIDDEN entirely (width 0 — not a strip); the center
    // (Min) soaks up the whole width, so the chat is clean + central.
    let left = if tree_collapsed { Constraint::Length(0) } else { Constraint::Percentage(26) };
    let right = if right_collapsed { Constraint::Length(0) } else { Constraint::Percentage(26) };
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([left, Constraint::Min(10), right])
        .split(content);
    // Grow a pane 1 cell LEFT so its left border draws over the neighbour's
    // right border > one shared vertical line. Only when that neighbour is a
    // real bordered pane (not a collapsed text strip, which keeps its 3-wide
    // contract and sits cleanly adjacent instead).
    let center = if tree_collapsed {
        cols[1]
    } else {
        Rect { x: cols[1].x.saturating_sub(1), width: cols[1].width + 1, ..cols[1] }
    };
    let agents = if right_collapsed {
        cols[2]
    } else {
        Rect { x: cols[2].x.saturating_sub(1), width: cols[2].width + 1, ..cols[2] }
    };
    [header, cols[0], center, agents, modebar, status]
}

/// Which pane a screen coordinate falls into (`None` = status bar / outside).
pub fn pane_at(area: Rect, col: u16, row: u16, tree_collapsed: bool, right_collapsed: bool) -> Option<Pane> {
    let [_header, tree, term, agents, _modebar, _status] = regions(area, tree_collapsed, right_collapsed);
    let hit = |r: Rect| col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height;
    if hit(tree) {
        Some(Pane::Tree)
    } else if hit(term) {
        Some(Pane::Terminal)
    } else if hit(agents) {
        Some(Pane::Agents)
    } else {
        None
    }
}

/// Split the right column into `[agents, passport]` — a 50/50 vertical split
/// (t40). Shared by draw + hit-test.
pub fn right_split(agents_col: Rect) -> [Rect; 2] {
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(agents_col);
    // Grow the bottom pane 1 row UP so its top border draws over the top pane's
    // bottom border > one shared horizontal line (no double divider).
    let bottom = Rect { y: parts[1].y.saturating_sub(1), height: parts[1].height + 1, ..parts[1] };
    [parts[0], bottom]
}

/// Draw the whole cockpit for one frame. Left = file tree + settings tab.
/// Center = the primary CHAT (full height), UNLESS a file is open — then the
/// code editor rides ABOVE the chat (a vertical split); `Terminal`/`Agent`/
/// `Settings` are secondary center modes. Right = agents (top) + `.mf` passport
/// (bottom), 50/50.
pub fn draw(f: &mut Frame, app: &mut App) {
    let [header, tree_col, center, agents_col, modebar, status] = regions(f.area(), app.tree_collapsed, app.right_collapsed);
    // Top-right corner readout ABOVE the chat: model · context-window · effort.
    draw_header_right(f, header, app);
    let focus = app.focus;

    // Left column. Hidden entirely when collapsed (/f toggles it); else the
    // file tree OR the running-agents "structure" view — FULL height so its
    // bottom border lines up with the chat's (settings is on F8, no tab strip).
    if !app.tree_collapsed {
        match app.left_view {
            crate::app::LeftView::Files => app.tree.render(f, tree_col, focus == Pane::Tree),
            crate::app::LeftView::Structure => draw_structure(f, tree_col, &app.agents),
        }
    }

    // Center.
    match &app.center {
        CenterMode::Terminal => {
            // The terminal window is named by the active project (/cp), so it's
            // clear what's being worked on; falls back to "terminal".
            let title = app.project.clone().unwrap_or_else(|| "terminal".into());
            app.term.render(f, center, focus == Pane::Terminal, &title);
        }
        CenterMode::Agent(id) => match app.agents.cards.iter().find(|c| &c.id == id) {
            Some(card) => crate::agents::render_detail(f, center, card),
            None => app.chat.render(f, center, focus == Pane::Terminal, false, 0, 0, app.user_right, ratatui::widgets::Borders::BOTTOM),
        },
        CenterMode::Settings => app.settings.render(
            f,
            center,
            focus == Pane::Terminal,
            crate::theme::name(),
            &app.provider,
            &app.base_url,
            app.user_right,
        ),
        // The project plan / passport, full-screen (/ps · /plan · Ctrl-P).
        CenterMode::Plan => app.passport.render(f, center, true),
        // The agents dashboard (/a or the bottom "agents" indicator).
        CenterMode::Agents => {
            crate::agents::render_dashboard(f, center, &app.agents, app.app_started.elapsed().as_millis());
        }
        // An agent's DNA (manifest) view + editor, full-screen.
        CenterMode::Dna => app.dna.render(f, center),
        // A screenshot / inline image, full-screen. Esc back to chat.
        CenterMode::Image(id) => {
            let id = *id;
            app.images.render(f, center, id);
        }
        // Chat is primary; it fills the whole center (same height as the
        // sidebars). The editor rides ABOVE it only when a file is open.
        CenterMode::Chat => {
            let busy = app.oracle_busy;
            let elapsed = app.oracle_elapsed_ms();
            // No top border ever; BOTTOM separates the chat from the mode bar;
            // a LEFT/RIGHT wall appears ONLY on the side whose sidebar is open.
            let mut borders = ratatui::widgets::Borders::BOTTOM;
            if !app.tree_collapsed {
                borders |= ratatui::widgets::Borders::LEFT;
            }
            if !app.right_collapsed {
                borders |= ratatui::widgets::Borders::RIGHT;
            }
            // Navigating the mode bar takes the caret away from the chat: the
            // input must not keep blinking somewhere you are not typing.
            let chat_has_caret = focus == Pane::Terminal && app.bar_focus.is_none();
            if app.editor.is_open() {
                let rows = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
                    .split(center);
                app.editor.render(f, rows[0], app.editor_focus);
                let chat_focus = chat_has_caret && !app.editor_focus;
                app.chat.render(f, rows[1], chat_focus, busy, elapsed, app.oracle_tokens, app.user_right, borders);
            } else {
                app.chat.render(f, center, chat_has_caret, busy, elapsed, app.oracle_tokens, app.user_right, borders);
            }
        }
    }
    // Mode bar (2nd bottom line) — shown ONLY while navigating it or briefly
    // after a toggle; otherwise the bottom is a single status line.
    if app.bar_visible() {
        draw_mode_bar(f, modebar, app);
    }

    // Right column: shows ONLY what is PINNED (P in the agents/passport window
    // docks it here). Both pinned → 50/50; one pinned → it fills the column;
    // none → the column is hidden and the chat gets the width.
    if !app.right_collapsed {
        match (app.pin_agents, app.pin_passport) {
            (true, true) => {
                let [agents_r, passport_r] = right_split(agents_col);
                app.agents.render(f, agents_r, focus == Pane::Agents);
                app.passport.render(f, passport_r, false);
            }
            (true, false) => app.agents.render(f, agents_col, focus == Pane::Agents),
            (false, true) => app.passport.render(f, agents_col, false),
            (false, false) => {}
        }
    }
    // The command palette pops up ABOVE the chat input (bottom of the center),
    // drawn last so it overlays everything. Anchor = the bottom row of `center`
    // (the input line); the palette grows upward from there.
    if app.palette.open {
        let input_row = Rect {
            x: center.x,
            y: center.y + center.height.saturating_sub(1),
            width: center.width,
            height: 1,
        };
        app.palette.render(f, input_row);
    }
    draw_status(f, status, app);
}

/// The left "structure" view (F9): the live agents/oracle. Empty by default;
/// reflects the running (otherwise-hidden) agents when any are active.
fn draw_structure(f: &mut Frame, area: Rect, agents: &crate::agents::AgentsPane) {
    let pal = crate::theme::palette();
    let block = ratatui::widgets::Block::default()
        .borders(ratatui::widgets::Borders::ALL)
        .border_style(Style::default().fg(pal.grid))
        .title(" structure ")
        .title_style(Style::default().fg(pal.ink));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let mut lines: Vec<Line> = Vec::new();
    if agents.cards.is_empty() {
        lines.push(Line::from(Span::styled("(no agents running)", Style::default().fg(pal.muted))));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("when the oracle works here,", Style::default().fg(pal.muted))));
        lines.push(Line::from(Span::styled("its agents surface below.", Style::default().fg(pal.muted))));
    } else {
        for c in &agents.cards {
            lines.push(Line::from(vec![
                Span::styled("█ ", Style::default().fg(c.status.dot_color())),
                Span::styled(c.label.clone(), Style::default().fg(pal.ink)),
            ]));
            lines.push(Line::from(Span::styled(format!("  {}", c.task), Style::default().fg(pal.muted))));
        }
    }
    f.render_widget(Paragraph::new(lines), inner);
}

/// The mic + speaker icon glyphs sit at these fixed offsets from the RIGHT edge
/// of the mode bar (2 cells each). Shared by draw + the mouse hit-test so a
/// click on an icon toggles it. NO emoji — plain glyphs that render in DejaVu
/// Mono; COLORED when on, muted gray when off.
// Right-corner TEXT labels (no glyphs): `mic spkr`.
pub const MIC_LABEL: &str = "mic";
pub const SND_LABEL: &str = "spkr";

// Fixed-width slots so draw + hit-test agree regardless of label state.
const PREFIX_W: u16 = 4; // " ⇧⇥ "
const APPROVAL_W: u16 = 15; // " accept-edits "
const GAP_W: u16 = 1;
const PLAN_W: u16 = 6; // " plan "

/// One clickable target on the mode bar. Controls (l>r): approval toggle
/// (auto↔accept-edits) · plan toggle. (mic + speaker moved to the status bar.)
pub enum ModeBarHit {
    Approval,
    Plan,
}

fn approval_x(bar: Rect) -> u16 { bar.x + PREFIX_W }
fn plan_x(bar: Rect) -> u16 { approval_x(bar) + APPROVAL_W + GAP_W }

/// Which mode-bar control a click landed on (mouse toggling — also F2/F7).
pub fn mode_bar_hit(bar: Rect, col: u16, row: u16) -> Option<ModeBarHit> {
    if row != bar.y || bar.width < 10 {
        return None;
    }
    let (ax, px) = (approval_x(bar), plan_x(bar));
    if col >= ax && col < ax + APPROVAL_W {
        Some(ModeBarHit::Approval)
    } else if col >= px && col < px + PLAN_W {
        Some(ModeBarHit::Plan)
    } else {
        None
    }
}

/// The mode bar under the chat. STATE is shown by COLOUR (light gray off ·
/// white on · mic RED recording · spkr BLUE speaking · plan GREEN exists ·
/// approval auto GREEN / accept-edits YELLOW). SELECTION is shown by a GREEN
/// UNDERLINE under the control you are on — never BOLD, which renders BLACK on
/// a gray fg in this terminal (recorded anti-pattern). The underline carries no
/// state of its own, so a terminal that ignores SGR 58 still shows the colour.
fn draw_mode_bar(f: &mut Frame, area: Rect, app: &App) {
    use ratatui::style::Color;
    let pal = crate::theme::palette();
    let sel = app.bar_focus; // Some(0..4): 0 approval · 1 plan · 2 mic · 3 speaker
    let plan_exists = app.project.is_some() && !app.passport.nodes.is_empty();

    let light = Color::Rgb(150, 154, 162); // off / default
    let white = Color::Rgb(238, 240, 244); // selected or on
    let red = Color::Rgb(235, 70, 70); // mic recording
    let blue = Color::Rgb(90, 150, 255); // spkr speaking
    let green = pal.done;
    let yellow = Color::Yellow;

    // `fg` keeps carrying the control's STATE; a green underline marks the one
    // you have navigated to. Slot `i`: 0 approval · 1 plan · 2 mic · 3 speaker.
    let styled = |fg: Color, slot: usize| {
        let s = Style::default().fg(fg);
        if sel == Some(slot) {
            s.add_modifier(Modifier::UNDERLINED).underline_color(green)
        } else {
            s
        }
    };

    // approval: always auto=green / accept-edits=yellow (one button).
    let approval_label = if app.approval_auto { "auto" } else { "accept-edits" };
    let approval_color = if app.approval_auto { green } else { yellow };
    // plan: exists>green · plan-mode-no-plan>white · selected>white · else light.
    let plan_color = if plan_exists {
        green
    } else if app.plan_mode || sel == Some(1) {
        white
    } else {
        light
    };
    // mic + spkr moved OFF the mode bar to the START of the status row (see
    // draw_status). The mode bar now carries only the approval + plan toggles.
    let _ = (red, blue); // colours live in draw_status now
    let spans: Vec<Span> = vec![
        Span::styled(" ⇧⇥ ", Style::default().fg(light)),
        Span::styled(format!("{:^1$}", approval_label, APPROVAL_W as usize), styled(approval_color, 0)),
        Span::raw(" "),
        Span::styled(format!("{:^1$}", "plan", PLAN_W as usize), styled(plan_color, 1)),
    ];
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Abbreviate a token count: `123` > "123", `13000` > "13к", `1_200_000` >
/// "1,2м" (Russian к/м, comma decimal). Feeds the compact token counter.
pub fn fmt_tokens(n: u32) -> String {
    if n < 1_000 {
        format!("{n}")
    } else if n < 1_000_000 {
        format!("{}к", (n + 500) / 1_000)
    } else {
        let m = n as f64 / 1_000_000.0;
        format!("{:.1}м", m).replace('.', ",")
    }
}

/// Same abbreviation as [`fmt_tokens`], for the wide day-total: the all-session
/// daily sum can exceed u32's ~4.3B ceiling.
pub fn fmt_tokens_u64(n: u64) -> String {
    if n < 1_000 {
        format!("{n}")
    } else if n < 1_000_000 {
        format!("{}к", (n + 500) / 1_000)
    } else {
        let m = n as f64 / 1_000_000.0;
        format!("{:.1}м", m).replace('.', ",")
    }
}

/// The top-right readout, right-aligned on the single header row: `model ·
/// <window> · effort` (e.g. `glm-4.6 · 200к · medium`). The left of the row is
/// left empty so it reads as a floating corner label, not a full banner.
fn draw_header_right(f: &mut Frame, area: Rect, app: &App) {
    if area.height == 0 {
        return;
    }
    let pal = crate::theme::palette();
    let model = app.current_model();
    let win = fmt_tokens(crate::app::MAX_CONTEXT_TOKENS);
    let tail = format!(" · {win} · {} ", app.effort);
    // plain width to right-anchor the two-colour line.
    let w = (model.chars().count() + tail.chars().count()) as u16;
    let x = area.x + area.width.saturating_sub(w);
    let at = Rect { x, y: area.y, width: w.min(area.width), height: 1 };
    let line = Line::from(vec![
        Span::styled(model, Style::default().fg(pal.done)),
        Span::styled(tail, Style::default().fg(pal.muted)),
    ]);
    f.render_widget(Paragraph::new(line), at);
}

fn draw_status(f: &mut Frame, area: Rect, app: &App) {
    let pal = crate::theme::palette();
    // Context = a BARE ratio `used/window` (no label, no token salad) — the one
    // number that matters, kept out of the way.
    let ctx = format!("{}/{}", fmt_tokens(app.context_tokens()), fmt_tokens(crate::app::MAX_CONTEXT_TOKENS));
    // When an agent window is open full-screen, the bottom row turns into a
    // SESSION indicator: █ = the window you're looking at now (the agent's own
    // session), ○ = the main chat session waiting behind it.
    if let CenterMode::Agent(id) = &app.center {
        let label = app
            .agents
            .cards
            .iter()
            .find(|c| &c.id == id)
            .map(|c| c.label.as_str())
            .unwrap_or("agent");
        let line = Line::from(vec![
            Span::styled(
                format!(" █ agent: {label} "),
                Style::default().fg(pal.paper).bg(pal.accent),
            ),
            Span::styled("  ○ main session", Style::default().fg(pal.muted)),
            Span::styled("  ·  Esc > main", Style::default().fg(pal.muted)),
            Span::styled(format!("   {ctx}"), Style::default().fg(pal.muted)),
        ]);
        f.render_widget(Paragraph::new(line), area);
        return;
    }
    // Bottom bar (Claude-Code style): mic/spkr · bare context · a live ACTIVITY
    // dot — a spinner + the running tool, or a `ВФ ×N` badge while sub-agents
    // run. No labels, no token salad on the line.
    let off = ratatui::style::Color::Rgb(150, 154, 162);
    let mic_c = if app.recorder.is_some() { ratatui::style::Color::Rgb(235, 70, 70) } else { off };
    let spk_c = if app.is_speaking() {
        ratatui::style::Color::Rgb(90, 150, 255)
    } else if app.voice_reply {
        pal.ink
    } else {
        off
    };
    let mut spans = vec![
        Span::styled(" mic", Style::default().fg(mic_c)),
        Span::styled(" spkr", Style::default().fg(spk_c)),
        Span::styled(format!("   {ctx}"), Style::default().fg(pal.muted)),
    ];
    // Per-request token cost (this/last send) — compact, only once it's non-zero.
    if app.run_tokens > 0 {
        spans.push(Span::styled(format!("  ↑{}", fmt_tokens(app.run_tokens)), Style::default().fg(pal.done)));
    }
    // Live activity: the spinning dot names what's happening RIGHT NOW.
    let spin = crate::sphere::glyph(app.app_started.elapsed().as_millis());
    let running_agents = app
        .agents
        .cards
        .iter()
        .filter(|c| matches!(c.status, crate::agents::AgentStatus::Running))
        .count();
    if running_agents > 0 {
        spans.push(Span::styled(format!("   {spin} ВФ ×{running_agents}"), Style::default().fg(pal.accent)));
    } else if app.oracle_busy {
        let txt = match &app.oracle_tool {
            Some(t) if !t.is_empty() => format!("   {spin} {t}"),
            _ => format!("   {spin}"),
        };
        spans.push(Span::styled(txt, Style::default().fg(pal.accent)));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);

    // Clickable "fs  agents" buttons pinned to the right edge (green when
    // active/open). `agents` opens the center dashboard; `fs` toggles the tree.
    let on = |active: bool| {
        if active { pal.done } else { ratatui::style::Color::Rgb(150, 154, 162) }
    };
    let right = area.x + area.width;
    let fs_at = Rect { x: right.saturating_sub(FS_FROM_RIGHT), y: area.y, width: 2, height: 1 };
    let ag_at = Rect { x: right.saturating_sub(AGENTS_FROM_RIGHT), y: area.y, width: 6, height: 1 };
    f.render_widget(Paragraph::new(Span::styled("fs", Style::default().fg(on(!app.tree_collapsed)))), fs_at);
    f.render_widget(
        Paragraph::new(Span::styled("agents", Style::default().fg(on(matches!(app.center, CenterMode::Agents))))),
        ag_at,
    );
}

/// Fixed right-edge offsets of the status-bar buttons (shared by draw + click).
const FS_FROM_RIGHT: u16 = 10; // "fs" at right-10 (2 cells)
const AGENTS_FROM_RIGHT: u16 = 7; // "agents" at right-7 (6 cells)

/// Left-edge offsets of the status-bar mic/spkr controls — they render as
/// " mic spkr" from `status.x`, so "mic" starts at +1 (3 wide) and "spkr" at
/// +5 (4 wide). Shared by draw + hit-test.
const MIC_AT: u16 = 1;
const SPKR_AT: u16 = 5;

/// A clickable button on the status bar (bottom row): mic + speaker at the
/// LEFT, "fs"/"agents" pinned to the RIGHT.
pub enum StatusHit {
    Mic,
    Speaker,
    Fs,
    Agents,
}

/// Which status-bar button a click landed on (bottom row only).
pub fn status_hit(status: Rect, col: u16, row: u16) -> Option<StatusHit> {
    if row != status.y {
        return None;
    }
    let mic = status.x + MIC_AT;
    let spk = status.x + SPKR_AT;
    if col >= mic && col < mic + MIC_LABEL.len() as u16 {
        return Some(StatusHit::Mic);
    }
    if col >= spk && col < spk + SND_LABEL.len() as u16 {
        return Some(StatusHit::Speaker);
    }
    let right = status.x + status.width;
    let fs = right.saturating_sub(FS_FROM_RIGHT);
    let ag = right.saturating_sub(AGENTS_FROM_RIGHT);
    if col >= fs && col < fs + 2 {
        Some(StatusHit::Fs)
    } else if col >= ag && col < ag + 6 {
        Some(StatusHit::Agents)
    } else {
        None
    }
}
