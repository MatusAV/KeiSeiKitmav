#!/usr/bin/env bash

# Runtime gate (hooks-control skill / KEI_DISABLED_HOOKS / KEI_HOOK_PROFILE).
_KEI_LIB="$(dirname "$0")/_lib/gate.sh"; if [ -r "$_KEI_LIB" ]; then . "$_KEI_LIB"; kei_hook_gate "default-change-guard" || exit 0; fi
# RULE 0.25 — DEFAULTS ARE NOT CHANGED SILENTLY (2026-07-22 LOCK) enforcement.
#
# Blocks a write that changes a DEFAULT declaration — the value that applies
# when nobody chose anything: LLM provider/model, agent routing, default
# agent/profile, pinned ports, sandbox flags.
#
# Two shapes of write are gated, because they need different evidence:
#
#   Edit / Write / kei_edit / kei_write — structured. Old and new text are both
#     available, so the gate fires only when a declared value actually MOVES.
#     Mentioning a default in added prose does not trip it.
#
#   Bash — unstructured. `sed -i`, a heredoc into python3, `tee`, a redirect:
#     the old value is not in the payload and the new one is buried in shell
#     text. So the test is coarser: a command that WRITES somewhere AND carries
#     a default declaration in its text. That is deliberately conservative —
#     this path existed precisely because the structured gate was bypassed
#     through Bash on 2026-07-22.
#
# Trigger:  PreToolUse, matchers "Write|Edit", "mcp__kei__kei_write|mcp__kei__kei_edit", "Bash"
# Severity: enforce (exit 2 — the call is refused, stderr goes back to Claude)
#
# Bypass, set AFTER the user answered — never instead of asking:
#   structured paths : export DEFAULT_CHANGE_APPROVED=1
#   Bash path        : prefix the command itself, e.g.
#                        DEFAULT_CHANGE_APPROVED=1 sed -i ... providers.toml
#                      An env var set outside is invisible in the transcript;
#                      an inline prefix is not, which is the point — the
#                      approval should be as reviewable as the change.
#
# Canon it protects: bulk/coding -> GLM, judgment -> Opus (~/.claude/CLAUDE.md).
# Incident of record: keiseikit-web provider glm -> claude (b4836fa, reverted c98cff0).

set -u

[[ "${DEFAULT_CHANGE_APPROVED:-0}" == "1" ]] && exit 0

PAYLOAD=$(cat)
TOOL_NAME=$(printf '%s' "$PAYLOAD" | jq -r '.tool_name // ""' 2>/dev/null)

# Default DECLARATIONS: key + value, in the positions where a default is set.
# Deliberately not bare `provider` — that also matches call arguments.
DECL='(--default-provider[[:space:]]+[A-Za-z0-9_.-]+'
DECL+='|default[-_]provider[[:space:]]*[:=][[:space:]]*\\*["'"'"']?[A-Za-z0-9_.-]+'
DECL+='|DEFAULT_PROVIDER[[:space:]]*=[[:space:]]*\\*["'"'"']?[A-Za-z0-9_.-]+'
DECL+='|provider[[:space:]]*[:=][[:space:]]*\\*["'"'"'][A-Za-z0-9_.-]+\\*["'"'"']'
DECL+='|primary[[:space:]]*[:=][[:space:]]*\\*["'"'"']?[A-Za-z0-9_.-]+'
DECL+='|defaultAgent[[:space:]]*[:=][[:space:]]*\\*["'"'"'][A-Za-z0-9_.-]+\\*["'"'"']'
DECL+='|ANTHROPIC_MODEL[[:space:]]*=[[:space:]]*\\*["'"'"']?[A-Za-z0-9_.:-]+'
DECL+='|\bmodel[[:space:]]*=[[:space:]]*\\*["'"'"'][A-Za-z0-9_.:-]+\\*["'"'"']'
DECL+='|KEI_HOOK_PROFILE[[:space:]]*=[[:space:]]*\\*["'"'"']?[A-Za-z0-9_.-]+'
DECL+=')'

extract() {
    # Normalise whitespace so pure reindentation is not a "change". `sed` is
    # line-oriented on purpose: `tr -s '[:space:]'` also squeezes newlines, which
    # collapsed every match onto one line and left `sort -u` nothing to dedupe.
    printf '%s' "$1" | grep -oiE "$DECL" 2>/dev/null \
        | sed -E 's/[[:space:]]+/ /g; s/[[:space:]]*$//' | sort -u
}

# Emit the refusal and exit 2. $1 = where, $2 = "was" line, $3 = "now" line,
# $4 = the bypass form appropriate to this path.
refuse() {
    {
        echo "[RULE 0.25 BLOCK] попытка сменить ДЕФОЛТ без согласования."
        echo "  где:   $1"
        echo "  было:  $2"
        echo "  стало: $3"
        echo
        echo "  Дефолт — канон пользователя, не деталь реализации. Внешняя поломка"
        echo "  (429, истёкший ключ, недоступный сервис) — инцидент, а не мандат."
        echo
        echo "  Что сделать вместо этой правки:"
        echo "   1. Спросить через AskUserQuestion, показав: старое → новое (file:line);"
        echo "      согласовано ли с каноном (со ссылкой); что тащит за собой"
        echo "      (деньги / безопасность / охват)."
        echo "   2. Найти ВСЕ места, где живёт этот дефолт (клиент + лаунчер +"
        echo "      манифест + юнит), и перечислить их в вопросе."
        echo "   3. Получив ответ — повторить: $4"
        echo
        echo "  Канон маршрутизации: bulk/coding → GLM, judgment → Opus"
        echo "  (~/.claude/CLAUDE.md). Правило: ~/.claude/rules/no-silent-default-change.md"
    } >&2
    exit 2
}

