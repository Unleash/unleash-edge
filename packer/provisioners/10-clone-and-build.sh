#!/usr/bin/env bash
set -euo pipefail

echo "[10-clone-and-build] ====================================================="
echo "[10-clone-and-build] Cloning and building..."
echo "[10-clone-and-build] ====================================================="

echo "[10-clone-and-build] ====================================================="
echo "[10-clone-and-build] Cloning repo at ${EDGE_VERSION:-main}"
echo "[10-clone-and-build] ====================================================="

git clone https://github.com/unleash/unleash-edge --depth 1 --branch "${EDGE_VERSION:-main}" /tmp/unleash-edge
cd /tmp/unleash-edge
source /home/ubuntu/.cargo/env
cargo build --release --features enterprise
cp target/release/unleash-edge /home/ubuntu/unleash-edge
rm -rf /tmp/unleash-edge
echo "[10-clone-and-build] Successfully built and copied"
