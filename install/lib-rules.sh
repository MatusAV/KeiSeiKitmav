set -e
# shellcheck shell=bash
# lib-rules.sh — rule doc copy loop.
#
# Rules live in $KIT_DIR/rules/*.md and are synced into $RULES_DIR/ on every
# install. They are reference docs consulted by reminder hooks — e.g.
# rust-first.sh points at ~/.claude/rules/rust-first.md.
#
# Requires: say from lib-log.sh.
# Requires: backup_dir from lib-backup.sh.
# Reads globals: $KIT_DIR, $RULES_DIR.

install_rules() {
  [ -d "$KIT_DIR/rules" ] || return 0
  say "copying rules"
  backup_dir "$RULES_DIR"
  mkdir -p "$RULES_DIR"
  local rule_file
  for rule_file in "$KIT_DIR/rules/"*.md; do
    [ -f "$rule_file" ] || continue
    cp -f "$rule_file" "$RULES_DIR/" 2>/dev/null || true
    say "  -> $(basename "$rule_file")"
  done
}
