#!/usr/bin/env bash
# kei-agent-cli — invoke a KeiSeiKit agent via an external LLM CLI backend.
#
# Two entry points (both route through this script):
#
#   kei run-via <backend> <agent> "<task>"       # explicit backend
#   kei agent <agent> "<task>"                   # backend resolved from DNA:
#                                                #   1. --on=<backend> flag
#                                                #   2. agent manifest's `provider`
#                                                #   3. ~/.claude/config/primary.toml
#                                                #   4. fallback: claude
#
# Other forms:
#   kei run-via list                             # show backends + agents
#   kei agent --on=<backend> <agent> "<task>"    # override DNA backend
#   kei primary                                  # print current primary
#   kei primary <backend>                        # set primary provider
#   kei run-via --help
#
# Backends (SSoT: _primitives/cli-backends.toml):
#   claude  grok  agy  copilot  kimi  codex
#
# Reads assembled prompt from ~/.claude/agents/<agent-name>.md.
# Strips YAML frontmatter, composes with task, execs the CLI.
#
# Env overrides:
#   KEI_AGENTS_DIR       agent .md dir (default: ~/.claude/agents)
#   KEI_MANIFESTS_DIR    manifest .toml dir (default: ~/.claude/_manifests)
#   KEI_PRIMARY          override primary backend (beats config file)
#   KEI_NATIVE_AGENT=1   prefer backend's native --agent flag (grok/claude)

set -euo pipefail

KEI_AGENTS_DIR="${KEI_AGENTS_DIR:-$HOME/.claude/agents}"
KEI_MANIFESTS_DIR="${KEI_MANIFESTS_DIR:-$HOME/.claude/_manifests}"
KEI_PRIMARY_CFG="${KEI_PRIMARY_CFG:-$HOME/.claude/config/primary.toml}"
KEI_NATIVE_AGENT="${KEI_NATIVE_AGENT:-0}"
KEI_SECRETS_FILE="${KEI_SECRETS_FILE:-$HOME/.claude/secrets/.env}"

# Source secrets (RULE 0.8) so backends that need keys (e.g. glm → ZAI_API_KEY)
# can read them. Never echoed; subshell-scoped to this process.
if [ -f "$KEI_SECRETS_FILE" ]; then
  set -a; . "$KEI_SECRETS_FILE"; set +a
fi

usage() { sed -n '2,32p' "$0" | sed 's|^# \{0,1\}||'; }

# ---- backend table (SSoT mirror; kept in sync with cli-backends.toml) -----
backend_bin() {
  case "$1" in
    claude)               echo "claude"  ;;
    grok)                 echo "grok"    ;;
    agy|antigravity)      echo "agy"     ;;
    copilot)              echo "copilot" ;;
    kimi)                 echo "kimi"    ;;
    codex)                echo "codex"   ;;
    glm)                  echo "claude"  ;;  # GLM rides the claude binary (Anthropic-compat endpoint)
    *) return 1 ;;
  esac
}

backend_supports_native_agent() {
  case "$1" in claude|grok) return 0 ;; *) return 1 ;; esac
}

