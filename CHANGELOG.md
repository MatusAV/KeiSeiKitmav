# Changelog

All notable changes are tagged via `git tag v*`. Latest entries first.

## Unreleased

- **fix(tooling): revive dormant `regen-counts.sh` + reconcile asset counts** —
  the README count generator carried three stale globs (`_manifests/kei-*.toml`
  matched 0 of the 37 `<name>.toml` agent manifests; `skills` counted `SKILL.md`,
  dropping the two SKILL.md-less router skills `ai-animation`/`rag-pipeline`;
  `_blocks` excluded only `README.md`, not the `INDEX.md` nav file). Fixed the
  globs to compute the intended **37 agents / 52 skills / 54 hooks / 83 blocks**,
  wired the four `<!-- count:… -->` markers into README's "By the numbers" block
  (they had never been added, so the generator was a silent no-op), and bumped the
  one drifted figure — hooks 53→54 — across all three `plugin.json` copies (S1
  parity held; `check-repo-ssot` stays green). `regen-counts.sh --check` now
  meaningfully guards README count drift, installed locally as a `pre-commit` gate.

- **fix(kei-search-core): `export` now includes sources** — `export.rs` only ever
  queried the `claims` table, so `kei-search-core export <id>` (md/json) silently
  omitted the fetched sources even though the pipeline persists them (a leftover
  from the stub era). Added `ResearchStore::sources_for` (ordered by relevance)
  and a `## Sources` section to the Markdown export (`[score] [title](url) — domain`,
  `_(none)_` when empty) plus a `sources` array to the JSON export. Verified live:
  a real run captured 10 web-search sources that were previously invisible via the
  CLI. 3 new unit tests.

- **feat(kei-search-core): live Anthropic web-search fetcher** — `kei-search-core`
  shipped with only a no-op `StubFetcher`, so the research pipeline returned zero
  sources in production. Added `AnthropicFetcher`, a `SourceFetcher` that calls
  the Messages API (`POST /v1/messages`) with the server-side `web_search`
  tool (`web_search_20260209`, model `claude-opus-4-8`) and harvests the
  returned `web_search_result` items — enriched with citation snippets, deduped
  by URL, rank-scored — into `Source`s, with a token+search cost estimate in
  microcents. **Opt-in via `ANTHROPIC_API_KEY`** (RULE 0.8 — secret via env, not
  a flag); `kei-search-core run` uses it when the key is set and falls back to
  the stub otherwise. Tunables: `KEI_SEARCH_MODEL`, `KEI_SEARCH_MAX_USES`. Raw
  HTTP over the already-vendored `reqwest` (blocking; no new dependency — only
  the `blocking` feature is unioned in). Response parsing is factored into pure
  functions covered by 5 offline unit tests (no live API call in CI).

- **test: smoke tests for the last 3 untested crates** — `kei-graph-export`,
  `kei-ping`, `kei-tlog` were the only workspace crates with zero tests. Added
  focused smoke coverage: in-crate unit tests for the pure helpers of the two
  binary crates (`sanitize_id`/`dna_prefix`/`truncate_chars` incl. Unicode
  char-boundary safety; `epoch_to_ymd_hms`/`year_days` leap rules/`iso_now`
  format), and a `tests/smoke.rs` for `kei-ping` covering `PingFilter` TTL +
  phase/branch logic and a `SqlitePingStore` send→list→upsert→clear round-trip
  on a temp DB. 12 tests, all green. Workspace test coverage is now 109/109
  crates.

- **fix(test): de-flake `kei-watch::rapid_modifies_are_debounced`** — the test
  asserted `start.elapsed() < 50ms` on its own 5-write loop, which measured the
  (loaded CI) test harness rather than the debouncer and intermittently failed
  `rust-primitives`, forcing release reruns. Dropped that wall-clock assertion
  (and the now-unused `Instant` import) and relaxed the event-count check from
  `≤2` to `<5` — i.e. assert only that debounce *coalesced* the burst, since the
  exact count depends on non-portable OS watch-event delivery timing. The precise
  `DEBOUNCE_WINDOW` behaviour stays covered deterministically by the unit tests
  in `src/debounce.rs`. Verified 15/15 stable locally.

