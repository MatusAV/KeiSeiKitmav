# Реестр правил

Что за правила действуют в установке, где лежит канон каждого и чем оно
подкреплено. Создан 2026-07-22 скиллом `/escalate-recurrence`.

Реестр отражает **фактическое** состояние, а не желаемое: если у правила нет
файла или нет хука — так и написано.

Лежит в `docs/`, а не в `rules/`, намеренно: он не правило, и в `rules/` его
подхватывал бы `kei-decompose decompose-rules`, регистрируя как rule-фрагменты.

## Правила с каноническим текстом

| Правило | Где живёт канон | Покрытие | Enforcement | Статус хука |
|---|---|---|---|---|
| RULE 0.2 — Rust First | `rules/rust-first.md` → копия в `~/.claude/rules/` | Rust по умолчанию для нового кода; 7 разрешённых исключений с обязанностью назвать номер | `hooks/rust-first.sh` (UserPromptSubmit, advisory) | зарегистрирован |
| RULE 0.24 — Truthfulness & Verifiability | `rules/truthfulness.md` → копия в `~/.claude/rules/`. В `~/.claude/CLAUDE.md` только 6-строчная отсылка | Не выдумывать; источник к каждому утверждению; `[UNVERIFIED]` / `[МНЕНИЕ]` / «Я не могу это подтвердить»; происхождение чисел; явно называть пропущенные шаги | `hooks/truthfulness-guard.sh` (UserPromptSubmit, remind, exit 0, безусловный; bypass `TRUTHFULNESS_BYPASS=1`) | зарегистрирован; смоук-тест пройден 2026-07-22 |

## Правила, у которых есть enforcement, но нет файла-правила

Установлено грепом по `~/.claude --include="*.md"` 2026-07-22: канонического
текста этих правил в `~/.claude` **нет**, они существуют только как хуки плюс
упоминания в документации кита. Строки ниже описывают enforcement, а не канон.

| Правило | Что заявляет комментарий хука | Хуки |
|---|---|---|
| RULE 0.18 — числовые утверждения | Перед числом в ответе прикладывать evidence-маркер | `chat-numeric-prewarn.sh` (UserPromptSubmit), `numeric-claims-guard.sh` (PreToolUse), `chat-numeric-postflag.sh` (Stop), `numeric-claims-record.sh` |
| RULE 0.5 — NO HALLUCINATION | Блокировать академические цитаты без `[VERIFIED: <url>]` / `[UNVERIFIED]` | `citation-verify.sh` (PreToolUse Edit\|Write) |
| RULE 0.14-Q — verify before trust | Каждое кодифицированное правило несёт секцию `## Verify`; хук смоук-тестится до регистрации | процедурное, в скилле `/escalate-recurrence` |

## Как правило попадает на машину

1. `rules/<slug>.md` — канон, под git.
2. `install/lib-rules.sh:13-24` копирует `$KIT_DIR/rules/*.md` → `~/.claude/rules/`
   через `cp -f`, предварительно снимая бэкап каталога. Удаления посторонних
   файлов в функции нет.
3. `hooks/<slug>-guard.sh` — enforcement, под git.
4. `settings-snippet.json` — регистрация хука; `install/lib-hooks.sh:53`
   идемпотентно мержит снippet в `~/.claude/settings.json` (group_by matcher +
   dedup by command).

Чего инсталлятор НЕ делает: `~/.claude/CLAUDE.md` он не трогает вовсе
(`grep -n "CLAUDE.md" install.sh` → пусто). Всё, что положено только туда,
остаётся вне git и вне переноса на другую машину.

## Порядок добавления нового правила

Через `/escalate-recurrence` либо руками, но с теми же четырьмя артефактами:
канон в `rules/`, хук в `hooks/`, запись в `settings-snippet.json`, строка в
этом реестре. Хук смоук-тестится ДО регистрации (RULE 0.14-Q), номер RULE
берётся следующий свободный:

```bash
grep -rhoE "RULE -?[0-9]+\.[0-9]+" --include="*.md" --include="*.sh" . | sort -uV
```

Добавление хука сдвигает счётчик в `README.md` — перед коммитом прогнать
`./scripts/regen-counts.sh`, иначе pre-commit отобьёт коммит с `DRIFT DETECTED`.

## Verify

- Полнота: `ls rules/*.md` — каждый файл должен иметь строку в первой таблице.
- Живость хуков: `jq -r '[.hooks[][].hooks[].command]' ~/.claude/settings.json`
  — команды из колонки Enforcement должны присутствовать.
- Синхронность копий: `for f in rules/*.md; do diff -q "$f" ~/.claude/rules/"$(basename "$f")"; done`
  → без вывода.
- Регистрация переносима: `jq -r '[.hooks.UserPromptSubmit[].hooks[].command]' settings-snippet.json`
  содержит каждый хук из колонки Enforcement.