# ---- DNA resolver: agent → preferred backend --------------------------------
# Reads `provider = "..."` line from the manifest TOML if present.
manifest_provider() {
  local agent="$1" tomlf="$KEI_MANIFESTS_DIR/$1.toml"
  [ -f "$tomlf" ] || return 1
  # Comment-safe: split only on the first '=', strip inline '# ...' comment,
  # then trim whitespace + quotes. Tolerates `provider = "glm"  # note`.
  awk '
    /^provider[[:space:]]*=/ {
      sub(/^provider[[:space:]]*=[[:space:]]*/, "")
      sub(/[[:space:]]*#.*$/, "")
      gsub(/^[[:space:]]+|[[:space:]]+$/, "")
      gsub(/^"|"$/, "")
      print; exit
    }
  ' "$tomlf"
}

# Reads primary from config file (or KEI_PRIMARY env override).
config_primary() {
  if [ -n "${KEI_PRIMARY:-}" ]; then
    printf '%s\n' "$KEI_PRIMARY"; return 0
  fi
  [ -f "$KEI_PRIMARY_CFG" ] || return 1
  awk '
    /^provider[[:space:]]*=/ {
      sub(/^provider[[:space:]]*=[[:space:]]*/, "")
      sub(/[[:space:]]*#.*$/, "")
      gsub(/^[[:space:]]+|[[:space:]]+$/, "")
      gsub(/^"|"$/, "")
      print; exit
    }
  ' "$KEI_PRIMARY_CFG"
}

# Resolution order: explicit --on= → manifest provider → primary → claude.
resolve_backend() {
  local agent="$1" explicit="${2:-}" out=""
  if [ -n "$explicit" ]; then printf '%s\n' "$explicit"; return 0; fi
  out=$(manifest_provider "$agent" 2>/dev/null) || true
  if [ -n "$out" ]; then printf '%s\n' "$out"; return 0; fi
  out=$(config_primary 2>/dev/null) || true
  if [ -n "$out" ]; then printf '%s\n' "$out"; return 0; fi
  printf 'claude\n'
}

# ---- GLM quota marker (fail-fast on Z.ai weekly/monthly 429) ---------------
# Z.ai returns HTTP 429 code 1310 "Weekly/Monthly Limit Exhausted" when the GLM
# Coding Plan quota is spent. The claude binary treats 429 as retryable and
# backs off for ~180s before giving up (0 tokens, is_error) — so EVERY call
# hangs. To avoid that, the first observed 429 drops a marker file holding the
# reset time; later calls then fail in <1ms (no network, no extra prompt spent)
# until the reset passes, at which point the marker self-heals. Verified cause
# 2026-07-08: raw curl → 429/1310, launcher ledger → 8×~180s is_error.
_glm_quota_marker() { printf '%s' "${KEI_GLM_QUOTA_MARKER:-$HOME/.claude/.glm-quota-blocked}"; }

# Prints the human reset string + returns 0 if the marker exists and has not
# expired; else clears a stale marker and returns 1.
_glm_quota_blocked() {
  local m; m=$(_glm_quota_marker)
  [ -f "$m" ] || return 1
  local reset_epoch reset_human now
  reset_epoch=$(sed -n '1p' "$m" 2>/dev/null)
  reset_human=$(sed -n '2p' "$m" 2>/dev/null)
  now=$(date -u +%s)
  if printf '%s' "$reset_epoch" | grep -qE '^[0-9]+$' && [ "$now" -lt "$reset_epoch" ]; then
    printf '%s' "${reset_human:-unknown reset}"
    return 0
  fi
  rm -f "$m" 2>/dev/null || true   # expired → self-heal
  return 1
}

# Scans a failure payload ($1) for a rate-limit signature; if found, writes the
# marker (with the reported reset time, or a short fallback window if Z.ai did
# not report one) and returns 0. Returns 1 when the payload is not a 429.
_glm_quota_mark_from() {
  local payload="$1" m reset_human reset_epoch
  # Signatures verified 2026-07-08 against real Z.ai body AND claude-binary JSON:
  # raw body → "rate_limit_error"/'"code":"1310"'; binary → "[1310]…Limit
  # Exhausted"/'"api_error_status":429'. A bare 429 with no reported reset falls
  # back to a short block window (see below).
  case "$payload" in
    *rate_limit_error*|*"Limit Exhausted"*|*'"code":"1310"'*|*'[1310]'*|*'"api_error_status":429'*) : ;;
    *) return 1 ;;
  esac
  m=$(_glm_quota_marker)
  reset_human=$(printf '%s' "$payload" \
    | grep -oE 'reset at [0-9]{4}-[0-9]{2}-[0-9]{2} [0-9]{2}:[0-9]{2}:[0-9]{2}' \
    | head -1 | sed 's/^reset at //')
  if [ -n "$reset_human" ]; then
    reset_epoch=$(date -u -d "$reset_human" +%s 2>/dev/null || printf '')
    reset_human="$reset_human UTC"
  fi
  if [ -z "${reset_epoch:-}" ]; then
    local fb="${KEI_GLM_QUOTA_FALLBACK_SECS:-1800}"
    reset_epoch=$(( $(date -u +%s) + fb ))
    reset_human="~$(( fb / 60 ))min (Z.ai did not report a reset time)"
  fi
  printf '%s\n%s\n%s\n' "$reset_epoch" "$reset_human" \
    "auto-marked $(date -u +%Y-%m-%dT%H:%M:%SZ) from Z.ai 429" > "$m" 2>/dev/null || true
  return 0
}

