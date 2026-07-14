#!/bin/sh
# kit-version-banner — print the KeiSeiKitmav substrate version at EVERY session
# start, so the user gets a visible "kit loaded vX.Y.Z" line. Event: SessionStart
# (stdout is injected into session context). Version SSOT is plugin.json — read
# it live so the banner never goes stale. Unlike first-run-onboard, this fires
# every session (no marker). Never fails the session: always exit 0.
set -e

PLUGIN_JSON="$HOME/.claude/plugin.json"
[ -f "$PLUGIN_JSON" ] || PLUGIN_JSON="$HOME/.claude/.claude-plugin/plugin.json"

VER="?"
if [ -f "$PLUGIN_JSON" ]; then
  if command -v jq >/dev/null 2>&1; then
    VER="$(jq -r '.version // "?"' "$PLUGIN_JSON" 2>/dev/null || echo '?')"
  else
    VER="$(sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$PLUGIN_JSON" 2>/dev/null | head -1)"
    [ -n "$VER" ] || VER="?"
  fi
fi

printf '[KeiSeiKitmav v%s] substrate loaded — hooks + agents + skills active (kei-doctor for health).\n' "$VER"
exit 0
