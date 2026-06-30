# CLAUDE.md

Guidance for AI assistants (Claude Code and others) working in this
repository. Read this before making changes — the substrate is
self-generating, and several directories must **never** be hand-edited.

## What this is

**KeiSeiKit** is a multi-LLM substrate for agentic coding. The same agent
definition runs on any backend (Claude Code, Grok, Antigravity/Gemini,
GitHub Copilot, Kimi). It is a *substrate*, not a framework — you compose
primitives; it does not dictate workflow.

The repo is a polyglot monorepo: **Rust core** (two cargo workspaces) +
**TypeScript adapters** (one workspace) + **Bash hooks/scripts** + **agent
manifests/blocks** (TOML + Markdown) + **skills** (Markdown wizards).

Current scale (see `plugin.json` / README for the authoritative count):
~109 Rust crates, 37 agent manifests, 52 skills, 53 hooks, 83 blocks.

## Golden rules (read first)

1. **Never hand-edit generated agent files.** Anything under
   `.claude/agents/*.md` that begins with `<!-- GENERATED -->` is output
   of the assembler. Edit the **manifest** (`_manifests/<name>.toml`) or a
   **block** (`_blocks/<name>.md`) instead, then let the assembler
   regenerate. The `no-hand-edit-agents` hook enforces this.
2. **The build pipeline is the source of truth.**
   `_manifests/*.toml` + `_blocks/*.md` → `_assembler` (Rust) →
   `.claude/agents/*.md`. Editing a block recomposes **all** agents that
   reference it; editing a manifest recomposes that one agent.
3. **Constructor Pattern limits.** ≤200 LOC per file, ≤30 LOC per
   function. When a file grows past 200 LOC, decompose it. No mixins, no DI
   containers, no abstract factories in user code. (`Box<dyn Trait>`
   backend dispatch — e.g. `kei-store::factory::build_store` — is canonical
   Rust and stays.)
4. **Never commit secrets.** Tokens live in `~/.claude/secrets/.env` or
   `<repo>/secrets/*.env`, referenced by env-var name only. The
   `secrets-pre-guard` hook scans for leaks.
5. **Counts are generated, not typed.** Numbers in README/INSTALL are
   wrapped in `<!-- count:NAME -->...<!-- /count:NAME -->` markers and
   produced by `./scripts/regen-counts.sh`. Don't hand-edit them — run the
   script (`--check` to detect drift).

## Repository layout

| Path | What lives there | Edit? |
|---|---|---|
| `_manifests/` | Agent manifests (TOML) — source of truth for agents | ✅ yes |
| `_blocks/` | Reusable prompt blocks (Markdown, 10–50 lines, single-concern) | ✅ yes |
| `_capabilities/` | Capability bundles (`output/`, `policy/`, `quality/`, `safety/`, `scope/`, `tools/`, `verify/`) | ✅ yes |
| `_roles/` | Substrate role definitions (read-only, edit-local, git-ops, …) | ✅ yes |
| `_assembler/` | Rust binary that composes manifests+blocks → agent `.md` | ✅ yes (code) |
| `_primitives/` | Shell primitives + `_rust/` cargo workspace + `MANIFEST.toml` (install profiles) | ✅ yes |
| `_primitives/_rust/` | ~109 Rust crates (`kei-*`) — the primitive layer | ✅ yes |
| `_ts_packages/` | TypeScript workspace: 6 adapters (gmail/grok/recall/telegram/youtube/mcp-server) | ✅ yes |
| `_templates/` | Manifest/agent scaffolding templates | ✅ yes |
| `_bridges/` | Cross-tool rule bridges (`.cursorrules`, `AGENTS.md`, `GEMINI.md`, …) | ✅ yes |
| `_schemas/` | Locked JSON/TOML schemas for manifests, DNA, etc. | ⚠️ schema-locked |
| `_generated/` | Assembler output / derived artifacts | ❌ generated |
| `skills/<name>/SKILL.md` | Slash-command wizards (AskUserQuestion phase pipelines) | ✅ yes |
| `hooks/` | Bash hooks (PreToolUse/PostToolUse/Stop) + `hooks.json` | ✅ yes |
| `scripts/` | CLI helpers (`kei-*.sh`), count regen, workflow lint | ✅ yes |
| `bin/kei` | The `kei` launcher (Bash) | ✅ yes |
| `.claude/agents/*.md` | **Generated** agent prompts | ❌ never hand-edit |
| `docs/` | Architecture, schema, DNA, sleep-layer specs + `encyclopedia/` | ✅ yes |
| `tasks/`, `tests/` | Task defs + integration/battle tests | ✅ yes |
| `install.sh`, `bootstrap.sh`, `web-install.sh` | Install entry points | ✅ yes |
| `.claude-plugin/`, `plugin.json`, `marketplace.json` | Claude Code plugin manifests | ✅ yes |

