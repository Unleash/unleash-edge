#!/usr/bin/env bash
set -euo pipefail

echo "[05-rust] ====================================================="
echo "[05-rust] Starting rust provisioning..."
echo "[05-rust] ====================================================="

# Install rustup only; the toolchain pinned in rust-toolchain.toml gets installed on first cargo invocation.
# Installing stable here as well would leave two full toolchains on the 8 GiB root volume.
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none
source $HOME/.cargo/env
echo "[05-rust] Successfully installed rustup"
echo "[05-rust]: $(rustup --version)"
echo "[05-rust]: Done"
