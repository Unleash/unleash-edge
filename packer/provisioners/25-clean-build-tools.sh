#!/usr/bin/env bash
set -euo pipefail

echo "[25-clean-build-tools] ====================================================="
echo "[25-clean-build-tools] Uninstalling tools no longer needed..."
echo "[25-clean-build-tools] ====================================================="

source $HOME/.cargo/env
rustup self uninstall -y
sudo apt-get purge -y git build-essential
sudo apt-get autoremove -y

echo "[25-clean-build-tools] ====================================================="
echo "[25-clean-build-tools] Successfully purged rust, git and build essential..."
echo "[25-clean-build-tools] ====================================================="