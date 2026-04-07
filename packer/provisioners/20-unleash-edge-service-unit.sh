#!/usr/bin/env bash
set -euo pipefail

echo "[20-unleash-edge-service-unit] ====================================================="
echo "[20-unleash-edge-service-unit] Configuring service unit..."
echo "[20-unleash-edge-service-unit] ====================================================="

# Install a launcher that resolves EC2 metadata before starting Edge.
sudo tee /usr/local/bin/start-unleash-edge > /dev/null <<'SCRIPT'
#!/usr/bin/env bash
set -euo pipefail

token="$(curl --silent --show-error --fail --max-time 2 -X PUT http://169.254.169.254/latest/api/token \
  -H "X-aws-ec2-metadata-token-ttl-seconds: 21600" || true)"

if [[ -n "${token}" ]]; then
  instance_id="$(curl --silent --show-error --fail --max-time 2 \
    -H "X-aws-ec2-metadata-token: ${token}" \
    http://169.254.169.254/latest/meta-data/instance-id || true)"

  if [[ -n "${instance_id}" ]]; then
    export EC2_INSTANCE_ID="${instance_id}"
  fi
fi

exec /home/ubuntu/unleash-edge edge
SCRIPT

sudo chmod 755 /usr/local/bin/start-unleash-edge

# Write service unit (start after network is online, wait until env exists)
sudo tee /etc/systemd/system/unleash-edge.service > /dev/null <<'UNIT'
          [Unit]
          Description=Unleash Edge Service
          After=cloud-final.service multi-user.target

          [Service]
          Type=simple
          ExecStart=/usr/local/bin/start-unleash-edge
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
