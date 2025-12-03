#!/usr/bin/env bash
set -euo pipefail

echo "[20-unleash-edge-service-unit] ====================================================="
echo "[20-unleash-edge-service-unit] Configuring service unit..."
echo "[20-unleash-edge-service-unit] ====================================================="

# Write service unit (start after network is online, wait until env exists)
sudo tee /etc/systemd/system/unleash-edge.service > /dev/null <<'UNIT'
          [Unit]
          Description=Unleash Edge Service
          After=cloud-final.service multi-user.target

          [Service]
          Type=simple
          ExecStart=/home/ubuntu/unleash-edge edge
          WorkingDirectory=/home/ubuntu
          EnvironmentFile=/etc/edge.env
          User=ubuntu
          LimitNOFILE=65536
          Restart=always

          [Install]
          WantedBy=multi-user.target
        UNIT

        sudo systemctl daemon-reload
        sudo systemctl enable unleash-edge

echo "[20-unleash-edge-service-unit]: Service unit configured successfully."
