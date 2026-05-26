#!/usr/bin/env bash
# kei-limits — probe each installed CLI's remaining quota / balance.
#
# Reality (research 2026-05-26):
#   • claude  — no programmatic API. Headers per-API-call only. Admin API
#               exists but needs a separate admin key. See dashboard.
#   • grok    — same as claude. Headers per-API-call only. No file.
#   • agy     — interactive /usage slash-cmd is broken (shows 100% always,
#               forum-verified bug). No public API.
#   • copilot — no public quota API. github.com/settings/billing only.
#               Inline output during call shows usage but nothing exposed
#               for poll.
#   • kimi    — Moonshot API /v1/users/me/balance returns $ balance only
#               (no session/weekly quota). Requires MOONSHOT_API_KEY.
#
# Output:
#   stdout: human summary (default) OR JSON (--json)
#   file:   ~/.claude/pet/limits-cache.json (always, for pet to read)
#
# Polling: NOT poll-friendly. Run on demand or via launchd at >5 min intervals.
# Pet's job: read the cache; pet does NOT call this script.

set -u

# v0.43-fix #4: jq runtime guard (convention with 40+ sibling scripts).
command -v jq >/dev/null 2>&1 || {
  echo "kei-limits: jq required (brew install jq / apt install jq)" >&2
  exit 1
}

CACHE="${KEI_LIMITS_CACHE:-$HOME/.claude/pet/limits-cache.json}"
mkdir -p "$(dirname "$CACHE")"

JSON_OUT=0
QUIET=0
for arg in "$@"; do
  case "$arg" in
    --json)  JSON_OUT=1 ;;
    --quiet) QUIET=1 ;;
    -h|--help) sed -n '2,22p' "$0" | sed 's|^# \{0,1\}||'; exit 0 ;;
  esac
done

# --- per-CLI probes (each returns one JSON value to stdout) ----------------
probe_claude() {
  # No public API; produce a status marker, no live data.
  printf '%s' '{"status":"no-api","note":"see claude.ai/settings/usage","dashboard":"https://claude.ai/settings/usage"}'
}

probe_grok() {
  printf '%s' '{"status":"no-api","note":"headers-only per API call; see x.ai dashboard","dashboard":"https://x.ai"}'
}

probe_agy() {
  printf '%s' '{"status":"broken-api","note":"interactive /usage shows 100% (forum-verified bug); use Google Cloud Console","dashboard":"https://console.cloud.google.com/apis/api/generativelanguage.googleapis.com/quotas"}'
}

probe_copilot() {
  # Try gh CLI graphQL — most variants don't expose Copilot billing publicly.
  # If we ever find an endpoint, drop it in here. For now: status marker.
  printf '%s' '{"status":"no-api","note":"see github.com/settings/billing → Copilot section","dashboard":"https://github.com/settings/billing"}'
}

probe_kimi() {
  if [ -z "${MOONSHOT_API_KEY:-}" ]; then
    printf '%s' '{"status":"need-key","note":"set MOONSHOT_API_KEY in env to fetch live balance","dashboard":"https://platform.kimi.ai"}'
    return
  fi
  if ! command -v curl >/dev/null 2>&1; then
    printf '%s' '{"status":"no-curl","note":"curl required for live probe"}'
    return
  fi
  # v0.43-fix #3: feed the bearer token via stdin (--config -), NOT as
  # a curl argv. argv is visible to `ps`/`/proc/<pid>/cmdline` for any
  # local user. Audit found this on critic@claude.
  local resp
  resp=$(printf 'header = "Authorization: Bearer %s"\n' "$MOONSHOT_API_KEY" \
    | curl -sS --max-time 5 --config - \
        "https://api.moonshot.ai/v1/users/me/balance" 2>/dev/null \
    || echo '')
  if [ -z "$resp" ]; then
    printf '%s' '{"status":"probe-failed","note":"no response (network / wrong key)"}'
    return
  fi
  # v0.43-fix #2: tonumber? swallows parse errors (was: tonumber threw on
  # any non-numeric balance, emitted empty JSON, poisoned the assembler
  # --argjson → whole cache wiped).
  local avail
  avail=$(printf '%s' "$resp" | jq -r '.data.available_balance // empty' 2>/dev/null)
  if [ -z "$avail" ]; then
    printf '%s' '{"status":"probe-failed","note":"API returned non-balance response"}'
    return
  fi
  local cash voucher
  cash=$(printf '%s'   "$resp" | jq -r '.data.cash_balance // 0'    2>/dev/null)
  voucher=$(printf '%s' "$resp" | jq -r '.data.voucher_balance // 0' 2>/dev/null)
  jq -n --arg s "live" --arg a "$avail" --arg c "$cash" --arg v "$voucher" \
    '{status:$s, available_balance_usd:($a|tonumber? // 0), cash_balance_usd:($c|tonumber? // 0), voucher_balance_usd:($v|tonumber? // 0), dashboard:"https://platform.kimi.ai"}'
}