# --- transient overload (HTTP 529) — SHORT cooldown, distinct from 429 quota ----
# 529 = "service temporarily overloaded" (Z.ai capacity). Unlike a 429 quota cap
# (reset hours/days out -> reroute to Opus), 529 is transient: the claude binary
# retries with backoff for MINUTES then fails. A short cooldown marker lets the
# next call fail fast instead of re-entering that retry loop. Self-heals after
# KEI_GLM_OVERLOAD_COOLDOWN_SECS. Verified [REAL: task b288wchqi.output 2026-07-14]:
# raw curl intermittent 200/529; launcher ledger call -> "API Error: 529" only
# after a multi-minute retry-backoff.
_glm_overload_marker() { printf '%s' "${KEI_GLM_OVERLOAD_MARKER:-$HOME/.claude/.glm-overload-blocked}"; }

# Prints the human cooldown string + returns 0 if an unexpired cooldown marker
# exists; else clears a stale marker and returns 1. Mirrors _glm_quota_blocked.
_glm_overload_blocked() {
  local m; m=$(_glm_overload_marker)
  [ -f "$m" ] || return 1
  local reset_epoch reset_human now
  reset_epoch=$(sed -n '1p' "$m" 2>/dev/null)
  reset_human=$(sed -n '2p' "$m" 2>/dev/null)
  now=$(date -u +%s)
  if printf '%s' "$reset_epoch" | grep -qE '^[0-9]+$' && [ "$now" -lt "$reset_epoch" ]; then
    printf '%s' "${reset_human:-cooldown active}"
    return 0
  fi
  rm -f "$m" 2>/dev/null || true   # expired → self-heal
  return 1
}

# Writes a short cooldown marker (now + KEI_GLM_OVERLOAD_COOLDOWN_SECS).
_glm_overload_mark() {
  local m cd reset_epoch
  m=$(_glm_overload_marker); cd="${KEI_GLM_OVERLOAD_COOLDOWN_SECS:-120}"
  reset_epoch=$(( $(date -u +%s) + cd ))
  printf '%s\n~%ss cooldown (Z.ai HTTP 529 overloaded)\nauto-marked %s\n' \
    "$reset_epoch" "$cd" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" > "$m" 2>/dev/null || true
}

# Scans a failure payload ($1) for a 529 overload signature; marks a short
# cooldown + returns 0 if found, else 1. Signatures: raw body 'overloaded_error'/
# '[1305]'/'temporarily overloaded'; binary JSON '"api_error_status":529'/'Error: 529'.
_glm_overload_mark_from() {
  case "$1" in
    *overloaded_error*|*'[1305]'*|*'temporarily overloaded'*|*'"api_error_status":529'*|*'Error: 529'*) : ;;
    *) return 1 ;;
  esac
  _glm_overload_mark
  return 0
}

# Cheap preflight so a FRESH exhaustion fails fast on ANY path — including the
# MCP spawn_agent 60s cap (kill_on_drop=true), which kills the launcher before
# the post-call detector above can run, so that path can't self-mark on its own.
# The probe is gated by a short-TTL "healthy" cache that every successful real
# call refreshes → during active healthy use it spends ~0 extra prompts (the
# cache is fresh, so the probe is skipped). Rejected the naive per-call probe
# for exactly that reason. Returns 1 (blocked) only on a confirmed 429.
_glm_quota_ok_cache() { printf '%s' "${KEI_GLM_QUOTA_OK_CACHE:-$HOME/.claude/.glm-quota-ok}"; }

