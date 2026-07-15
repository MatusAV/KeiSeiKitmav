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
  if "$kr" scan \
        --kit-root "$KIT_DIR" \
        --rules-root "$HOME_DIR/.claude/rules" \
        --hooks-root "$HOME_DIR/.claude/hooks" >/dev/null 2>&1; then
    say "  registry seeded from installed substrate (idempotent)"
  else
    warn "  registry seed failed (non-fatal); encyclopedia populates on first edit"
    return 0
  fi
  # Rule fragments — companion to decompose-rules-on-edit.sh. Optional, guarded.
  local kd
  if kd="$(_registry_seed_bin kei-decompose)"; then
    "$kd" decompose-rules >/dev/null 2>&1 || true
  fi
  return 0
}
