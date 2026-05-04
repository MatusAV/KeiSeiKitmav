# MATH-DNA — Block Formulas as the SSoT for PLAN.toml

> Design doc, Phase 2 of self-validation. Read-only research, no code changes.
> Date: 2026-05-04. Grade: §1-§3 E1 (existing infra), §4-§6 E3-E4 (untried but
> standard mechanisms). Author: researcher-code (Track B).

---

## §0 — Problem (one paragraph)

Current Phase 1 (`arch/PLAN.toml` + `kei-arch-map verify`) ships hand-written
claims for ~6 modules. With 565 blocks in `kei-registry`, the gap is 559×
manual claims that nobody will write and nobody will keep in sync. Each
block already carries a DNA (`<role>::<caps>::<scope_sha8>::<body_sha8>-<nonce8>`)
that proves identity but says nothing about contract. If every block also
carried its **formula** — type, invariants, effects, deps — then PLAN.toml
claims become the **derived projection** of the formula table onto the
existing evidence kinds. Coverage = 100% by construction; drift becomes
impossible by definition (the formula IS the SSoT).

---

## §1 — Block formula 4-tuple

Each block is the tuple

    formula(b) = (type, invariants, effects, deps)

where:

- `type` — input/output/error type signature
- `invariants` — ordered list of `Predicate` that must hold on the block's body or surface
- `effects` — set of side-effect kinds the block exercises
- `deps` — set of other block IDs the block requires at link/load/run time

All four together form the block's **observable contract** — everything a
verifier needs to mechanically check the block does what it claims.

### 1.1 Rust types

```rust
// New module: kei-registry/src/formula.rs
pub struct BlockFormula {
    pub id: BlockId,                       // = registry.dna (UNIQUE key)
    pub kind: BlockKind,                   // mirrors existing BlockType
    pub r#type: TypeSignature,
    pub invariants: Vec<Predicate>,
    pub effects: BTreeSet<EffectKind>,
    pub deps: BTreeSet<BlockId>,
    pub source: FormulaSource,             // declared | inferred | hybrid
}

pub struct BlockId(pub String);            // full DNA wire-format

pub struct TypeSignature {
    pub inputs: Vec<TypeAtom>,
    pub output: TypeAtom,
    pub errors: Vec<TypeAtom>,
}

pub enum TypeAtom {
    Unit, Bool, Int, String, Path, Json, Bytes,
    Custom(String),
}

pub enum Predicate {
    ContentRegex { file: PathBuf, pattern: String, min: u32, max: Option<u32> },
    ContentNotRegex { file: PathBuf, pattern: String },
    FileExists { path: PathBuf },
    ExitOk { cmd: String, expected: i32 },
    JsonSchema { file: PathBuf, schema: PathBuf },
    CountEq { lhs: String, rhs: String },
    HttpStatus { url: String, expected: Vec<u16> },
    CargoCheck { member: String },
    CargoTest { member: String, filter: Option<String> },
    SymbolDeclared { file: PathBuf, name: String, kind: SymbolKind },
    BodyShaEq { sha8: String },
}

pub enum SymbolKind { Fn, Struct, Enum, Trait, Const, Impl }

pub enum EffectKind {
    // Read effects
    FsRead { glob: String },
    EnvRead { var: String },
    NetIngress { port: Option<u16> },
    DbRead { backend: String },
    // Write effects
    FsWrite { glob: String },
    EnvWrite { var: String },
    NetEgress { host_glob: String },
    DbWrite { backend: String },
    // IO effects
    Stdout, Stderr, Stdin,
    // Process effects
    Exec { binary: String },
    SpawnAgent,
    // Crypto / state
    Sign,
    HashDigest,
    Sleep { seconds_max: u32 },
    // Sandbox-relevant
    FileLock { glob: String },
    NetListen { port: u16 },
    GitMutate,
    // Fallback
    Other(String),
}

pub enum FormulaSource {
    Declared,
    Inferred { confidence: u8 },
    Hybrid,
}
```

### 1.2 Why `EffectKind` matters most

Effects are the lever that turns a 565-line registry into 565 verifiable
claims with zero hand-writing. Inferred from a 1-pass scan of each block's
body:

