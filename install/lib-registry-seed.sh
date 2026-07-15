set -e
# shellcheck shell=bash
# lib-registry-seed.sh — one-time substrate registry seed.
#
# Populates ~/.claude/registry.sqlite from the installed substrate so the
# encyclopedia hooks wired in settings-snippet.json (auto-register-on-edit /
# auto-encyclopedia-refresh / decompose-rules-on-edit) regenerate a COMPLETE
# docs/DNA-INDEX.md on the first substrate edit — instead of one block at a
# time from an empty DB. `kei-registry scan` is idempotent (unchanged content
# = no-op) and creates the DB if absent, so re-running install is safe.
#
# Defensive: NEVER fails the install. Skips silently when kei-registry is not
# installed (minimal profile / KEI_SKIP_RUST) or KEI_SKIP_REGISTRY_SEED=1.
#
# Requires: say / warn from lib-log.sh.
# Reads globals: $KIT_DIR, $HOME_DIR.

# Resolve a substrate binary: PATH first (canonical ~/.cargo/bin), then the
# source repo's target/release as a bootstrap fallback. Echoes path or returns 1.
_registry_seed_bin() {
  local name="$1" p
  command -v "$name" 2>/dev/null && return 0
  for p in "$HOME_DIR/.cargo/bin/$name" \
           "$KIT_DIR/_primitives/_rust/target/release/$name"; do
    [ -x "$p" ] && { printf '%s\n' "$p"; return 0; }
  done
  return 1
}

# Seed the substrate registry. Idempotent, non-fatal, opt-out via env.
seed_registry() {
  if [ "${KEI_SKIP_REGISTRY_SEED:-0}" = "1" ]; then
    say "registry seed skipped (KEI_SKIP_REGISTRY_SEED=1)"
    return 0
  fi
  local kr
  kr="$(_registry_seed_bin kei-registry)" || {
    say "  kei-registry absent — skipping registry seed (encyclopedia populates incrementally)"
    return 0
  }
  say "seeding substrate registry (~/.claude/registry.sqlite)"
  # Idempotent full scan of the installed substrate. Default DB (the same one
  # the encyclopedia hooks read — never pass --db so both agree).
  # index-substrate is KIT-ROOT-scoped (walks only substrate dirs under the kit:
  # primitives/skills/hooks/atoms/blocks/capabilities/roles) — NOT ~/.claude.
  # The previous `scan --rules-root ~/.claude/rules --hooks-root ~/.claude/hooks`
  # registered ~60 installed-copy blocks with absolute `/home/<user>/.claude`
  # paths into docs/DNA-INDEX.md — a COMMITTED, shared artefact — inflating it
  # 223->405 and baking machine-specific paths into a repo file. index-substrate
  # yields repo-relative paths that render identically on any checkout, and it
  # skips node_modules/target so it stays fast. (KIT_DIR = repo root.)
  if "$kr" index-substrate "$KIT_DIR" >/dev/null 2>&1; then
    say "  registry seeded from kit substrate (repo-scoped, idempotent)"
  else
    warn "  registry seed failed (non-fatal); encyclopedia populates on first edit"
    return 0
  fi
  # NOTE: we deliberately do NOT run `kei-decompose decompose-rules` at seed
  # time. It writes rule fragments to ~/.claude/registry-fragments and registers
  # them as Rule blocks whose paths are OUTSIDE the kit root — the encyclopedia
  # cannot relativise them, so they'd re-introduce absolute ~/.claude paths into
  # the committed docs/DNA-INDEX.md (the very pollution this seed avoids). The
  # decompose-rules-on-edit.sh hook regenerates those fragments on the first rule
  # edit, matching the incremental-population model used for the encyclopedia.
  return 0
}
