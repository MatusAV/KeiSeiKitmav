#!/bin/sh
# projects-registry-sync — держит ~/PROJECTS.md в мёрж-режиме, а не перезаписью.
# Event: SessionStart (stdout инжектится в контекст). Всегда exit 0 — не роняет сессию.
#
# Что делает:
#   1) Сканирует диск: проекты в ~/ (кроме инфра) и в Documents (кроме Win-мусора).
#   2) НОВАЯ папка (её пути НЕТ в реестре) → дописывает строку-ЗАГЛУШКУ в раздел
#      «🆕 Не разобрано (авто)». Существующие строки/комментарии НЕ трогает.
#   3) ПРОПАВШИЙ путь (в реестре есть, на диске нет) → только СООБЩАЕТ в stdout,
#      файл НЕ правит — потому что ~/win/* и Documents-зеркала при рестарте WSL
#      временно исчезают, и правка затёрла бы живой проект.
#   Молчит, когда дрейфа нет (никакого шума в контексте).
#
# Матчинг — по ПУТИ (не по имени): все пути в реестре обёрнуты в `бэктики`,
# поэтому «известен» = реестр содержит "<путь>`" (с завершающим бэктиком) —
# это убивает ложные срабатывания на префиксах (~/kei vs ~/keisei).
set -e

REG="$HOME/PROJECTS.md"
[ -f "$REG" ] || exit 0            # нет реестра — нечего синхронизировать
[ -w "$REG" ] || exit 0           # не можем писать — молчим

DOCS="${PROJREG_DOCS:-/mnt/c/Users/Александр/Documents}"  # переопределяемо для тестов
BT='`'                             # литеральный бэктик отдельной переменной

# --- Win-мусор в Documents, который НЕ проект ---
is_doc_junk() {
  case "$1" in
    "Мои видеозаписи"|"Моя музыка"|"мои рисунки"|"Мои фигуры"|\
    "My Music"|"My Pictures"|"My Videos"|"desktop.ini") return 0 ;;
    *) return 1 ;;
  esac
}

# «известен» ли проект: путь совпал (терпимо к завершающему '\' у Documents\X\)
# ИЛИ есть жирная запись **имя**. $1 = путь-токен, $2 = имя (basename).
known() {
  grep -qF -- "$1$BT" "$REG" && return 0     # путь + бэктик: `~/keisei`, `Documents\surfbot`
  grep -qF -- "$1\\$BT" "$REG" && return 0    # путь + '\' + бэктик: `Documents\Bazi\`
  grep -qF -- "**$2**" "$REG" && return 0     # жирное имя: **Ваниль**, **Резюме**
  return 1
}

NEW_STUBS=""      # список путей новых папок (человекочитаемо)
appended=0

append_stub() {   # $1 = отображаемый путь, $2 = имя проекта
  # раздел-заглушка создаётся один раз
  if ! grep -qF -- "## 🆕 Не разобрано (авто)" "$REG"; then
    printf '\n## 🆕 Не разобрано (авто)\n\n' >> "$REG"
  fi
  printf -- '- **%s** (`%s`) — (новый — добавить описание)\n' "$2" "$1" >> "$REG"
  NEW_STUBS="$NEW_STUBS $1"
  appended=1
}

# --- 1. WSL ~/ (кроме инфра flutter/tmp/win) ---
for d in "$HOME"/*/; do
  [ -d "$d" ] || continue
  name=$(basename "$d")
  case "$name" in flutter|tmp|win) continue ;; esac
  known "~/$name" "$name" || append_stub "~/$name" "$name"
done

# --- 2. Documents (только если C: смонтирован) ---
if [ -d "$DOCS" ]; then
  for d in "$DOCS"/*/; do
    [ -d "$d" ] || continue
    name=$(basename "$d")
    is_doc_junk "$name" && continue
    known "Documents\\$name" "$name" || append_stub "Documents\\$name" "$name"
  done
fi

# --- 3. Пропавшие пути из реестра (только предупреждаем) ---
MISSING=""
# вытащить все `бэктик-токены`, оставить только пути-проекты
grep -oE "$BT[^$BT]+$BT" "$REG" 2>/dev/null \
  | sed "s/$BT//g" \
  | while IFS= read -r tok; do
      case "$tok" in
        *"*"*) continue ;;                         # globs (~/win/*, Documents\*)
        "~/win/"*|"~/flutter"|"~/tmp") continue ;; # инфра — не проверяем
        "~/"*)
          rp="$HOME/${tok#\~/}"   # \~ — иначе тильда в паттерне сама раскроется в $HOME
          [ -e "$rp" ] || echo "$tok"
          ;;
        "Documents\\"*)
          [ -d "$DOCS" ] || continue               # C: не смонтирован — не судим
          sub=$(printf '%s' "${tok#Documents\\}" | tr '\\' '/')
          [ -e "$DOCS/$sub" ] || echo "$tok"
          ;;
      esac
  done > "$HOME/.claude/.projreg-missing.$$" 2>/dev/null || true
if [ -s "$HOME/.claude/.projreg-missing.$$" ]; then
  MISSING=$(paste -sd',' "$HOME/.claude/.projreg-missing.$$" 2>/dev/null)
fi
rm -f "$HOME/.claude/.projreg-missing.$$" 2>/dev/null || true

# --- вывод только при дрейфе ---
if [ "$appended" = 1 ] || [ -n "$MISSING" ]; then
  printf '[projects-registry] ~/PROJECTS.md разошёлся с диском:\n'
  [ "$appended" = 1 ] && printf '  🆕 добавил заглушки (допиши описание):%s\n' "$NEW_STUBS"
  [ -n "$MISSING" ]   && printf '  ⚠ в реестре есть, на диске НЕ найдено (проверь — удалено или размонтировано?): %s\n' "$MISSING"
fi
exit 0