_glm_quota_preflight() {
  [ "${KEI_GLM_PREFLIGHT:-1}" = "1" ] || return 0
  command -v curl >/dev/null 2>&1 || return 0
  local ok ttl now age
  ok=$(_glm_quota_ok_cache); ttl="${KEI_GLM_PREFLIGHT_TTL:-300}"; now=$(date -u +%s)
  if [ -f "$ok" ]; then
    age=$(( now - $(stat -c %Y "$ok" 2>/dev/null || echo 0) ))
    [ "$age" -lt "$ttl" ] && return 0     # recently confirmed healthy → no probe
  fi
  local base model body http payload
  base="${ZAI_BASE_URL:-https://api.z.ai/api/anthropic}"; model="${ZAI_MODEL:-glm-5.2}"
  body=$(curl -sS --max-time "${KEI_GLM_PREFLIGHT_TIMEOUT:-12}" -w '\n%{http_code}' \
    "$base/v1/messages" \
    -H 'content-type: application/json' -H 'anthropic-version: 2023-06-01' \
    -H "Authorization: Bearer ${ZAI_API_KEY}" \
    -d "{\"model\":\"$model\",\"max_tokens\":1,\"messages\":[{\"role\":\"user\",\"content\":\"ping\"}]}" \
    2>/dev/null) || return 0              # network hiccup → don't false-block
  http=$(printf '%s' "$body" | tail -n1)
  payload=$(printf '%s' "$body" | sed '$d')
  case "$http" in
    429) _glm_quota_mark_from "$payload" || true; return 1 ;;   # exhausted → marked
    529) _glm_overload_mark; return 2 ;;                        # overloaded → short cooldown
    200) : > "$ok" 2>/dev/null || true; return 0 ;;             # healthy → refresh cache
    *)   return 0 ;;                                            # unknown → proceed
  esac
}