---

## v0.65.0 — 2026-07-12

Repo-consistency + convergence cut. Two quality changes accumulated since
v0.64.2: a CI-enforced SSOT guard, and completion of the v0.24 provisioner
unification (the back-compat shell shims are removed — a mild breaking change
for anything that called `provision-hetzner.sh` / `provision-vultr.sh`
directly; use `kei-provision <backend>`).

Provisioner unification completed (v0.24 convergence item, finished):

- **`kei-provision` is now the sole VPS provisioner** — the back-compat shims
  `_primitives/provision-hetzner.sh` and `_primitives/provision-vultr.sh` (each
  a one-line `exec kei-provision <backend> "$@"` forwarder) are removed. All
  callers now invoke the unified Rust binary directly: the `vm-provision` skill
  (Phase 3), `onboard` proposals, `MANIFEST.toml` `ops`/`full` profiles, and the
  `install/lib-prereqs.sh` soft-dep gate (which now maps `kei-provision` →
  needs `hcloud` + `vultr-cli`, so the install-time warnings survive the shim
  removal). Docs (REFERENCE, INSTALL, CONVERGENCE-PLAN, deploy-hetzner-cloud,
  PLUGIN) repointed; counts corrected (shell primitives 13→11, `ops` profile
  9→7). Also fixed a latent bug: `vm-provision`'s vultr command still passed
  the pre-unification `--plan`/`--region` flags, which `kei-provision` rejects —
  now `--type`/`--location`. `kei-provision`'s own `Cargo.toml` metadata
  corrected (backends are Hetzner + Vultr, not the never-implemented
  Linode/DO/baremetal; `supersedes` now lists both shells).

Tech-debt audit fixes (quality pass, no behaviour change):

- **`scripts/check-repo-ssot.sh` — SSOT drift guard, wired into CI** — enforces
  the invariant that the 3 tracked `plugin.json` copies (root, `.claude-plugin/`,
  `.claude/`) share an identical `version` **and** `description`, the class of
  drift that shipped in v0.64.2 (#2) where a stale copy carried an old version
  after a manual re-sync. Also checks marketplace-manifest version parity and
  workspace lock hygiene (see below). Runs as a new `repo-ssot` job on GitHub
  Actions and inside the Forgejo `preflight` job. Asset-count accuracy is
  intentionally **not** enforced — `hooks`/`skills` lack a single mechanical
  SSOT, so a count assertion would be flaky; the 3-copy parity check is the
  robust invariant.
- **remove 6 stray member `Cargo.lock`** — `kei-brain-view`, `kei-discover`,
  `kei-fork`, `kei-hibernate`, `kei-migrate`, `kei-shared` are all members of the
  `_primitives/_rust` workspace, which resolves against the single root lock;
  their crate-level `Cargo.lock` files were dead cruft that could silently drift
  from the real graph. Only `kei-model-router` (in the workspace `exclude` list)
  legitimately keeps its own lock. `check-repo-ssot.sh` now guards against
  regressions.
- **README: drop stale `(v0.63.0)` from the "By the numbers" header** — the
  version-in-a-section-header was a drift vector; the counts themselves are
  accurate (`_blocks/*.md` minus README/INDEX = 83 per `build-index.sh`,
  37 manifests, 52 skill dirs).
- **`kei-search-core`: correct a stale `TODO(v0.15)`** — the no-op `StubFetcher`
  comment promised wiring "per v0.14 spec" for v0.15 (now v0.64); reworded to
  match the honest module docs (the crate is a deliberate future scaffold;
  `/research` does not depend on it).

---

## v0.64.2 — 2026-07-11

Documentation-only cut clarifying how the v0.64.1 splash fix reaches installed
setups.

- **docs: `~/.claude/bin/kei` is a symlink to the checkout** — clarifies why the
  v0.64.1 substrate-version fix also covers installed setups. `install.sh` symlinks
  `~/.claude/bin/kei -> <checkout>/bin/kei`, so `substrate_version()` follows the
  symlink to the repo's `bin/` and its `<dir>/../plugin.json` fallback resolves to
  the checkout's `plugin.json`. The splash shows the right version even when
  `~/.claude/plugin.json` is absent — which matters because the installer never
  manages that copy (it must be placed by hand).

---

## v0.64.1 — 2026-07-11

Splash version resolves correctly for in-repo runs; the accumulated
GLM-quota fail-fast work (previously Unreleased) ships in this cut.

- **in-repo substrate version resolution** — `substrate_version()` in `bin/kei`
  only searched `~/.claude/plugin.json` and `~/.local/share/keisei/plugin.json`,
  so an in-repo run (or an install without a copied `plugin.json`) fell through
  to `vunknown` in the splash. It now resolves the script's own dir (following
  symlinks) and adds `<dir>/../plugin.json` to the search list, keeping
  `plugin.json` the single SSoT.
