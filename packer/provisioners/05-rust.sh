#!/usr/bin/env bash
set -euo pipefail

echo "[05-rust] ====================================================="
echo "[05-rust] Starting rust provisioning..."
echo "[05-rust] ====================================================="

curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source $HOME/.cargo/env
echo "[05-rust] Successfully installed rust"
echo "[05-rust]: $(rustc --version)"
echo "[05-rust]: Done"
