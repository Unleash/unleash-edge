#!/usr/bin/env bash
set -euo pipefail

echo "[00-base] ====================================================="
echo "[00-base] Starting base provisioning..."
echo "[00-base] ====================================================="

echo "[00-base] 🟩 Ubuntu detected"
echo "[00-base] Updating system packages..."
export DEBIAN_FRONTEND=noninteractive
sudo apt-get update -y
sudo apt-get upgrade -y || true
sudo apt-get install -y jq curl unzip tar git build-essential || true

# ------------------------------------------------------------------------------
# SSH configuration tuning
# ------------------------------------------------------------------------------
echo "[00-base] Applying SSH keepalive configuration..."
sudo sed -i 's/^#*ClientAliveInterval.*/ClientAliveInterval 300/' /etc/ssh/sshd_config || true
sudo sed -i 's/^#*ClientAliveCountMax.*/ClientAliveCountMax 2/' /etc/ssh/sshd_config || true
sudo systemctl reload sshd || true
echo "[00-base] SSH keepalive configured"
# ------------------------------------------------------------------------------
# Optional: Disable motd, banner, and cleanup unnecessary files
# ------------------------------------------------------------------------------
echo "[00-base] Performing system cleanup..."
sudo rm -rf /etc/motd.d/* /etc/issue.net /etc/issue 2>/dev/null || true
echo "[00-base] MOTD banner and issue files deleted"

# ------------------------------------------------------------------------------
# Cleanup package caches to reduce AMI size
# ------------------------------------------------------------------------------
echo "[00-base] Cleaning up package caches..."
sudo apt-get clean -y 2>/dev/null || true
sudo rm -rf /var/cache/apt/* /tmp/* /var/tmp/* || true
echo "[00-base] Package caches cleaned"

echo "[00-base] ====================================================="
echo "[00-base] ✅ Completed base provisioning successfully."
echo "[00-base] ====================================================="