- **GLM quota fail-fast + `kei glm-quota`** — when the Z.ai GLM Coding Plan
  weekly/monthly cap is spent it returns HTTP 429 (code 1310), which the
  `claude` binary treats as retryable and backs off on for ~180s before
  failing with 0 tokens — so every `kei agent --on=glm` call hung (verified
  2026-07-08: 8/8 ledger rows `is_error`, ~180–194s, 0 tokens). The glm path
  in `scripts/kei-agent-cli.sh` now drops a marker (`~/.claude/.glm-quota-blocked`,
  reset-epoch + human time) the first time a 429 is seen and fails subsequent
  calls in <1ms — no network, **no extra prompt spent** (a per-call preflight
  probe was rejected because it would double the prompt count against the same
  per-window cap). The marker self-heals once the reset passes; bypass with
  `KEI_GLM_IGNORE_QUOTA=1`. New `kei glm-quota` verb reports state offline
  (free) or `--live` (one probe). Detection signatures verified against the
  real Z.ai body *and* the `claude`-binary JSON. Smoke: fast-fail `exit 4` in
  0.04s (was ~180s), self-heal, `glm-quota → BLOCKED`.
- **MCP `spawn_agent` covered too (no recompile)** — `invoke_spawn_agent`
  shells the same `kei-agent-cli.sh`, so it inherits the fast-fail; but its
  `kill_on_drop` + 60s cap means a *first* exhaustion arriving via MCP dies
  before the post-call detector can self-mark. Closed with a preflight probe
  gated by a short-TTL healthy cache (`~/.claude/.glm-quota-ok`, refreshed by
  every real success) — probes ~0 times during active healthy use, but fails a
  fresh 429 fast on any path within the 60s cap. Toggle `KEI_GLM_PREFLIGHT=0`.
  Smoke: no-marker `kei agent --on=glm` → preflight 429 → re-mark → exit 4 in
  0.60s; `mcp__kei__spawn_agent(critic)` → error in <1s (was 60s timeout).

---

## v0.64.0 — 2026-07-06

Verify convention wired end-to-end through the RULE 0.14 self-audit loop:
codified guardrails must now state how they are re-confirmed, and hooks are
proven to fire before they are trusted.

- **self-audit codify quality gate (RULE 0.14-Q)** — Phase-4 `codify` /
  `create hook` routes now must carry a when-NOT-to-apply clause + a
  verification criterion, and match rigidity to finding severity
  (critical→block … low→note) before the `/escalate-recurrence` handoff
  (new `skills/self-audit/codify-quality-gate.md`). Adds a "When NOT to
  use" section to the skill. Method adapted from Trail of Bits
  `skills-curated/skill-extractor` quality guide. (`ee40c43`)
- **escalate-recurrence Verify criterion + hook smoke-test (RULE 0.14-Q)**
  — the codifier that self-audit's codify route hands off to now emits a
  `## Verify` section in every generated rule and smoke-tests the hook
  against the reproducing input (correct exit code + clean on benign
  input) *before* registration — a hook not seen fire is never registered.
  Closes the loop with `codify-quality-gate.md`. Pure-click contract
  preserved (Verify derived from Phase-0 evidence, no new question).
  (`3020172`)

