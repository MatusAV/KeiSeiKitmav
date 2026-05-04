# Self-Validating Architecture

> KeiSeiKit doesn't just ship code — it ships proof that the code matches
> its declared architecture. This document explains how that works, why it
> matters, and how to use it day-to-day.

---

## §1 The problem

Narrative claims drift from reality. A commit message says "VERIFIED:
cargo check exits clean" while cargo check actually fails. A README says
"100% test coverage" while a third of the crates have zero tests. A rule
file says "enforced by hook X" while hook X was deleted six months ago.

Drift is silent because nothing mechanical was watching. The Wave 2 audit
on 2026-05-04 caught a commit (`aaa8f36`) where the narrative and the
repo state directly contradicted each other; nothing flagged it before
the audit walked the diff by hand.

The fix is to refuse narrative as evidence. Every architectural claim
gets a machine-checkable assertion next to it, and the kit refuses to
move forward (install / commit / release) when an assertion is FAIL.
`arch/PLAN.toml` is where the assertions live; `kei-arch-map verify` is
the verifier; four enforcement layers (install / commit / agent / CI)
mean a stale claim cannot reach release.

## §2 The schema — `arch/PLAN.toml`

`arch/PLAN.toml` is the single hand-edited source of truth for what
should hold true about the repo. Each `[[module]]` is a logical unit
(file or directory); each `[[module.claim]]` is a Boolean assertion.

Skeleton:

```toml
[meta]
schema_version = 1
repo_root = "."

[[module]]
id = "cargo-workspace"
path = "_primitives/_rust/Cargo.toml"

[[module.claim]]
id = "workspace-package-has-authors"
description = "Cargo workspace declares package.authors"
evidence = { kind = "regex_match", file = "_primitives/_rust/Cargo.toml", pattern = "^authors\\s*=" }
```

Eight evidence kinds are wired into `kei-arch-map`:

| Kind | What it proves |
|---|---|
| `file_exists` | Path exists on disk |
| `regex_match` | Regex matches in file content |
| `grep_count` | Regex matches exactly N times in file content |
| `file_size` | File size in given byte range |
| `json_field` | JSON file's field equals expected literal |
| `cargo_check_clean` | `cargo check` resolves manifest cleanly (no compile) |
| `cargo_check_safe` | Full compile-check on allowlist of safe-to-build members |
| `http_status` | HTTP GET returns expected status (with SSRF guard) |

The conjunction `valid(state) = ∀c ∈ claims. c.holds(state)` is mechanically
checkable — that is the rigid schema hallucinations cannot break.

See `arch/PLAN.toml` for the live claim set and
`_primitives/_rust/kei-arch-map/src/schema.rs` for the enum definitions.

## §3 The verifier — `kei-arch-map`

Build:

```bash
cd _primitives/_rust
cargo build --release -p kei-arch-map
```

Verify the substrate against its plan:

```bash
./_primitives/_rust/target/release/kei-arch-map verify --plan arch/PLAN.toml
```

Exits 0 iff every claim PASSes. Exits 1 with a per-claim FAIL line and a
file:line pointer when any claim does not hold. Two companion subcommands:

- `kei-arch-map render` — regenerates `arch/ARCH.md` (module + claim map
  with hyperlinks).
- `kei-arch-map plan` — regenerates `arch/CLAIMS.md` (real-time PASS/FAIL
  audit table).

Workflow: edit `PLAN.toml`, run `verify` to confirm, then re-run `render`
+ `plan` so the generated docs match. Commit all three together.

## §4 The hygiene scanner — `kei-cleanup`

`kei-cleanup` is the companion code-hygiene scanner. It walks a workspace
and emits findings as JSON for downstream tooling.

```bash
cd _primitives/_rust
cargo build --release -p kei-cleanup
./target/release/kei-cleanup _primitives/_rust --json findings.json
```

Ten scanners are wired in
`_primitives/_rust/kei-cleanup/src/scanners/`:

- `dead_code` — modules / functions with no callers
- `unused_deps` — `[dependencies]` entries no source file imports
- `dep_drift` — version pin drift across workspace members
- `loc_check` — files >200 LOC / functions >30 LOC (Constructor Pattern)
- `todo_age` — `TODO` markers older than a threshold
- `coverage_map` — test-file presence per source file
- `workspace_tests` — workspace members with zero `#[test]` blocks
- `doc_warnings` — `cargo doc` warnings
- `naming_consistency` — drift between crate name / dir name / `package.name`
- `fn_extract` — function-extraction candidates

Findings flow back into `kei-registry` via
`kei-registry register-status-truth` so they become queryable alongside
DNA records.

## §5 Native enforcement — four layers

The verifier is wired to the kit at four points so a stale claim cannot
reach a release.

### 5.1 Install-time

`./install.sh` sources `install/lib-arch-verify.sh`, which runs
`kei-arch-map verify` once the substrate is on disk. **Advisory by default**
(missing `kei-arch-map` binary → skip; FAIL → warn). Block install on FAIL
by exporting `INSTALL_ARCH_STRICT=1` before calling `install.sh`. Skips
silently when `arch/PLAN.toml` is absent (project not arch-mapped yet).

