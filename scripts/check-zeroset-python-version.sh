#!/bin/sh
set -eu

toml_version() {
  awk '
    /^\[package\]/ { in_package = 1; next }
    /^\[/          { in_package = 0 }
    in_package && $1 == "version" { gsub(/"/, "", $3); print $3; exit }
  ' "$1"
}

rust_version=$(toml_version Cargo.toml)
python_version=$(toml_version python/Cargo.toml)

if [ -z "$rust_version" ] || [ "$python_version" != "$rust_version" ]; then
  printf 'error: Cargo.toml (%s) and python/Cargo.toml (%s) versions disagree\n' \
    "${rust_version:-missing}" "${python_version:-missing}" >&2
  exit 1
fi

if [ "$#" -gt 0 ] && [ "$1" != "zeroset-v$rust_version" ]; then
  printf 'error: tag %s does not match version %s (expected zeroset-v%s)\n' \
    "$1" "$rust_version" "$rust_version" >&2
  exit 1
fi

printf '%s\n' "$rust_version"
