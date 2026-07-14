# RULE 0.2 — Rust First

Default language for all new code is **Rust**. Choosing another language
requires an explicit architectural reason from the allowed exception list
below, stated in chat and recorded in the project's `DECISIONS.md`.

## Allowed exceptions (cite the number)

1. Large ML training (> 10M parameters) — Python for the training loop only;
   the inference / production path is rewritten in Rust.
2. Existing language-locked project being extended (respect the incumbent
   stack rather than introducing a second one).
3. Platform-native UI (Swift/SwiftUI for Apple, Kotlin for Android, etc.).
4. Browser / DOM runtime (TypeScript).
5. Under-50-line throwaway script with no reuse expectation.
6. External binding that only exists in Python/JS (no native Rust crate).
7. Explicit user override with a stated reason.

## Not acceptable reasons

"Python is the ML language", "I know Python better", "faster iteration in
Python", "matplotlib is easier", "we will rewrite in Rust later", "just a
prototype", "libraries".

For one-off calculations prefer, in order: existing Rust in the project → a
Rust one-shot (`cargo run` on an `examples/*.rs`) → `awk` / `bc` / `dc` →
`jq` → `node`. Reach for Python only under an exception above.

## Enforcement (current)

This rule is **advisory**, enforced by:

- `rust-first.sh` (UserPromptSubmit) — reminds on language-choice keywords.
- The Rust-first default baked into the `code-implementer-*` / `ml-implementer`
  agent manifests.

The hard-blocking `no-python-without-approval.sh` PreToolUse gate that
previously rejected every `python`/`python3` invocation was **removed in
v0.69.0** (per user decision — it was redundant friction for legitimate
one-offs). Python is no longer blocked at the tool layer; prefer Rust by
judgment and cite an exception when you don't.