# ---- backend invocation ---------------------------------------------------
backend_invoke() {
  local backend="$1" prompt="$2" agent_name="${3:-}" bin
  bin=$(backend_bin "$backend") || {
    printf '[kei-agent-cli] unknown backend: %s\n' "$backend" >&2
    return 2
  }
  command -v "$bin" >/dev/null 2>&1 || {
    printf '[kei-agent-cli] %s not on PATH. Install it or pick another backend.\n' "$bin" >&2
    return 127
  }

  # Native --agent path (grok/claude) — pass agent name + task directly.
  if [ "$KEI_NATIVE_AGENT" = "1" ] \
     && [ -n "$agent_name" ] \
     && backend_supports_native_agent "$backend"; then
    printf '[kei-agent-cli] %s --agent %s\n' "$bin" "$agent_name" >&2
    exec "$bin" --agent "$agent_name" --print "${prompt##*TASK FOR THIS RUN:}"
  fi

  # v0.41 fix: headless subprocess invocation of claude/grok without
  # --dangerously-skip-permissions returns empty (the agent's system prompt
  # asks for Read/Grep tools, but those need permission prompts which can't
  # be answered in -p mode). Pass the flag so the agent actually executes.
  # Override via KEI_AGENT_PERMISSIVE=0 to keep the strict default.
  local permissive_claude="" permissive_grok=""
  if [ "${KEI_AGENT_PERMISSIVE:-1}" = "1" ]; then
    permissive_claude="--permission-mode=bypassPermissions"
    permissive_grok="--always-approve"
  fi

  case "$backend" in
    claude)               exec "$bin" $permissive_claude -p "$prompt" ;;
    glm)
      # Z.ai GLM Coding Plan: same claude binary, Anthropic-compatible endpoint.
      # Key sourced from ~/.claude/secrets/.env (RULE 0.8). Env is scoped to the
      # exec'd subprocess only — your real `claude` backend is untouched.
      if [ -z "${ZAI_API_KEY:-}" ]; then
        printf '[kei-agent-cli] ZAI_API_KEY unset. Add it to %s\n' "$KEI_SECRETS_FILE" >&2
        printf '  echo '\''ZAI_API_KEY=...'\'' >> %s && chmod 600 %s\n' "$KEI_SECRETS_FILE" "$KEI_SECRETS_FILE" >&2
        return 3
      fi
      # Fail fast if a prior 429 marked the quota exhausted — no network call,
      # no prompt spent. Bypass with KEI_GLM_IGNORE_QUOTA=1 to force a retry.
      if [ "${KEI_GLM_IGNORE_QUOTA:-0}" != "1" ]; then
        local _blocked_until _ol_until _pf_rc
        if _blocked_until=$(_glm_quota_blocked); then
          printf '[kei-agent-cli] GLM quota exhausted — cheap routing unavailable until %s.\n' "$_blocked_until" >&2
          printf '  (Z.ai HTTP 429, weekly/monthly cap.) Reroute this agent to Opus:\n' >&2
          printf '    kei agent --on=claude %s "<task>"\n' "${agent_name:-<agent>}" >&2
          printf '  Force a GLM retry anyway: KEI_GLM_IGNORE_QUOTA=1\n' >&2
          return 4
        fi
        # Transient 529 overload cooldown active → fail fast instead of re-entering
        # the binary's multi-minute retry-backoff against an overloaded Z.ai.
        if _ol_until=$(_glm_overload_blocked); then
          printf '[kei-agent-cli] Z.ai temporarily overloaded (HTTP 529) — GLM in cooldown for %s.\n' "$_ol_until" >&2
          printf '  Transient server-side capacity issue; retry shortly, or run on Opus now:\n' >&2
          printf '    kei agent --on=claude %s "<task>"\n' "${agent_name:-<agent>}" >&2
          printf '  Force a GLM retry anyway: KEI_GLM_IGNORE_QUOTA=1\n' >&2
          return 5
        fi
        # No marker yet — cheap preflight so a fresh 429/529 fails fast even under
        # the MCP hard-cap (which kills before the post-call detector runs).
        _pf_rc=0; _glm_quota_preflight || _pf_rc=$?
        if [ "$_pf_rc" = "1" ]; then
          local _pf; _pf=$(_glm_quota_blocked || printf 'reset pending')
          printf '[kei-agent-cli] GLM quota exhausted (preflight 429) — unavailable until %s.\n' "$_pf" >&2
          printf '  Reroute this agent to Opus:\n    kei agent --on=claude %s "<task>"\n' "${agent_name:-<agent>}" >&2
          printf '  Force a GLM retry anyway: KEI_GLM_IGNORE_QUOTA=1\n' >&2
          return 4
        elif [ "$_pf_rc" = "2" ]; then
          local _pf; _pf=$(_glm_overload_blocked || printf 'cooldown pending')
          printf '[kei-agent-cli] Z.ai overloaded (preflight 529) — GLM in cooldown for %s.\n' "$_pf" >&2
          printf '  Transient; retry shortly, or run on Opus now:\n    kei agent --on=claude %s "<task>"\n' "${agent_name:-<agent>}" >&2
          printf '  Force a GLM retry anyway: KEI_GLM_IGNORE_QUOTA=1\n' >&2
          return 5
        fi
      fi
      # Ledger mode (default on; disable with KEI_GLM_LEDGER=0). Runs the call
      # with --output-format=json to capture the REAL per-run token usage that
      # the Z.ai endpoint reports, appends it to the GLM ledger, then re-emits
      # the agent's text result on stdout so the caller's contract is unchanged.
      # NOTE: the binary's total_cost_usd is NOT trusted for GLM (it prices the
      # mapped Anthropic slot, not Z.ai) — we log raw token counts only.
      if [ "${KEI_GLM_LEDGER:-1}" = "1" ] && command -v jq >/dev/null 2>&1; then
        local _out _rc
        set +e
        _out=$(env \
          ANTHROPIC_BASE_URL="${ZAI_BASE_URL:-https://api.z.ai/api/anthropic}" \
          ANTHROPIC_AUTH_TOKEN="$ZAI_API_KEY" \
          ANTHROPIC_DEFAULT_OPUS_MODEL="${ZAI_MODEL:-glm-5.2}" \
          ANTHROPIC_DEFAULT_SONNET_MODEL="${ZAI_MODEL:-glm-5.2}" \
          ANTHROPIC_DEFAULT_HAIKU_MODEL="${ZAI_SMALL_MODEL:-glm-5-turbo}" \
          "$bin" --strict-mcp-config $permissive_claude --output-format=json -p "$prompt")
        _rc=$?
        set -e
        # Detect a Z.ai quota 429 in the output (JSON or raw) → mark for
        # fast-fail so the next call doesn't burn another ~180s retry loop.
        if _glm_quota_mark_from "$_out"; then
          printf '[kei-agent-cli] Z.ai 429 (quota exhausted) — marked; further GLM calls fail fast until reset. Reroute: kei agent --on=claude %s\n' "${agent_name:-<agent>}" >&2
        elif _glm_overload_mark_from "$_out"; then
          printf '[kei-agent-cli] Z.ai 529 (overloaded) — marked; next GLM call fast-fails during cooldown. Retry shortly or reroute: kei agent --on=claude %s\n' "${agent_name:-<agent>}" >&2
        elif [ -n "$_out" ]; then
          : > "$(_glm_quota_ok_cache)" 2>/dev/null || true   # not rate-limited → refresh healthy cache (skips next preflight)
        fi
        if printf '%s' "$_out" | jq -e . >/dev/null 2>&1; then
          local _ledger="${KEI_GLM_LEDGER_FILE:-$HOME/.claude/glm-ledger.jsonl}"
          mkdir -p "$(dirname "$_ledger")" 2>/dev/null || true
          printf '%s' "$_out" | jq -c \
            --arg ts "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
            --arg agent "${agent_name:-unknown}" \
            --arg model "${ZAI_MODEL:-glm-5.2}" \
            '{ts:$ts, agent:$agent, model:$model,
              input:(.usage.input_tokens//0),
              output:(.usage.output_tokens//0),
              cache_read:(.usage.cache_read_input_tokens//0),
              cache_creation:(.usage.cache_creation_input_tokens//0),
              duration_ms:(.duration_ms//0),
              is_error:(.is_error//false)}' >> "$_ledger" 2>/dev/null || true
          printf '%s' "$_out" | jq -r '.result // empty'
        else
          # Unexpected non-JSON — emit raw so the caller still gets output.
          printf '%s' "$_out"
        fi
        return $_rc
      fi
      exec env \
        ANTHROPIC_BASE_URL="${ZAI_BASE_URL:-https://api.z.ai/api/anthropic}" \
        ANTHROPIC_AUTH_TOKEN="$ZAI_API_KEY" \
        ANTHROPIC_DEFAULT_OPUS_MODEL="${ZAI_MODEL:-glm-5.2}" \
        ANTHROPIC_DEFAULT_SONNET_MODEL="${ZAI_MODEL:-glm-5.2}" \
        ANTHROPIC_DEFAULT_HAIKU_MODEL="${ZAI_SMALL_MODEL:-glm-5-turbo}" \
        "$bin" --strict-mcp-config $permissive_claude -p "$prompt"
      ;;
    grok)                 exec "$bin" $permissive_grok --print "$prompt" ;;
    agy|antigravity)      exec "$bin" --dangerously-skip-permissions --print "$prompt" ;;
    copilot)              exec "$bin" --prompt "$prompt" ;;
    kimi)
      # Kimi has NO one-shot print mode (smoke-tested 2026-05-26): bare `kimi`
      # opens an interactive TUI that ignores piped stdin and exits with "Bye!".
      # For headless invocation we'd need an ACP client (`kimi acp` is a JSON-RPC
      # stdio server). Until KeiSeiKit ships that client, dump the composed
      # prompt to a tmpfile and open the TUI so the user can paste it in.
      tmp=$(mktemp -t kei-agent-kimi.XXXX.md)
      printf '%s\n' "$prompt" > "$tmp"
      printf '[kei-agent-cli] kimi non-interactive is unsupported (TUI only).\n' >&2
      printf '[kei-agent-cli] composed prompt saved: %s\n' "$tmp" >&2
      printf '[kei-agent-cli] copy-paste it into Kimi after the TUI opens.\n' >&2
      printf '[kei-agent-cli] (or pipe via `kimi acp` if you have an ACP client.)\n' >&2
      exec "$bin"
      ;;
    codex)                exec "$bin" exec "$prompt" ;;
  esac
}