## The layer model

```
Layer 4  Frontends     Claude Code plugin · cortex-ui · kei-tty · MCP clients
Layer 3  Skills        skills/<name>/SKILL.md — AskUserQuestion phase wizards
Layer 2  Recipes       TOML DAGs of atoms, run by kei-pipe
Layer 1  Primitives    Rust crates (_primitives/_rust/*) + shell primitives
Layer 0  Atoms         13 locked verbs: INGEST PARSE TRANSFORM ENRICH VALIDATE
                       DECIDE DISPATCH PERSIST RETRIEVE SEQUENCE EMIT OBSERVE EXECUTE
```

The 13-verb atom set is **locked vocabulary** — adding a verb requires a
schema change (see `docs/SUBSTRATE-SCHEMA.md`). New primitives, skills, and
recipes compose without touching lower layers. `kei-mcp` walks
`_primitives/_rust/<crate>/atoms/<verb>.md` and exposes one MCP tool per
atom (`<crate>::<verb>`).

## Build & test

There are **three** independent build roots — `cd` into the right one.

**Rust assembler** (the build pipeline itself):
```bash
cd _assembler && cargo test --release
```

**Rust primitives** (~109-crate workspace; large — link phase is heavy):
```bash
cd _primitives/_rust && cargo check --workspace
cd _primitives/_rust && cargo test --workspace --no-fail-fast
```

**TypeScript adapters**:
```bash
cd _ts_packages && npm install   # or: bun install / pnpm install
npm run build --workspaces
npm test --workspaces --if-present
```
(Note: CI installs TS with `--package-lock=false` due to a known lockfile
drift; see `.github/workflows/ci.yml`.)

**Shell**: `bash -n <file>` syntax check; `shellcheck -S warning` (advisory
in CI). Hooks and scripts are Bash; prefer POSIX `sh` where practical.

**Install dry-run**:
```bash
./install.sh --no-execute --profile=minimal   # also: dev, full
```

## CI gates (`.github/workflows/ci.yml`)

- `rust-assembler` — `cargo test --release` in `_assembler`
- `rust-primitives` — `cargo test --workspace` in `_primitives/_rust`
- `ts-packages` — build + test across Node 20/22
- `install-dry-run` — `./install.sh --no-execute` for selected profiles
- `shell-lint` — shellcheck (advisory)
- `workflow-lint` — `actionlint` + `validate-workflow-shas.sh`

Third-party actions are **SHA-pinned** (supply-chain hardening). When
editing `.github/workflows/*.yml`, run `scripts/lint-workflows.sh` and
`scripts/validate-workflow-shas.sh` first — a fabricated/force-pushed SHA
fails the workflow-lint gate. `dtolnay/rust-toolchain@stable` is a
documented, intentional exception.

## Conventions

- **Commits**: Conventional Commits — `feat:` / `fix:` / `chore:` /
  `refactor:` / `docs:` / `test:` (also `checkpoint:` / `audit:` in the
  substrate). `kei-changelog` regenerates `CHANGELOG.md` from these.
