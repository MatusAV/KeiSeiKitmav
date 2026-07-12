#!/usr/bin/env sh
# check-repo-ssot.sh — repo-consistency invariants (fast, no build).
#
# Guards drift classes that have each already bitten a release:
#
#   S1  plugin.json SSOT — the 3 tracked copies (root, .claude-plugin/,
#       .claude/) must share an identical `version` AND `description`.
#       Context: v0.64.2 commit #2 (878fe58) shipped after a stale copy
#       carried an old version. The version was re-synced by hand and
#       nothing kept the 3 in lockstep — this check does.
#
#   S2  marketplace.json version parity — the 2 marketplace manifests
#       (root, .claude-plugin/) must share the same version string. This
#       version is deliberately independent of plugin.json (catalog vs
#       plugin), so it is only checked against its own sibling.
#
#   S3  workspace lock hygiene — no MEMBER crate under _primitives/_rust
#       may carry its own Cargo.lock. A Cargo workspace resolves against
#       the single root lock; a member-level lock is dead cruft that
#       silently drifts from the real graph. Only crates in the workspace
#       `exclude` list (standalone builds) may keep one.
#
# NOT enforced: asset-count accuracy (the "N agents, N skills, N hooks,
# N blocks" in the plugin description). `hooks` and `skills` have no single
# mechanical SSOT (hooks chain via hooks/_lib/policy-chain.toml; some skill
# dirs are SKILL.md-less routers), so a count assertion would be flaky.
# S1 still guarantees the 3 copies agree with EACH OTHER, which is the
# drift that actually shipped.
#
# Usage:  sh scripts/check-repo-ssot.sh
# Exit:   0 clean, 1 violation(s), 2 usage / missing dependency.

set -eu

need() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "check-repo-ssot: missing dependency: $1" >&2
    exit 2
  }
}
need jq

ROOT=$(git rev-parse --show-toplevel 2>/dev/null || pwd)
cd "$ROOT"

FAIL=0
fail() { printf 'FAIL  %s\n' "$1" >&2; FAIL=1; }

# --- S1: plugin.json version + description parity --------------------------
PJ_SSOT="plugin.json"
PJ_COPIES=".claude-plugin/plugin.json .claude/plugin.json"
[ -f "$PJ_SSOT" ] || { echo "check-repo-ssot: $PJ_SSOT missing" >&2; exit 2; }

ver_ssot=$(jq -r '.version' "$PJ_SSOT")
desc_ssot=$(jq -r '.description' "$PJ_SSOT")
for pj in $PJ_COPIES; do
  [ -f "$pj" ] || { fail "S1  missing plugin.json copy: $pj"; continue; }
  v=$(jq -r '.version' "$pj")
  d=$(jq -r '.description' "$pj")
  [ "$v" = "$ver_ssot" ] \
    || fail "S1  version drift: $pj has '$v', SSOT ($PJ_SSOT) has '$ver_ssot'"
  [ "$d" = "$desc_ssot" ] \
    || fail "S1  description drift: $pj differs from SSOT ($PJ_SSOT) — copy the description verbatim"
done

# --- S2: marketplace.json version parity ----------------------------------
mv_a=$(jq -r '(.version // .metadata.version) // empty' marketplace.json 2>/dev/null || true)
mv_b=$(jq -r '(.version // .metadata.version) // empty' .claude-plugin/marketplace.json 2>/dev/null || true)
if [ -n "$mv_a" ] && [ -n "$mv_b" ] && [ "$mv_a" != "$mv_b" ]; then
  fail "S2  marketplace version drift: marketplace.json='$mv_a' vs .claude-plugin/marketplace.json='$mv_b'"
fi

# --- S3: workspace lock hygiene -------------------------------------------
WS="_primitives/_rust"
if [ -d "$WS" ]; then
  # crates the workspace explicitly excludes may keep a standalone lock
  excl=$(sed -n 's/.*exclude *= *\[\([^]]*\)\].*/\1/p' "$WS/Cargo.toml" \
           | tr ',' '\n' | tr -d ' "')
  stray=$(find "$WS" -mindepth 2 -name Cargo.lock -not -path '*/tests/fixtures/*' 2>/dev/null \
    | while IFS= read -r lk; do
        crate=$(basename "$(dirname "$lk")")
        printf '%s\n' "$excl" | grep -qx "$crate" || printf '%s\n' "$lk"
      done)
  if [ -n "$stray" ]; then
    printf 'FAIL  S3  member crate(s) carry a stray Cargo.lock (workspace uses the root lock — git rm these):\n' >&2
    printf '%s\n' "$stray" | sed 's/^/          /' >&2
    FAIL=1
  fi
fi

if [ "$FAIL" = 0 ]; then
  echo "check-repo-ssot: OK (plugin.json + marketplace parity, workspace lock hygiene)"
fi
exit "$FAIL"