| Body pattern (regex) | Inferred effect |
|---|---|
| `std::fs::write\|tokio::fs::write\|fs.writeFileSync` | `FsWrite{glob}` |
| `std::fs::read\|tokio::fs::read\|fs.readFile` | `FsRead{glob}` |
| `std::env::var\|os.environ\|getenv` | `EnvRead{var}` |
| `std::env::set_var\|os.environ\[.\] =` | `EnvWrite{var}` |
| `reqwest::\|hyper::client\|fetch(` | `NetEgress{host_glob}` |
| `std::process::Command\|Bash tool` | `Exec{binary}` |
| `git add\|git commit\|git push` | `GitMutate` |
| `rusqlite::Connection::open\|sqlx::` | `DbWrite\|DbRead` |
| `claude_code.spawn_agent\|Task(` | `SpawnAgent` |
| `tokio::time::sleep\|thread::sleep` | `Sleep{seconds_max}` |

Effect-kind enumeration is a closed alphabet (~25 variants); every block
maps onto a subset. Across 565 blocks, the union should be small (<25),
per-block subset is <5 on average.

---

## §2 — How current PLAN.toml claims map to formulas

Current `kei-arch-map/src/schema.rs` exposes evidence kinds: `FileExists`,
`RegexMatch`, `GrepCount`, `FileSize`, `JsonField`, `CargoCheckClean`,
`HttpStatus`. Each is **derivable** from the formula 4-tuple:

| Existing Evidence Kind | Derivation rule |
|---|---|
| `FileExists{path}` | ∃ effect `FsWrite{glob}` ∧ `path matches glob`, OR ∃ predicate `FileExists{path}` |
| `RegexMatch{file, pattern}` | ∃ predicate `ContentRegex{file, pattern, min ≥ 1}` |
| `GrepCount{file, pattern, n}` | ∃ predicate `ContentRegex{file, pattern, min=n, max=n}` |
| `FileSize{path, range}` | ∃ effect `FsWrite{glob}` ∧ size constraint metadata |
| `JsonField{file, path, expected}` | ∃ predicate `JsonSchema{file, schema}` with literal-eq leaf |
| `CargoCheckClean{manifest_dir}` | ∃ predicate `CargoCheck{member}` |
| `HttpStatus{url, expected}` | ∃ predicate `HttpStatus{url, expected}` |

Plus **6 new evidence kinds derivable directly from formulas**:

| New Evidence Kind | Source predicate |
|---|---|
| `SymbolDeclared` | `Predicate::SymbolDeclared` |
| `BodyShaEq` | `Predicate::BodyShaEq` (immutability lock) |
| `JsonSchema` | `Predicate::JsonSchema` |
| `CargoTestMember` | `Predicate::CargoTest` |
| `EffectSubset` | block.effects ⊆ declared upper-bound |
| `DepsClosed` | block.deps ⊆ registered blocks (no orphan refs) |

Key invariant: **every claim in PLAN.toml is the projection of one
predicate or one effect-rule from one block's formula**. PLAN.toml stops
being hand-edited; it becomes a generated artefact.

### 2.1 Round-trip: PLAN.toml ↔ formulas

Today's flow:
```
human writes PLAN.toml → kei-arch-map verify → exit 0/1
```

Phase 2 flow:
```
human writes <block>/.dna.toml         ← formula (per block, ~10-30 LOC)
        ↓
kei-arch-derive --emit arch/PLAN.toml  ← derived (565× claims)
        ↓
kei-arch-map verify                    ← unchanged from Phase 1
```

The `kei-arch-derive` binary is a small new tool (~300 LOC) that reads
kei-registry, joins with formula files, and projects the 4-tuple onto
the evidence schema.

---

## §3 — DNA storage: kei-registry today vs needed

### 3.1 What kei-registry already stores

```sql
blocks(id, dna, block_type, name, path, caps, scope_sha, body_sha,
       nonce, created, modified, superseded_by)
```

Indexes: `block_type`, `path`, `body_sha`. Schema version pragma'd at v1.

This gets us **identity** (DNA), **lifecycle** (created / modified /
superseded), **integrity** (body_sha for tamper-detect), and **type
classification**. It says nothing about contract.

### 3.2 Gaps for the formula 4-tuple

Three new columns, two new tables:

