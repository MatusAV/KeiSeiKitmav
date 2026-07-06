# Codify Quality Gate (RULE 0.14-Q)

Applied when a Phase-4 finding is routed to `codify` or `create hook`.
Before the `/escalate-recurrence` handoff is emitted, the suggested
artifact (rule / hook / wiki entry) MUST carry the three fields below —
or the handoff prints an explicit `TODO:` placeholder for each missing
one, for the downstream skill to fill. Method adapted from Trail of Bits
`skills-curated/skill-extractor` quality guide.

## 1 — When-NOT-to-apply clause (mandatory)

Every codified rule/hook states the case where it must NOT fire, so it
does not over-trigger and become noise the user learns to ignore. A rule
with no explicit negative scope is rejected — echo:

> "Codify blocked: <class> has no 'when NOT to apply' clause. Add one
>  before making it permanent."

## 2 — Verification criterion (mandatory)

State how we will KNOW the artifact actually prevents the recurrence —
the observable that must flip fail→pass:

- rule → the session-trace signal (`event_class`) that must drop to zero
  in the next audit.
- hook → the exact exit code / blocked call the hook must produce on the
  reproducing input.

No verification → the artifact is advisory-only and may NOT be marked
`enforce`.

## 3 — Scope-under-risk (prescriptiveness match)

Match the rigidity of the codified artifact to the finding's Phase-2
severity — do not hard-block low-risk ergonomics, do not merely warn on
a critical finding:

| Finding severity           | Codified rigidity                        |
|----------------------------|------------------------------------------|
| critical (security/data)   | hook `block` (exit 2), no override        |
| high (breaks work/prod)    | hook `block` with a `KEI_*_BYPASS` escape |
| medium (slows work)        | hook `warn` (exit 0 + message)            |
| low (ergonomics/style)     | wiki/rule note only, no hook              |

## Verify-criterion

- Each `codify` / `create hook` entry in `ROUTES` carries fields 1 + 2,
  or an explicit `TODO:` placeholder for each missing one.
- The rigidity chosen in field 3 matches the finding `severity` from
  Phase 2. A mismatch prints a one-line advisory (does not block).