---

## v0.63.0 — 2026-05-30

Post-audit fix-all release (5 HIGH + 7 MED from multi-LLM audit).

- **H1 numeric drift** — `plugin.json`, README header counts, `bin/kei`
  splash now read substrate version from `plugin.json` instead of
  hardcoding. Real counts: 37 agents / 52 skills / 53 hooks / 83 blocks.
- **H2** — stripped `_primitives/_rust/target/` + `_assembler/target/`
  from on-disk scrub tree (15 GB → 23 MB).
- **H3 workspace orphans** — `kei-graph-stream` added to umbrella
  `members`; `kei-model-router` added to umbrella `exclude` (it ships
  its own `[workspace]` for faster iteration).
- **H4** — README + `_blocks/baseline.md` claim "no abstract factories"
  narrowed to user code. `Box<dyn Trait>` for backend dispatch (memory
  / git / llm pluggable stores) is canonical Rust and stays.
- **M1** — README + bootstrap.sh dropped the "private repo /
  `gh auth login`" prerequisite. Documented `--activate-hooks`.
- **M2** — `kei status` subcommand added (previously fell through to
  `claude status` silently).
- **M3** — `hooks/hooks.json` documented as curated plugin-format
  subset; `settings-snippet.json` is canonical for classic install.
  Both intentional.
- **M5** — `kei-search-core::StubFetcher` docstring clarifies that the
  user-facing `/research` skill works via Claude tools and does NOT
  depend on this crate. The Rust stub is for future Rust-side
  automation.
- **M6** — this changelog re-summarises v0.47 → v0.63 (17 untracked
  releases).
- **M7** — `install/lib-preflight.sh` Russian comments translated to
  English.

## v0.62.0 — 2026-05-30

Deep scrub of every leftover reference to the extracted frontend
cluster across ~50 files (install scripts, hook registries, MCP TS
registry, test fixtures, docs banners, block index, Forgejo CI matrix).

## v0.61.0 — 2026-05-30

Site-building cluster (17 skills + 8 primitives + frontend-validator
agent) extracted to a private sibling repo `KeiSeiLab/frontend-studio`
for productisation. Generic image / video / 3D / animation skills
(`nano-banana`, `video-gen`, `animate`, `motion-design`,
`scroll-animation`, `3d-scene`, `visual-explainer`,
`design-inspiration`, `playwright-cli`) remain in public KSK.

## v0.60.0 — 2026-05-28

Identity unified: `git filter-repo --mailmap` rewrote every commit's
author + committer to `KeiSei LLC <hello@keilab.io>`.
`--replace-message` removed remaining identity strings from commit
message bodies. Force-pushed main + all 32 tags.

## v0.59.0 — 2026-05-28

Bootstrap wizard fix: source `kei-prompt.sh` BEFORE invoking
`prompt_profile()` (the earlier order left `kei_is_interactive`
unbound, and the wizard silently defaulted to "minimal" under
`curl|bash`). Also reads from `/dev/tty` explicitly when stdin is
the curl pipe.

## v0.58.0 — 2026-05-28

`web-install.sh` auto-resets the `origin` URL if it disagrees with the
expected `KEISEI_REPO` — fixes stale checkouts from the keigit-mirror
era after the public-github migration.

## v0.57.0 — 2026-05-28

`install/lib-launchd.sh` functions guarded for Linux — every entry
point returns 0 immediately when `KEI_OS != darwin`.

## v0.56.0 — 2026-05-28

`install/lib-prereqs.sh` detects missing C toolchain (`cc`/`gcc`) on
Linux before invoking cargo build — emits actionable install hint per
distro instead of failing inside `libc` build script.

## v0.55.0 — 2026-05-28

KeiSei LLC named as project owner in LICENSE / NOTICE / plugin.json.
Linux compatibility guards added to all 7 dev-hub-* libs +
`lib-launchd.sh`. `lib-os.sh` introduced as OS-detection cube.

## v0.54.0 — 2026-05-28