```sql
-- Migration v2: add formula columns
ALTER TABLE blocks ADD COLUMN type_sig_json   TEXT;
ALTER TABLE blocks ADD COLUMN effects_json    TEXT;
ALTER TABLE blocks ADD COLUMN formula_source  TEXT;
ALTER TABLE blocks ADD COLUMN formula_sha     TEXT;

-- Migration v3: predicates as separate rows (1:N from blocks)
CREATE TABLE block_predicates (
    block_id   INTEGER NOT NULL REFERENCES blocks(id),
    seq        INTEGER NOT NULL,
    kind       TEXT NOT NULL,
    args_json  TEXT NOT NULL,
    PRIMARY KEY (block_id, seq)
);

-- Migration v4: deps as separate rows (M:N)
CREATE TABLE block_deps (
    block_id   INTEGER NOT NULL REFERENCES blocks(id),
    dep_dna    TEXT NOT NULL,
    dep_kind   TEXT NOT NULL,
    PRIMARY KEY (block_id, dep_dna, dep_kind)
);
```

Existing migration machinery in `kei-registry::store::MIGRATIONS` already
supports ordered append-only DDL; this is 4 const strings + bumping
`SCHEMA_VERSION` from 1 to 4. No CRUD changes needed in `registry.rs`.

### 3.3 Per-block declaration file: `.dna.toml`

Each block carries one of:
- For Rust crates: `[package.metadata.keisei.formula]` block in `Cargo.toml`
  (precedent: `kei-tlog/Cargo.toml` already uses `[package.metadata.keisei]`)
- For hooks (shell scripts): a sidecar `<hook>.dna.toml` file
- For rules (markdown): YAML frontmatter at top of `.md` file
- For skills: existing `skill.md` frontmatter extended

Canonical TOML shape:

```toml
[formula]
type = { inputs = ["json"], output = "unit", errors = [] }
effects = ["FsWrite:traces/*.jsonl", "Exec:shasum", "Stderr"]
deps = ["primitive::_::<sha8>::<sha8>-<nonce8>"]

[[formula.invariant]]
kind = "content_not_regex"
file = "agent-fork-logger.sh"
pattern = "set -e[^u]"

[[formula.invariant]]
kind = "exit_ok"
cmd = "shellcheck -S error agent-fork-logger.sh"
expected = 0
```

---

## §4 — Migration path (Phase 2 implementation plan)

Five sequential PRs, each independently mergeable:

### PR-1: schema migrations v2-v4 in kei-registry
- Append 4 const strings to `MIGRATIONS`
- Bump `SCHEMA_VERSION` 1 → 4
- Unit tests: open old v1 DB, verify migration runs, schema is at v4
- **Verify:** `cargo test -p kei-registry migrate_v1_to_v4_idempotent`

### PR-2: formula module + `register_formula` API
- New file `kei-registry/src/formula.rs` (~150 LOC)
- Public API: `pub fn register_formula(conn, block_id, formula) -> Result<()>`
- Roundtrip test: serialize → DB → deserialize → equal

### PR-3: `kei-arch-derive` binary
- New crate `_primitives/_rust/kei-arch-derive` (~300 LOC)
- Reads registry SQLite + walks repo for `.dna.toml` files
- Emits canonical `arch/PLAN.toml` (deterministic, sorted)
- Subcommand `--check-coverage` reports `formula-present-fraction = K/N`

### PR-4: Inference pass for un-declared blocks
- `kei-arch-derive --infer` walks each block body, applies regex table
  from §1.2, writes inferred formula with `source = inferred{confidence}`
- For high-confidence cases (≥80) auto-promote to `Declared` via PR
- **Verify:** mutation tests — flip 10 random body bytes per block,
  inferred effects must change in ≥9/10 cases (sensitivity proof)

### PR-5: Coverage gate in CI
- Add to `.github/workflows/ci.yml`: `kei-arch-derive --check-coverage --min 0.95`
- Initial threshold 0.95, ratchet upward as formulas get hand-declared

Phase 1's `kei-arch-map verify` consumes the auto-generated PLAN.toml
unchanged. Phase 2 is **purely additive** to Phase 1.

---

## §5 — Math note: why this works