### 5.2 Commit-time

`hooks/arch-verify-precommit.sh` runs as a `PreToolUse:Bash` hook on
`git commit`. If `kei-arch-map verify` fails, the commit is blocked. Bypass
with `ARCH_VERIFY_BYPASS=1 git commit ...` (visible per-call).

### 5.3 Agent-time

Every Agent spawn writes a RULE 0.16 STATUS-TRUTH MARKER block in its
final report. `hooks/agent-stub-scan.sh` parses the marker and pipes the
findings to `kei-registry register-status-truth`, which writes a row in
the `cleanup_findings` table. Agents that claim `shipped: functional`
while shipping stubs are surfaced before the orchestrator commits the
batch.

### 5.4 Release-time

`.github/workflows/ci.yml::arch-verify` builds `kei-arch-map` and runs
`verify` on every push and PR. The job exits NONZERO if any claim fails;
release tags cannot publish through a red CI.

## §6 Math DNA (Phase 2 design — partial)

`arch/MATH-DNA-DESIGN.md` proposes block formulas as the SSoT, with
`PLAN.toml` derived from formulas instead of hand-written. Every block
already carries a DNA (`<role>::<caps>::<scope_sha8>::<body_sha8>-<nonce8>`)
proving identity; Phase 2 adds a 4-tuple `(type, invariants, effects, deps)`
proving contract. Status:

- **PR-1 schema migrations (registry v1→v4)** — landed
- **PR-2 formula API** (`kei-registry::register_formula`) — landed
- **PR-3 `kei-arch-derive` emit** — landed; reads
  `[package.metadata.keisei.formula]` from per-crate `Cargo.toml` and emits
  a derived `arch/PLAN.toml`
- **PR-4 inference pass** — stubbed; `kei-arch-derive infer` currently
  prints `deferred to PR-4 (mutation-tested body→effects pass)`. Real
  body-regex walk is pending.
- **PR-5 CI coverage gate** — pending

Until PR-4 lands and crates declare `[package.metadata.keisei.formula]`,
`PLAN.toml` is hand-curated. The kit currently ships
**6 modules / 9 claims** in `PLAN.toml` against
**587 blocks** in `kei-registry` (`docs/DNA-INDEX.md` for the live count).
Coverage is intentionally narrow; Phase 2's job is to make it total.

## §7 Cookbook

### How do I add a claim?

Edit `arch/PLAN.toml`. Pick or add a `[[module]]`, then append a
`[[module.claim]]` with one of the eight evidence kinds. Run
`kei-arch-map verify` to confirm it passes, then `render` + `plan` to
refresh the generated docs. Commit all three together.

### How do I see what FAILed in CI?

Open the failing CI run, expand the `arch-verify` job. The verifier
prints one line per FAIL with claim id and a file:line pointer. The same
line appears in `.arch-verify.log` after a local install run.

### How do I bypass during emergency?

Three escape hatches, each visible per-invocation:

- **Install:** `unset INSTALL_ARCH_STRICT` (leave default — advisory mode).
- **Commit:** prefix `ARCH_VERIFY_BYPASS=1 git commit ...`.
- **CI:** there is no CI bypass. Fix the failing claim or revert the
  commit. This is intentional — the release-time gate is the last line of
  defence and bypassing it defeats the whole stack.

### How do I add my own evidence kind?

Phase 1 only: extend the `EvidenceKind` enum in
`_primitives/_rust/kei-arch-map/src/schema.rs`, write the verifier branch,
add a unit test. Phase 2 will make this declarative via the formula
4-tuple.

## §8 Limitations / honest caveats

- **Coverage is currently narrow.** 9 hand-written claims across 6
  modules; 587 substrate blocks total. Phase 2 (`kei-arch-derive`) is the
  path to 1.0 coverage, but PR-4's inference pass is a stub and crates
  have not yet declared formulas in their `Cargo.toml`.
- **PR-4 inference is unwritten.** When it lands it will produce
  low-confidence formulas (regex over body, no AST). Mutation testing per
  the design doc is the planned soundness check; not yet wired.
- **Phase 1 enum has 8 kinds.** The design doc (§2.1) sketched 14
  (8 derivable from existing + 6 new from formulas). The 6 new kinds
  (`SymbolDeclared`, `BodyShaEq`, `JsonSchema`, `CargoTestMember`,
  `EffectSubset`, `DepsClosed`) are projected DOWN to existing
  `RegexMatch` / `FileExists` / `CargoCheckClean` until Phase 2 wires the
  full schema.
- **Cross-project consumer: 0 external projects integrated yet.** The
  kit dogfoods its own substrate; the patterns are reusable but nothing
  outside this repo currently imports `kei-arch-map` as a library.
- **Commit-time hook is opt-in.** It ships as a hook script; you wire it
  into `~/.claude/settings.json` (or equivalent) the same way as every
  other kit hook. Install does not auto-enable it.

These are listed in the spirit of `[ESTIMATE-HTC: ...]` markers — the
self-validating substrate is honest about what it does and does not yet
prove. As Phase 2 lands, each line above downgrades or disappears.