# --- assemble cache JSON ---------------------------------------------------
# v0.43-fix #1: atomic stage-and-rename. Was: `jq > "$CACHE"` truncated the
# cache BEFORE jq ran — a transient failure permanently wiped the cache.
# Now: build in tmpfile, validate non-empty, then atomic mv. Preserves
# last-known-good across probe failures.
# v0.43-fix #2 (defense-in-depth): if any individual probe returns empty
# string, substitute a status marker so --argjson never sees invalid JSON.

_safe_json() {
  local payload="$1"
  if [ -z "$payload" ]; then
    printf '%s' '{"status":"probe-empty","note":"probe returned empty result"}'
    return
  fi
  # Validate parses.
  if ! printf '%s' "$payload" | jq empty 2>/dev/null; then
    printf '%s' '{"status":"probe-invalid","note":"probe returned non-JSON"}'
    return
  fi
  printf '%s' "$payload"
}

P_CLAUDE=$(_safe_json "$(probe_claude)")
P_GROK=$(_safe_json "$(probe_grok)")
P_AGY=$(_safe_json "$(probe_agy)")
P_COPILOT=$(_safe_json "$(probe_copilot)")
P_KIMI=$(_safe_json "$(probe_kimi)")

NOW=$(date -u +%Y-%m-%dT%H:%M:%SZ)
TMP=$(mktemp "${CACHE}.XXXXXX")
if jq -n \
    --arg ts "$NOW" \
    --argjson claude  "$P_CLAUDE" \
    --argjson grok    "$P_GROK" \
    --argjson agy     "$P_AGY" \
    --argjson copilot "$P_COPILOT" \
    --argjson kimi    "$P_KIMI" \
    '{ts:$ts, claude:$claude, grok:$grok, agy:$agy, copilot:$copilot, kimi:$kimi}' \
    > "$TMP" 2>/dev/null \
   && [ -s "$TMP" ]; then
  mv -f "$TMP" "$CACHE"
else
  rm -f "$TMP" 2>/dev/null
  echo "kei-limits: cache refresh failed — keeping previous cache" >&2
  if [ ! -f "$CACHE" ]; then
    # No prior cache + assembly failed: write a minimal marker so consumers
    # don't see a missing file as their failure mode.
    printf '%s\n' '{"ts":"","status":"assembly-failed"}' > "$CACHE"
  fi
fi

# --- output ----------------------------------------------------------------
if [ "$JSON_OUT" = "1" ]; then
  cat "$CACHE"
  exit 0
fi

if [ "$QUIET" = "1" ]; then
  exit 0
fi

C0= CB= CG= CY= CR= CD=
if [ -t 1 ]; then
  C0=$'\033[0m'
  CB=$'\033[1;38;5;39m'
  CG=$'\033[32m'
  CY=$'\033[33m'
  CR=$'\033[31m'
  CD=$'\033[2m'
fi

format_one() {
  local label="$1" key="$2" data="$3"
  local status note
  status=$(printf '%s' "$data" | jq -r '.status')
  note=$(printf '%s' "$data" | jq -r '.note // ""')
  case "$status" in
    live)
      local avail
      avail=$(printf '%s' "$data" | jq -r '.available_balance_usd // empty')
      printf "  ${CG}✓${C0} %-8s \$%-8s ${CD}live (Moonshot balance)${C0}\n" "$label" "$avail"
      ;;
    no-api|need-key)
      printf "  ${CY}?${C0}  %-8s ${CD}%s${C0}\n" "$label" "$note"
      ;;
    broken-api)
      printf "  ${CR}✗${C0} %-8s ${CD}%s${C0}\n" "$label" "$note"
      ;;
    *)
      printf "  ${CY}?${C0}  %-8s ${CD}%s${C0}\n" "$label" "$note"
      ;;
  esac
}

cat <<EOF

${CB}╔════════════════════════════════════════════════════════════╗
║  KeiSeiKit · CLI subscription limits                         ║
╚════════════════════════════════════════════════════════════╝${C0}

EOF

CACHE_CONTENT=$(cat "$CACHE")
for cli in claude grok agy copilot kimi; do
  data=$(printf '%s' "$CACHE_CONTENT" | jq -c ".$cli")
  format_one "$cli" "$cli" "$data"
done

echo
echo "${CD}cached: $CACHE${C0}"
echo "${CD}note:   no CLI exposes session/weekly quota in a poll-friendly way.${C0}"
echo "${CD}        See dashboards via 'open <url>' from --json output.${C0}"
