#!/bin/sh
# truthfulness-guard — освежает правила правдивости перед каждым ответом.
# Severity: remind (exit 0); Event: UserPromptSubmit
# Rule: ~/.claude/rules/truthfulness.md (RULE 0.24; источник ~/keisei/rules/truthfulness.md)
# Bypass: TRUTHFULNESS_BYPASS=1

# Runtime gate (hooks-control skill / KEI_DISABLED_HOOKS / KEI_HOOK_PROFILE).
_KEI_LIB="$(dirname "$0")/_lib/gate.sh"; if [ -r "$_KEI_LIB" ]; then . "$_KEI_LIB"; kei_hook_gate "truthfulness-guard" || exit 0; fi

set -u

[ "${TRUTHFULNESS_BYPASS:-0}" = "1" ] && exit 0

# Безусловный remind — промпт не парсится, но stdin надо вычитать.
cat >/dev/null 2>&1 || true

cat <<'HOOK'
{"hookSpecificOutput":{"hookEventName":"UserPromptSubmit","additionalContext":"<truthfulness>\nПРАВИЛА ОТВЕТА (RULE 0.24, ~/.claude/rules/truthfulness.md, задано 2026-07-22):\n1. Не выдумывать факты, цитаты, числа, ссылки. Проверить нечем → «Я не могу это подтвердить».\n2. К каждому утверждению источник: код file:line | команда + её вывод | URL + дата.\n3. Непроверенное помечать [UNVERIFIED], оценочное — [МНЕНИЕ]. Иначе не подавать.\n4. Числа — показывать происхождение: формула или команда (RULE 0.18).\n5. Точность выше скорости: проверка ДО ответа, не после.\nBypass: TRUTHFULNESS_BYPASS=1\n</truthfulness>"}}
HOOK

exit 0