# ---- agent loader -------------------------------------------------------
load_agent() {
  local name="$1" path
  case "$name" in
    --file=*) path="${name#--file=}" ;;
    /*|./*|*/*) path="$name" ;;
    *)         path="$KEI_AGENTS_DIR/$name.md" ;;
  esac
  if [ ! -f "$path" ]; then
    printf '[kei-agent-cli] agent not found: %s\n' "$path" >&2
    if [ -d "$KEI_AGENTS_DIR" ]; then
      printf '  Available (%s): %s\n' "$KEI_AGENTS_DIR" \
        "$(find "$KEI_AGENTS_DIR" -maxdepth 1 -name '*.md' -not -name '_*' 2>/dev/null \
           | xargs -n1 basename 2>/dev/null | sed 's/\.md$//' \
           | sort | head -8 | tr '\n' ' ')..." >&2
    fi
    return 1
  fi
  awk '
    BEGIN { in_fm=0 }
    NR==1 && /^---$/ { in_fm=1; next }
    in_fm && /^---$/ { in_fm=0; next }
    in_fm { next }
    { print }
  ' "$path"
}

# ---- primary subcommand ------------------------------------------------
handle_primary() {
  local arg="${1:-}"
  if [ -z "$arg" ]; then
    cur=$(config_primary 2>/dev/null || true)
    printf 'primary provider: %s\n' "${cur:-claude (default fallback)}"
    [ -f "$KEI_PRIMARY_CFG" ] && printf 'config: %s\n' "$KEI_PRIMARY_CFG"
    return 0
  fi
  backend_bin "$arg" >/dev/null || {
    printf '[kei-primary] unknown backend: %s\n' "$arg" >&2
    printf 'valid: claude grok agy copilot kimi codex glm\n' >&2
    return 2
  }
  mkdir -p "$(dirname "$KEI_PRIMARY_CFG")"
  printf '# kei primary — written %s\nprovider = "%s"\n' \
    "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$arg" > "$KEI_PRIMARY_CFG"
  printf 'primary provider set: %s → %s\n' "$arg" "$KEI_PRIMARY_CFG"
}

