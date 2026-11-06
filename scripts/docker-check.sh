#!/usr/bin/env bash
# Runs a command in a CI-like Debian container (Rust + Tauri system libraries +
# pnpm), using cached cargo volumes. Use it to build/check the Tauri crate on
# machines without WebKitGTK:
#
#   scripts/docker-check.sh 'cargo clippy --workspace --all-targets -- -D warnings'
#   scripts/docker-check.sh 'pnpm install --frozen-lockfile && pnpm tauri build --no-bundle'
set -euo pipefail
repo="$(cd "$(dirname "$0")/.." && pwd)"
docker build -q -t notenest-ci-check - >/dev/null <<'DOCKERFILE'
FROM node:22-bookworm
RUN apt-get update && apt-get install -y --no-install-recommends \
    libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libssl-dev libxdo-dev patchelf pkg-config \
    && rm -rf /var/lib/apt/lists/*
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal -c rustfmt -c clippy
RUN npm install -g pnpm@10.34.9
ENV PATH=/root/.cargo/bin:$PATH
DOCKERFILE
docker run --rm -v "$repo":/src -w /src \
  -v nn-cargo-registry:/cargo-home -v nn-cargo-target:/cargo-target \
  -e CARGO_HOME=/cargo-home -e CARGO_TARGET_DIR=/cargo-target \
  notenest-ci-check bash -c "$1; rc=\$?; chown -R $(id -u):$(id -g) /src/Cargo.lock /src/src-tauri/gen /src/node_modules 2>/dev/null; exit \$rc"