Define the verification problem:

    Σ = current state of repo + machine
    valid(Σ) = ∀ b ∈ Blocks. ∀ c ∈ derive(b). c.holds(Σ)

where `derive(b) : Block → Set<Claim>` is the formula → claim projection
defined in §2. Three measurable properties follow:

**Coverage** — fraction of blocks with non-empty derived claim set:

    coverage = |{b ∈ Blocks : derive(b) ≠ ∅}| / |Blocks|

Goal: `coverage = 1.0`. Today: `coverage ≈ 0.01` (6 modules / 565 blocks
hand-written). Phase 2 trivially reaches `coverage ≥ 0.95` because the
inference pass produces a non-empty set for every block whose body is
non-empty.

**Soundness** — derive correctness:

    derive ⊨ formula  ⇔  ∀ Σ. valid(Σ) ⇒ formula.holds(Σ)

Verified via mutation tests on `derive` (PR-4) and golden tests on the
projection table (§2). Mutation testing is standard practice; Rust crate
`cargo-mutants` is the toolchain.

**Idempotence** — re-running derive on unchanged registry produces
byte-identical PLAN.toml:

    derive(registry) = derive(registry)

Enforced by `BTreeSet`/`BTreeMap` ordering throughout, sha-keyed sort by
DNA, `--check-format` flag in CI to detect non-determinism.

These three together give: **PLAN.toml is a pure function of the registry
+ formulas, and PLAN.toml verification is a pure function of repo state.**
Pure functions don't drift.

---

## §6 — Open questions

**Q1: How to express RULE files (markdown, ~180 of them) as predicates?**
Recommend: YAML frontmatter declares `enforced_by: [<hook-id>, ...]`,
plus `Predicate::RuleHasLockDate{date}` — every rule must have `LOCK YYYY-MM-DD`
marker for audit trail.

**Q2: Skills — what predicate captures their contract?**
Skills (~68) have well-defined entry points (`skill.md` front-matter
`name:`, `trigger:`, `tools:`). Predicate: `SkillManifestSchema{file: skill.md}`
+ effect-bound `tools` list ⊆ formula's `effects.exec_set`.

**Q3: Orphan / stub atoms — coverage-denominator policy?**
Recommend: add a `BlockKind::Stub` flag — counted in a separate ledger.
A stub is a real architectural state ("declared, not implemented") and
deserves first-class representation.

**Q4: Cross-repo deps?**
Existing `dep_dna` field stores full DNA wire-format; scope_sha distinguishes
hooks in `~/.claude/hooks/` from primitives in `_primitives/_rust/`.

**Q5: Performance — 565 × ~5 predicates = ~2825 verifications per CI run.**
Mitigation: hash-keyed cache `(predicate, body_sha) → result` in
`~/.cache/kei-arch-map/results.sqlite`; only re-verify on body change.
Plus parallel verification by predicate-kind (Phase 3).

---

## §7 — Verdict

| Concern | Grade | Notes |
|---|---|---|
| kei-registry SQLite + DNA wire-format | **E1** | Already in production, 565 rows, schema v1 stable |
| `kei-arch-map` evidence-kind verifier | **E1** | Already shipped, used by Phase 1 PLAN.toml |
| Effect inference from regex table | **E3** | Standard static-analysis pattern; untested at our scale |
| Formula declaration via `[package.metadata.keisei]` | **E2** | `kei-tlog/Cargo.toml` already uses pattern |
| Mutation testing for derive soundness | **E4** | `cargo-mutants` mature; not yet wired into our CI |
| Cross-repo deps via DNA | **E2** | wire format already distinguishes |
| Coverage gate in CI ratchet | **E3** | Standard codecov-style discipline; threshold tuning empirical |

**Aggregate: feasible, low-risk, mostly additive to Phase 1.** Estimated
implementation surface: 5 PRs, ~800 LOC new code. Zero changes to
`kei-arch-map` Phase 1 binary; zero changes to existing `arch/PLAN.toml`
consumers.

The hard work was already done in Phase 1: defining the evidence kinds
proved that mechanical claim-checking is tractable. Phase 2 just turns
the crank from "8 hand-written claims" to "565 derived claims" via the
existing infrastructure.

**Recommended next action:** approve PR-1 (schema migrations) as the
smallest possible step that enables the rest.

---

*End of design doc.*