# ---- subcommands --------------------------------------------------------
case "${1:-}" in
  ""|-h|--help|help) usage; exit 0 ;;
  list|--list)
    printf 'Backends (✓ installed, ✗ missing):\n'
    for b in claude grok agy copilot kimi codex glm; do
      bin=$(backend_bin "$b")
      if p=$(command -v "$bin" 2>/dev/null); then
        printf '  %-10s ✓ %s\n' "$b" "$p"
      else
        printf '  %-10s ✗ (not on PATH)\n' "$b"
      fi
    done
    cur=$(config_primary 2>/dev/null || true)
    printf '\nprimary: %s\n' "${cur:-claude (default)}"
    printf '\nAgents (%s):\n' "$KEI_AGENTS_DIR"
    if [ -d "$KEI_AGENTS_DIR" ]; then
      find "$KEI_AGENTS_DIR" -maxdepth 1 -name '*.md' -not -name '_*' 2>/dev/null \
        | xargs -n1 basename 2>/dev/null | sed 's/\.md$/  /' | sort | column 2>/dev/null \
        || (find "$KEI_AGENTS_DIR" -maxdepth 1 -name '*.md' -not -name '_*' \
            | xargs -n1 basename | sed 's/\.md$//' | sort | head -20)
    fi
    exit 0
    ;;
  primary)
    shift
    handle_primary "${1:-}"
    exit $?
    ;;
  agent)
    # Direct-invocation passthrough: `kei-agent-cli.sh agent <name> "task"`
    # behaves identically to `kei-agent-cli.sh <name> "task"` (DNA mode).
    # Lets users call either form without surprise.
    shift
    ;;
esac

# ---- main: DNA mode (no leading backend) OR explicit run-via ------------
# Detect call shape:
#   "$1" is a known backend → run-via flow (kei run-via <backend> <agent> "task")
#   "$1" starts with --on=  → DNA flow with override
#   "$1" is anything else   → DNA flow (kei agent <agent> "task")

EXPLICIT_BACKEND=""
case "${1:-}" in
  --on=*)
    EXPLICIT_BACKEND="${1#--on=}"
    shift
    ;;
  *)
    if [ $# -ge 1 ] && backend_bin "$1" >/dev/null 2>&1; then
      EXPLICIT_BACKEND="$1"
      shift
    fi
    ;;
esac

if [ $# -lt 2 ]; then
  usage
  exit 2
fi

AGENT_REF="$1"; shift
TASK="$*"

AGENT_NAME=$(basename "${AGENT_REF#--file=}")
AGENT_NAME="${AGENT_NAME%.md}"

BACKEND=$(resolve_backend "$AGENT_NAME" "$EXPLICIT_BACKEND")

if ! AGENT_PROMPT=$(load_agent "$AGENT_REF"); then
  exit 1
fi

COMPOSED=$(printf '%s\n\n---\n\nTASK FOR THIS RUN:\n%s\n' "$AGENT_PROMPT" "$TASK")

printf '[kei-agent-cli] agent=%s backend=%s (via %s)\n' \
  "$AGENT_NAME" "$BACKEND" \
  "$([ -n "$EXPLICIT_BACKEND" ] && echo explicit \
     || ([ -n "$(manifest_provider "$AGENT_NAME" 2>/dev/null)" ] && echo manifest \
         || ([ -n "$(config_primary 2>/dev/null)" ] && echo primary || echo default)))" >&2

backend_invoke "$BACKEND" "$COMPOSED" "$AGENT_NAME"