- **Rust**: `rustfmt` defaults, `clippy -W clippy::all`. Crates target
  ≤2 MB. Backend selection via trait objects + a factory function.
- **TypeScript**: project-local `tsconfig.json`, no broad `any`.
- **File size**: ≤200 LOC/file, ≤30 LOC/function (Constructor Pattern).
- **Agent manifests**: TOML `[table]` sections must precede `[[array]]`
  sections (else they nest under the last array entry). Manifests declare
  `name`, `description`, `tools`, `model`, `substrate_role`, `role`,
  `blocks`, plus `[taxonomy]` / `[lineage]`. See any file in `_manifests/`.

## Working with agents (the Constructor Pattern)

To change an agent's behavior, **edit the manifest, not the `.md`**:

1. Edit `_manifests/<agent>.toml` (or a shared `_blocks/<block>.md`).
2. The `assemble-agents` PostToolUse hook regenerates the affected
   agent(s). To rebuild manually, run the `_assembler` binary.
3. `assemble-validate` blocks commits if a manifest fails validation.

Create a new agent with the `/new-agent` skill (option-picker wizard that
composes the manifest, validates, assembles, and prints a git-commit
command). All kit agents are namespaced `kei-*`.

## Hooks

Hooks live in `hooks/*.sh` and are wired via `hooks/hooks.json` (plugin) or
`settings-snippet.json` (classic install, merged with
`./bootstrap.sh --activate-hooks`). Categories:

- **Pipeline** (load-bearing): `assemble-agents`, `assemble-validate`,
  `no-hand-edit-agents`, `tomd-preread`.
- **Safety guards**: `no-github-push`, `safety-guard`, `destructive-guard`,
  `secrets-pre-guard`, `citation-verify`, `numeric-claims-guard`.
- **Self-audit** (advisory, non-blocking): `session-end-dump`,
  `milestone-commit-hook`, `error-spike-detector`, `orchestrator-dirty-check`.

Advisory hooks warn on stderr and never block; some accept bypass env vars
(e.g. `ORCHESTRATOR_DIRTY_OK=1`, `AGENT_MIGRATION=1` for emergencies only).

## The `kei` CLI

`bin/kei` is the substrate entrypoint after install. Key subcommands:
`kei` (launch primary CLI), `kei pick` / `kei primary` (choose orchestrator),
`kei agent <name> "<task>"` (DNA-routed dispatch), `kei run-via <backend>`
(explicit backend), `kei mcp-wire` (wire kei-mcp into installed CLIs),
`kei configure`, `kei onboard`. `keisei` (Rust) is a separate exobrain CLI.

## Key documentation

- `docs/ARCHITECTURE.md` — build pipeline, layer model, hooks, git model
- `docs/PHILOSOPHY.md` / `docs/WHY.md` — design intent
- `docs/SUBSTRATE-SCHEMA.md`, `docs/AGENT-SUBSTRATE-SCHEMA.md` — locked schemas
- `docs/DNA-FORMAT.md`, `docs/DNA-INDEX.md` — agent identity / dedup
- `docs/SLEEP-LAYER.md` — three-phase nightly consolidation (A/B/C)
- `docs/INSTALL.md`, `docs/QUICKSTART.md`, `docs/PUBLISHING.md`
- `CONTRIBUTING.md` — pre-PR checklist · `DECISIONS.md` — ADR log
- `PLUGIN.md` — Claude Code plugin-install path vs classic `./install.sh`

## Before opening a PR

- `cd _primitives/_rust && cargo check --workspace` clean
- `cargo test --workspace --no-fail-fast` green (assembler + primitives)
- `cargo audit` — no critical CVEs
- Constructor Pattern respected (≤200 LOC/file, ≤30 LOC/function)
- If you changed counts-relevant sources, run `./scripts/regen-counts.sh`
- Conventional-commit prefix on every commit
- No secrets committed
