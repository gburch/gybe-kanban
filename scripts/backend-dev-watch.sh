#!/usr/bin/env bash

set -e

# Ensure cargo is on PATH even when npm scripts run with a limited environment
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

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

if [ -z "${BACKEND_PORT:-}" ]; then
  if ! command -v node >/dev/null 2>&1; then
    echo "node is required to determine BACKEND_PORT" >&2
    exit 1
  fi

  BACKEND_PORT=$(node "$SCRIPT_DIR/setup-dev-environment.js" backend | tr -d '"\n\r ')
  if [ -z "$BACKEND_PORT" ]; then
    echo "Failed to determine BACKEND_PORT" >&2
    exit 1
  fi
  export BACKEND_PORT
fi

export PORT="${PORT:-$BACKEND_PORT}"
if [ -z "${HOST+x}" ]; then
  export HOST="127.0.0.1"
fi

exec cargo watch -w crates -x 'run --bin server'
