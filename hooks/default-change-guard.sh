#!/usr/bin/env bash

# Runtime gate (hooks-control skill / KEI_DISABLED_HOOKS / KEI_HOOK_PROFILE).
_KEI_LIB="$(dirname "$0")/_lib/gate.sh"; if [ -r "$_KEI_LIB" ]; then . "$_KEI_LIB"; kei_hook_gate "default-change-guard" || exit 0; fi
# RULE 0.25 — DEFAULTS ARE NOT CHANGED SILENTLY (2026-07-22 LOCK) enforcement.
#
# Blocks an Edit/Write that changes a DEFAULT declaration — the value that
# applies when nobody chose anything: LLM provider/model, agent routing,
# default agent/profile, pinned ports, sandbox flags.
#
# Fires only when a default's VALUE actually changes old -> new. Merely
# mentioning "provider" in added prose or a new comment does not trip it.
#
# Trigger:  PreToolUse matcher = "Write|Edit|mcp__kei__kei_write|mcp__kei__kei_edit"
# Severity: enforce (exit 2 — the edit is refused, stderr goes back to Claude)
# Bypass:   export DEFAULT_CHANGE_APPROVED=1  — set AFTER the user answered an
#           AskUserQuestion, never instead of asking.
#
# Canon it protects: bulk/coding -> GLM, judgment -> Opus (~/.claude/CLAUDE.md).
# Incident of record: keiseikit-web provider glm -> claude (b4836fa, reverted c98cff0).

set -u

[[ "${DEFAULT_CHANGE_APPROVED:-0}" == "1" ]] && exit 0

PAYLOAD=$(cat)

TOOL_NAME=$(printf '%s' "$PAYLOAD" | jq -r '.tool_name // ""' 2>/dev/null)
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

# Default DECLARATIONS: key + value, in the positions where a default is set.
# Deliberately not bare `provider` — that also matches call arguments.
DECL='(--default-provider[[:space:]]+[A-Za-z0-9_.-]+'
DECL+='|default[-_]provider[[:space:]]*[:=][[:space:]]*["'"'"']?[A-Za-z0-9_.-]+'
DECL+='|DEFAULT_PROVIDER[[:space:]]*=[[:space:]]*["'"'"']?[A-Za-z0-9_.-]+'
DECL+='|provider[[:space:]]*[:=][[:space:]]*["'"'"'][A-Za-z0-9_.-]+["'"'"']'
DECL+='|primary[[:space:]]*[:=][[:space:]]*["'"'"']?[A-Za-z0-9_.-]+'
DECL+='|defaultAgent[[:space:]]*[:=][[:space:]]*["'"'"'][A-Za-z0-9_.-]+["'"'"']'
DECL+='|ANTHROPIC_MODEL[[:space:]]*=[[:space:]]*["'"'"']?[A-Za-z0-9_.:-]+'
DECL+='|\bmodel[[:space:]]*=[[:space:]]*["'"'"'][A-Za-z0-9_.:-]+["'"'"']'
DECL+='|KEI_HOOK_PROFILE[[:space:]]*=[[:space:]]*["'"'"']?[A-Za-z0-9_.-]+'
DECL+=')'

extract() {
    # Normalise whitespace so pure reindentation is not a "change".
    printf '%s' "$1" | grep -oiE "$DECL" 2>/dev/null \
        | tr -s '[:space:]' ' ' | sed 's/[[:space:]]*$//' | sort -u
}

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

{
    echo "[RULE 0.25 BLOCK] попытка сменить ДЕФОЛТ без согласования."
    echo "  file: ${FILE_PATH:-<stdin>}"
    echo "  было:  ${REMOVED//$'\n'/ | }"
    echo "  стало: ${ADDED:-<удалено>}"
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
    echo "   3. Получив ответ — повторить правку с DEFAULT_CHANGE_APPROVED=1."
    echo
    echo "  Канон маршрутизации: bulk/coding → GLM, judgment → Opus"
    echo "  (~/.claude/CLAUDE.md). Правило: ~/.claude/rules/no-silent-default-change.md"
} >&2

exit 2
