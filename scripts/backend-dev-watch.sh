#!/usr/bin/env bash

set -e

# Ensure cargo is on PATH even when npm scripts run with a limited environment
if ! command -v cargo >/dev/null 2>&1; then
  if [ -n "${CARGO_HOME:-}" ] && [ -x "${CARGO_HOME}/bin/cargo" ]; then
    export PATH="${CARGO_HOME}/bin:${PATH}"
  elif [ -x "${HOME}/.cargo/bin/cargo" ]; then
    export PATH="${HOME}/.cargo/bin:${PATH}"
  else
    echo "cargo not found on PATH. Install Rust via rustup (https://rustup.rs/)" >&2
    exit 127
  fi
fi

export DISABLE_WORKTREE_ORPHAN_CLEANUP="${DISABLE_WORKTREE_ORPHAN_CLEANUP:-1}"
export RUST_LOG="${RUST_LOG:-debug}"

if ! cargo watch --version >/dev/null 2>&1; then
  echo "cargo-watch is not installed. Install it with 'cargo install cargo-watch'." >&2
  exit 1
fi

exec cargo watch -w crates -x 'run --bin server'
