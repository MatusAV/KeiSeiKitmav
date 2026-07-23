#!/usr/bin/env bash

# Runtime gate (hooks-control skill / KEI_DISABLED_HOOKS / KEI_HOOK_PROFILE).
_KEI_LIB="$(dirname "$0")/_lib/gate.sh"; if [ -r "$_KEI_LIB" ]; then . "$_KEI_LIB"; kei_hook_gate "numeric-claims-guard" || exit 0; fi
# RULE 0.18 — Numeric claim enforcement — block Edit/Write of numeric claims
# without evidence marker. Bypass: RULE_017_BYPASS=1 prefix (kept for compat).
#
# Reads tool-call JSON on stdin (Claude Code hook protocol).

set -euo pipefail

# Bypass check
if [[ "${RULE_017_BYPASS:-0}" = "1" ]]; then
  exit 0
fi

# Read the tool input from stdin
INPUT="$(cat)"

# Extract the new content (Edit: new_string, Write: content)
NEW_CONTENT="$(printf '%s' "$INPUT" | jq -r '.tool_input.new_string // .tool_input.content // empty' 2>/dev/null)"

if [[ -z "$NEW_CONTENT" ]]; then
  exit 0
fi

# Patterns that indicate a numeric claim
# - "~N min/hour/day/week"
# - "N MB/GB/LOC/tests/crates/atomars"
# - "~$N", "$N/mo", "$N.NN", "$NN" (money needs decimal / unit / tilde / 2+ digits
#   so shell positionals $1..$9 are NOT flagged)
# - "Nm Ns", "займёт N", "should take N"
# NB: \bETA — граница слова: без неё grep -i ловит «eta» внутри LaTeX \beta/\theta/\zeta
NUMERIC_PATTERN='(~\s*[0-9]+(\.[0-9]+)?\s*(min|minute|hour|hr|day|week|month|sec|second|MB|GB|KB|LOC|line|test|crate|atomar|%|µs|ms|ns|TPS|req/s)|[0-9]+m\s*[0-9]+s|\$[0-9]+\.[0-9]+|\$[0-9]+/(mo|hr|day|run)|\$[0-9]{2,}|~\s*\$[0-9]+|should take|will take|takes about|займёт|за ~|estimated at|\bETA[: ]|approximately\s+[0-9])'

# Markers that satisfy the rule
EVIDENCE_PATTERN='\[(REAL|FROM-JOURNAL|ESTIMATE-HTC)[: ]'

# Проверки идут через here-string (grep ... <<< "$VAR"), НЕ через
# `echo "$VAR" | grep -q`: под `set -o pipefail` grep -q выходит по первому
# совпадению и закрывает pipe, echo на большом входе гибнет от SIGPIPE (141),
# и pipefail делал весь конвейер ненулевым — из-за чего наличие маркера
# читалось как отсутствие и хук НЕДЕТЕРМИНИРОВАННО блокировал валидные файлы.

# Check if numeric pattern present
if ! grep -iqE "$NUMERIC_PATTERN" <<< "$NEW_CONTENT"; then
  exit 0
fi

# Numeric pattern present — check for evidence marker
if grep -qE "$EVIDENCE_PATTERN" <<< "$NEW_CONTENT"; then
  exit 0
fi

# Violation ( || true: head -3 закрывает pipe → grep SIGPIPE, не роняем set -e)
MATCHED="$(grep -iEo "$NUMERIC_PATTERN" <<< "$NEW_CONTENT" | head -3 | tr '\n' '; ' || true)"

cat >&2 <<EOF
════════════════════════════════════════════════════════════════
  RULE 0.18 — Numeric claim without evidence marker.
════════════════════════════════════════════════════════════════

Found in Edit/Write content:
  $MATCHED

Required: append ONE of these markers in the same paragraph:
  [REAL: <file:line | commit | timestamp>]
  [FROM-JOURNAL: ~/.claude/memory/time-metrics/<file>.jsonl#<id>]
  [ESTIMATE-HTC: <one-sentence reason this can't be measured yet>]

Or write the actual measurement to a JSONL journal first:
  echo '{"kind":"task","name":"...","duration_s":...}' \\
    >> ~/.claude/memory/time-metrics/tasks.jsonl

Then cite that line.

Bypass (visible, per-call):
  RULE_017_BYPASS=1 <command>

See: ~/.claude/rules/numeric-claims-evidence.md
════════════════════════════════════════════════════════════════
EOF

exit 2
