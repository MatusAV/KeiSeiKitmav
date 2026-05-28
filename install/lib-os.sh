# shellcheck shell=bash
# lib-os.sh — OS detection + XDG paths (v0.55, audit Linux-compat fix).
#
# Was: dev-hub-* libs hardcoded macOS paths (`~/Library/Application Support/`,
# `~/Library/Logs/`) and called launchctl / brew without OS gates. Result: on
# Linux these libs crashed with "brew not found" + wrote files to a
# non-existent ~/Library/ tree (silently created the dir but no daemon
# manager picked them up).
#
# Now: ONE place defines:
#   KEI_OS                 darwin | linux | wsl | unknown
#   KEI_DATA_DIR           ~/Library/Application Support/keisei  (macOS)
#                          ~/.local/share/keisei                 (Linux/XDG)
#   KEI_LOG_DIR            ~/Library/Logs/keisei                 (macOS)
#                          ~/.local/state/keisei/logs            (Linux/XDG)
#   KEI_SVC_MANAGER        launchd | systemd | none
#
# dev-hub-* libs source this and use the paths instead of hardcoding.
# macOS-only libs (forgejo / zoekt / restic / etc) check KEI_OS and
# skip-with-warn on Linux until ported.

# Re-source guard.
[ "${_KEI_OS_SOURCED:-0}" = "1" ] && return 0
_KEI_OS_SOURCED=1

# ---- OS detection ----------------------------------------------------------
case "$(uname -s)" in
  Darwin)
    KEI_OS=darwin
    KEI_DATA_DIR="$HOME/Library/Application Support/keisei"
    KEI_LOG_DIR="$HOME/Library/Logs/keisei"
    KEI_SVC_MANAGER=launchd
    ;;
  Linux)
    if grep -qi microsoft /proc/version 2>/dev/null; then
      KEI_OS=wsl
    else
      KEI_OS=linux
    fi
    # XDG_DATA_HOME default per spec; override via env.
    KEI_DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/keisei"
    KEI_LOG_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/keisei/logs"
    # Honor user-systemd if installed (best-effort).
    if command -v systemctl >/dev/null 2>&1; then
      KEI_SVC_MANAGER=systemd
    else
      KEI_SVC_MANAGER=none
    fi
    ;;
  *)
    KEI_OS=unknown
    KEI_DATA_DIR="$HOME/.keisei"
    KEI_LOG_DIR="$HOME/.keisei/logs"
    KEI_SVC_MANAGER=none
    ;;
esac

export KEI_OS KEI_DATA_DIR KEI_LOG_DIR KEI_SVC_MANAGER

# ---- helper: kei_require_macos -------------------------------------------
# Use in lib-dev-hub-* scripts that aren't ported to Linux yet:
#
#     kei_require_macos "Forgejo dev-hub" || return 0
#
# Returns 0 if macOS, 1 (with warn) otherwise so the caller `return 0`s
# instead of erroring the whole install. Audit fix 2026-05-28 — was
# `[error] brew not found — Forgejo requires Homebrew on macOS arm64.`
# leaking through every install on Linux.
kei_require_macos() {
  local what="${1:-this dev-hub primitive}"
  if [ "$KEI_OS" = "darwin" ]; then
    return 0
  fi
  if command -v warn >/dev/null 2>&1; then
    warn "$what is macOS-only (KEI_OS=$KEI_OS). Skipping. See docs/architecture/dev-hub-linux-port.md (planned)."
  else
    printf '[warn] %s is macOS-only (KEI_OS=%s). Skipping.\n' "$what" "$KEI_OS" >&2
  fi
  return 1
}

# ---- helper: kei_require_brew --------------------------------------------
# Many dev-hub libs `command -v brew >/dev/null || die "no brew"`. Linux
# falls through with no fallback. Centralise:
kei_require_brew() {
  local what="${1:-this package}"
  if command -v brew >/dev/null 2>&1; then
    return 0
  fi
  if [ "$KEI_OS" = "darwin" ]; then
    if command -v err >/dev/null 2>&1; then
      err "$what needs Homebrew. Install: https://brew.sh/"
    else
      printf '[error] %s needs Homebrew. Install: https://brew.sh/\n' "$what" >&2
    fi
    return 1
  fi
  # Linux / WSL — skip without error (dev-hub-* libs are macOS-targeted).
  if command -v warn >/dev/null 2>&1; then
    warn "$what (Homebrew-based) — skipping on $KEI_OS"
  fi
  return 1
}
