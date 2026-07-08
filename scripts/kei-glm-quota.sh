#!/usr/bin/env bash
# kei glm-quota — report GLM (Z.ai) quota state.
#
# Default is OFFLINE: it reads the fail-fast marker file that kei-agent-cli.sh
# drops when Z.ai returns HTTP 429 (weekly/monthly cap, code 1310). Costs
# nothing — no network, no prompt spent.
#
#   kei glm-quota          # offline: report from marker
#   kei glm-quota --live   # send a minimal probe to Z.ai (spends ONE prompt)
#                          # and refresh the marker from the real answer
#
# Marker format (3 lines): <reset_epoch> / <reset_human> / <note>
set -euo pipefail

MARKER="${KEI_GLM_QUOTA_MARKER:-$HOME/.claude/.glm-quota-blocked}"
SECRETS="${KEI_SECRETS_FILE:-$HOME/.claude/secrets/.env}"

live=0
[ "${1:-}" = "--live" ] && live=1

if [ "$live" = "1" ]; then
  # shellcheck disable=SC1090
  [ -f "$SECRETS" ] && { set -a; . "$SECRETS"; set +a; }
  if [ -z "${ZAI_API_KEY:-}" ]; then
    printf 'ZAI_API_KEY unset — cannot probe (%s).\n' "$SECRETS" >&2
    exit 3
  fi
  base="${ZAI_BASE_URL:-https://api.z.ai/api/anthropic}"
  model="${ZAI_MODEL:-glm-5.2}"
  body=$(curl -sS --max-time 15 -w '\n%{http_code}' "$base/v1/messages" \
    -H 'content-type: application/json' \
    -H 'anthropic-version: 2023-06-01' \
    -H "Authorization: Bearer ${ZAI_API_KEY}" \
    -d "{\"model\":\"$model\",\"max_tokens\":1,\"messages\":[{\"role\":\"user\",\"content\":\"ping\"}]}" \
    2>/dev/null || true)
  http=$(printf '%s' "$body" | tail -n1)
  payload=$(printf '%s' "$body" | sed '$d')
  case "$http" in
    429)
      reset=$(printf '%s' "$payload" \
        | grep -oE 'reset at [0-9]{4}-[0-9]{2}-[0-9]{2} [0-9]{2}:[0-9]{2}:[0-9]{2}' \
        | head -1 | sed 's/^reset at //')
      if [ -n "$reset" ]; then
        epoch=$(date -u -d "$reset" +%s 2>/dev/null || printf '')
        [ -n "$epoch" ] && printf '%s\n%s\n%s\n' "$epoch" "$reset UTC" \
          "live-probe $(date -u +%FT%TZ)" > "$MARKER"
      fi
      printf 'GLM quota: EXHAUSTED (HTTP 429). Reset: %s\n' "${reset:-unknown}"
      printf '  Bulk should route to Opus: kei agent --on=claude <name> "<task>"\n'
      exit 0 ;;
    200)
      rm -f "$MARKER" 2>/dev/null || true
      printf 'GLM quota: OK (HTTP 200, live probe succeeded). Marker cleared.\n'
      exit 0 ;;
    401|403)
      printf 'GLM auth failed: HTTP %s — check ZAI_API_KEY in %s.\n' "$http" "$SECRETS"
      exit 0 ;;
    *)
      printf 'GLM probe: HTTP %s (unexpected). Body: %s\n' "${http:-?}" "$(printf '%s' "$payload" | head -c 200)"
      exit 0 ;;
  esac
fi

# ---- Offline (default): report from the marker -----------------------------
if [ ! -f "$MARKER" ]; then
  printf 'GLM quota: no block recorded (marker absent) — assumed OK.\n'
  printf "  Confirm against Z.ai with 'kei glm-quota --live' (spends one prompt).\n"
  exit 0
fi

reset_epoch=$(sed -n '1p' "$MARKER" 2>/dev/null)
reset_human=$(sed -n '2p' "$MARKER" 2>/dev/null)
note=$(sed -n '3p' "$MARKER" 2>/dev/null)
now=$(date -u +%s)

if printf '%s' "$reset_epoch" | grep -qE '^[0-9]+$' && [ "$now" -lt "$reset_epoch" ]; then
  mins=$(( (reset_epoch - now) / 60 ))
  printf 'GLM quota: BLOCKED until %s (~%d min left).\n' "${reset_human:-?}" "$mins"
  [ -n "$note" ] && printf '  %s\n' "$note"
  printf '  Cheap GLM routing fails fast; reroute bulk to Opus: kei agent --on=claude <name> "<task>"\n'
  printf "  Marker self-clears at reset; 'kei glm-quota --live' re-checks now.\n"
  exit 0
else
  rm -f "$MARKER" 2>/dev/null || true
  printf 'GLM quota: marker expired — GLM should be healthy again (marker cleared).\n'
  exit 0
fi
