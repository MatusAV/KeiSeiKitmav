# VRC CORPUS-PREP (verified reasoning corpus — author/gate discipline)

Building the Verified Reasoning Corpus (`denis/reasoning-corpus`, MAX-free, OSS).
Supplements `corpus-doctrine` + `corpus-pipeline` (hygiene/gate) with the reasoning-trace SOP.

**Every record = a verified decomposition trace.** Spine:
`decompose(q) → solve_each → gap_check → recompose → V(oracle)`.
Fields: `track, domain, has_think, subproblems(3..7), abstention, verified, answer_check`. EN-only.

**Author ≠ gate (two-quota split):**
- TEACHER (GLM via kit / Opus / sonnet) = AUTHOR-ONLY → candidate traces to `tracks/<track>/<domain>.candidates.jsonl`. A teacher trace is a DRAFT (blind ≈50% first-pass).
- ORCHESTRATOR = GATE → scrub → oracle → doctrine-gate → keep verified → `*.verified.jsonl` → commit. GLM has no Bash (RULE 0.13) so it cannot self-gate.

**Oracle V per domain — DROP on fail (non-negotiable):** math=exact · code=sandbox · logic=checker · qa=exact/retrieve-V_R · safety=rubric+scrub. Never ship an oracle-failed trace (may ship as honest-abstention).

**Checker-oracle contract (math/logic/qa) — learned 2026-06-21:** the authored checker MUST be `def check(answer)->bool` that takes the answer and INDEPENDENTLY recomputes from constraints. Two boundary bugs the GATE must absorb (else correct traces are falsely dropped): (1) **type:** answer arrives as a str — gate tries `check(answer)`, then `check(int(answer))`, `check(float(answer))` (`30=='30'` is False). (2) **signature:** teachers often emit `def check()` (a solver, no arg) + free-form prose answers — these are UNVERIFIABLE as authored; the gate drops them (correct) and the PROMPT must demand `def check(answer)->bool` + a SHORT canonical answer string. logic/qa self-authored checkers are error-prone (wave-1: 0/32 wave-checkers valid by signature) → tighten the contract, over-generate.

**Scrub BEFORE oracle (ordering load-bearing):** drop/redact secrets (`sk-/ghp_/AKIA/Bearer/PEM/.env`), internal infra (IPs/hosts/paths/owners), MAX/theorem-IDs/rule-numbers, RULE-0.1 IP. Specifics live in gitignored `tools/scrub-patterns.internal`; docs name CLASSES only.

**Rules → pairs:** a safety/behavior rule becomes a trace where the rule is a SUB-STEP of the decomposition (user "commit" → `<think>` step "scan keys sk-/ghp_/AKIA" → refuse+redirect). That IS decomposition. M≥5 trigger paraphrases per rule.

**Mix:** ~35% pure-CoT + ~10% retrieve (think) · ~35% direct (≥40% no-think floor) · ≥20% replay · ~15% abstention across ALL domains.

**Parallelism (load-tested 2026-06-21):** GLM ≤4-5 concurrent kit units, each a SMALL disjoint task — NEVER one call over >~10 items (single-big-call stalls/never writes). Anthropic Workflow fan-out chunk to ≤~5 concurrent (>~16 → server rate-limit). **Always set model effort** (GLM `--effort max` for reasoning gen; Workflow agents `effort:'high'|'xhigh'`).

**Discipline:** benchmark-first (teacher-level lift is INVALID for strong teachers — measure at student P5); verified-citations only (resolve to file:line). Hook `vrc-record-validate.sh` enforces record schema/scrub/EN.

**Prohibited:** ship unverified trace · teacher self-gates · record missing track/domain/verified or carrying a secret · large multi-item GLM call.
