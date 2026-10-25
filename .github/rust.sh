#!/usr/bin/env bash
# Stable Rust with the given musl target, installing rustup first where the runner image has none.
set -euo pipefail
target=$1
if ! command -v rustup >/dev/null 2>&1; then
  curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
  echo "${HOME}/.cargo/bin" >> "${GITHUB_PATH}"
  export PATH="${HOME}/.cargo/bin:${PATH}"
fi
rustup toolchain install stable --profile minimal --component rustfmt,clippy
rustup default stable
rustup target add "${target}"
rustc --version