# ---------------------------------------------------------------- Bash path
if [[ "$TOOL_NAME" == "Bash" ]]; then
    CMD=$(printf '%s' "$PAYLOAD" | jq -r '.tool_input.command // ""' 2>/dev/null)
    [[ -z "$CMD" ]] && exit 0

    # Inline approval — visible in the transcript, unlike an exported var.
    printf '%s' "$CMD" | grep -qE '\bDEFAULT_CHANGE_APPROVED=1\b' && exit 0

    # Hard file writers — the tools that put bytes into a path.
    HARD='(\bsed[[:space:]]+[^|]*-[a-zA-Z]*i|\bperl[[:space:]]+[^|]*-[a-zA-Z]*i'
    HARD+='|\btee\b|\bcp[[:space:]]|\bmv[[:space:]]|\binstall[[:space:]]'
    HARD+='|\bdd[[:space:]]|\btruncate[[:space:]]|\bpatch[[:space:]]'
    HARD+='|\bex[[:space:]]+-s|\bopen\(|\.write\(|writeFileSync)'

    # Git commands that write HISTORY, not working-tree files. A commit message
    # DESCRIBING a default change is not a default change — this hook blocked
    # its own commit that way, because the message heredoc looked like a write.
    #
    # Exempt only when NO hard writer rides along, so `git add . && sed -i ...`
    # is still caught. `checkout`/`restore`/`revert`/`stash` are deliberately
    # absent: those rewrite tracked files and must stay gated.
    if printf '%s' "$CMD" | grep -qE '\bgit[[:space:]]+(commit|tag|notes)\b'; then
        printf '%s' "$CMD" | grep -qE "$HARD" || exit 0
    fi

    DECLS=$(extract "$CMD")
    # No default declared anywhere in the command -> nothing to protect.
    [[ -z "$DECLS" ]] && exit 0

    # Does this command write? Strip the redirects that go nowhere first, so
    # `2>/dev/null` and `>&1` are not mistaken for a file write.
    SANE=$(printf '%s' "$CMD" \
        | sed -E 's/[0-9]?&?>+[[:space:]]*\/dev\/null//g; s/[0-9]?>&[0-9]//g')
    WRITES='(\bsed[[:space:]]+[^|]*-[a-zA-Z]*i|\bperl[[:space:]]+[^|]*-[a-zA-Z]*i'
    WRITES+='|\btee\b|\bcp[[:space:]]|\bmv[[:space:]]|\binstall[[:space:]]'
    WRITES+='|\bdd[[:space:]]|\btruncate[[:space:]]|\bpatch[[:space:]]'
    WRITES+='|\bex[[:space:]]+-s|\bopen\(|\.write\(|writeFileSync|>)'
    printf '%s' "$SANE" | grep -qE "$WRITES" || exit 0

    # Direction is unknowable here — the command text holds both sides at once
    # (`s/old/new/`) or only the new one. So list every declaration found and
    # let the reader see what the command touches, instead of guessing which
    # one is the replacement.
    refuse "Bash — команда пишет в файл" \
        "<неизвестно: Bash не отдаёт старое значение отдельно>" \
        "${DECLS//$'\n'/ | }" \
        "DEFAULT_CHANGE_APPROVED=1 <та же команда>"
fi

# ------------------------------------------------- structured (Edit/Write/MCP)
case "$TOOL_NAME" in
    Write|Edit|mcp__kei__kei_write|mcp__kei__kei_edit) ;;
    *) exit 0 ;;
esac

FILE_PATH=$(printf '%s' "$PAYLOAD" | jq -r '.tool_input.file_path // ""' 2>/dev/null)

# Never gate our own rule/doc text — it QUOTES the defaults it describes.
case "$FILE_PATH" in
    */rules/*|*/hooks/*|*/memory/*|*CHANGELOG*|*/docs/*) exit 0 ;;
esac

NEW=$(printf '%s' "$PAYLOAD" | jq -r '.tool_input.new_string // .tool_input.content // ""' 2>/dev/null)
[[ -z "$NEW" ]] && exit 0

# Old side: Edit carries old_string; Write replaces a file, so read it off disk.
OLD=$(printf '%s' "$PAYLOAD" | jq -r '.tool_input.old_string // ""' 2>/dev/null)
if [[ -z "$OLD" && -r "$FILE_PATH" ]]; then
    OLD=$(cat "$FILE_PATH" 2>/dev/null)
fi
# A brand-new file declares defaults for the first time — nothing is being
# overridden, so there is nothing to reconcile with the user.
[[ -z "$OLD" ]] && exit 0

OLD_D=$(extract "$OLD")
NEW_D=$(extract "$NEW")

# No default declared on either side, or nothing moved -> allow.
[[ "$OLD_D" == "$NEW_D" ]] && exit 0
[[ -z "$OLD_D" && -z "$NEW_D" ]] && exit 0

# Purely ADDING a default where none stood is not an override of a user choice.
# Flag only when a previously-declared default disappears or is rewritten.
REMOVED=$(comm -23 <(printf '%s\n' "$OLD_D") <(printf '%s\n' "$NEW_D") 2>/dev/null)
[[ -z "$REMOVED" ]] && exit 0

ADDED=$(comm -13 <(printf '%s\n' "$OLD_D") <(printf '%s\n' "$NEW_D") 2>/dev/null)

refuse "${FILE_PATH:-<stdin>}" \
    "${REMOVED//$'\n'/ | }" \
    "${ADDED:-<удалено>}" \
    "правку с DEFAULT_CHANGE_APPROVED=1"
