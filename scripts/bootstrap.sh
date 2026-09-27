#!/usr/bin/env bash
# Explicit real-toolchain preparation for a fresh checkout.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
if ! command -v cargo >/dev/null; then
  echo 'Cargo is unavailable. Run: nix develop -c bash scripts/bootstrap.sh' >&2
  exit 1
fi
if ! command -v npm >/dev/null; then
  echo 'npm is unavailable. Run: nix develop -c bash scripts/bootstrap.sh' >&2
  exit 1
fi
if [[ -L Cargo.lock ]]; then
  echo 'Refusing a symlink Cargo.lock; resolve and review a local regular lock.' >&2
  exit 1
fi
if [[ ! -f Cargo.lock ]]; then
  cargo generate-lockfile
else
  cargo metadata --locked --no-deps --format-version 1 >/dev/null
fi
# Browser model tests import these packages directly on a fresh checkout.
npm ci --prefix web --ignore-scripts --no-audit --no-fund
printf '\nPrepared locked Cargo/web dependencies. Gate:\n'
printf '  cargo fmt --all -- --check && cargo test --workspace --locked\n'
printf '  node --test tests/web_model.test.mjs tests/web_workspace.test.mjs tests/web_context_flow.test.mjs tests/web_multiplayer.test.mjs tests/web_agent_parity.test.mjs tests/web_rich_markdown.test.mjs\n'