Hierarchical multi-LLM orchestration trial — used claude/grok/agy/
kimi/copilot/codex CLIs as parallel sub-orchestrators for the v0.51
audit-fix batch. Validated the "team-of-CLIs" pattern that landed as
Phase C.

## v0.53.0 — 2026-05-28

Author identity unified across all `Cargo.toml`s (50+ files). All
v0.51 audit findings addressed.

## v0.52.x — 2026-05-22 → 2026-05-27

Rules-as-cubes design + TTY-interactivity-gate rule (after 7-instance
install incident — `[ -t 1 ]` falsely false under `curl|bash` because
of tee'd stdout). v0.52.1 converted Russian user-facing strings to
English.

## v0.51.x — 2026-05-21

9 HIGH/MED audit findings fixed in parallel agents (double-audit
protocol). `kei-cortex/src/handlers/webfetch.rs` decomposed
(448 LOC → policy cube).

## v0.50.0 — 2026-05-20

UX: complete-stack profiles (`full`, `cortex`, `full-hub`, `dashboard`,
`local-mirror`) imply `--yes` to skip all 3 wizards.

## v0.49.x — 2026-05-19

Refactored installer interactivity into `scripts/kei-prompt.sh` cube
(SSoT). v0.49.1 added a visible-reason banner when the onboarding
wizard skips; v0.49.2 guards `/dev/tty` open() with a subshell probe
to survive ENXIO in sandboxes.

## v0.48.0 — 2026-05-18

`bootstrap.sh` reattaches stdin to `/dev/tty` ONCE in the entry-point
so `curl|bash` prompts actually wait for the user.

## v0.47.0 — 2026-05-17

Splash polish: yellow drop-shadow on the KeiSei wordmark; post-install
launch prompt offers to run `kei` immediately; Windows guidance text
added.

---

## Earlier (v0.38 → v0.45)

### v0.45.0 — post-install onboarding wizard + 5 prod-install bug fixes (2026-05-26)

`kei onboard` wizard auto-triggers at end of install (TTY only).
4 fixes: act_runner → gitea-runner fallback; forgejo migrate before
admin user create; zoekt graceful skip; kei-shared / launchd
deferred to v0.46.

### v0.44.0 — pre-release audit: 1 CRITICAL + 4 HIGH + 4 MEDIUM (2026-05-26)

Four-CLI parallel pre-release audit (Claude+Grok+Gemini+Copilot)
surfaced 9 real issues; all patched.

### v0.43.0 — `kei limits` + 4 audit fixes (2026-05-26)

Honest subscription-quota report. Research-grounded: only Kimi has a
public balance API (`/v1/users/me/balance`); others have none.

### v0.42.0 — re-audit fixes: 1 CRITICAL + 5 HIGH+MED (2026-05-26)

Re-audit found v0.41 fixes were incomplete. Symlink leaf bypass closed
(canonicalize + reject is_symlink); $HOME removed from default
allowed_roots; fail-closed on empty config sections.

### v0.41.0 — security hardening from Phase C dogfooding (2026-05-26)

Fail-CLOSED on missing config; path-traversal guard;
`tokio::fs` async I/O; process-group kill on Unix.

### v0.40.0 — Phase C: cross-CLI hook enforcement (2026-05-26)

`kei_bash` / `kei_edit` / `kei_write` MCP tools in `kei-mcp`.
`policy-chain.toml` SSoT for which hooks gate which tool. 3-tier
enforcement model. `kei mcp-wire` orchestrator + 5 per-CLI wire
scripts.

### v0.39.x — multi-LLM DNA (2026-05-26)

`kei pick`, `kei agent <name>` with DNA-driven provider resolution,
`kei primary` get/set, `spawn_agent` MCP tool.

### v0.38.0 — opt-in hook packs + stack profiles (2026-05-26)

Hook packs (safety / evidence / observability / epistemic /
orchestration / git-guard / stack-rust). Stack profiles (minimal /
web / ml / systems / mobile). `kei configure` re-runnable.

---

Older release notes are kept on the GitHub Releases page:
https://github.com/KeiSeiLab/KeiSeiKit/releases
