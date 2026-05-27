#!/usr/bin/env bash
# Shim: forwards to the unified Rust `kei-provision vultr` binary.
# Kept so deployed scripts/skills calling `provision-vultr.sh ...` keep working.
exec kei-provision vultr "$@"
