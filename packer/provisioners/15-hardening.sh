#!/bin/bash
set -euo pipefail
LOG="/var/log/packer-provision.log"
exec > >(tee -a "$LOG") 2>&1

export DEBIAN_FRONTEND=noninteractive  # For Ubuntu

echo "[15-hardening] ====================================================="
echo "[15-hardening] Hardening instance..."
echo "[15-hardening] ====================================================="

# Ensure script is run as root
if [[ $EUID -ne 0 ]]; then
  echo "[ERROR] This script must be run as root. Use sudo." >&2
  exit 1
fi

run_step() {
  "$@" || echo "[Ubuntu] Step failed: $*"
}

echo "[Ubuntu] Starting CIS Level 1 hardening"

# CIS: Disable cramfs
run_step bash -c 'echo "install cramfs /bin/true" | sudo tee /etc/modprobe.d/cramfs.conf > /dev/null'

# CIS: SSH hardening
run_step sudo sed -i 's/^#Protocol .*/Protocol 2/' /etc/ssh/sshd_config
run_step sudo sed -i 's/^PermitRootLogin.*/PermitRootLogin no/' /etc/ssh/sshd_config

# CIS: Password aging
run_step sudo chage --maxdays 90 root
run_step sudo chage --mindays 7 root
run_step sudo chage --warndays 14 root

# CIS: File permissions
run_step sudo chmod 644 /etc/passwd
run_step sudo chmod 000 /etc/shadow

echo "[15-hardening] ====================================================="
echo "[15-hardening] Successfully completed hardening..."
echo "[15-hardening] ====================================================="
