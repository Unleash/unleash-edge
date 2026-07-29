#!/usr/bin/env bash
set -euo pipefail

TAG="[21-filebeat-service-unit]"

echo "${TAG} ====================================================="
echo "${TAG} Installing and configuring Filebeat..."
echo "${TAG} ====================================================="

export DEBIAN_FRONTEND=noninteractive

# ------------------------------------------------------------------------------
# Add Elastic APT repository
# ------------------------------------------------------------------------------
echo "${TAG} Adding Elastic APT repository..."

# Download the Elastic signing key, verify its fingerprint, then dearmor it for apt.
# Fingerprint: https://www.elastic.co/docs/deploy-manage/deploy/self-managed/install-elasticsearch-with-debian-package
ELASTIC_KEY_FINGERPRINT="46095ACC8548582C1A2699A9D27D666CD88E42B4"
ELASTIC_KEY_TMP=/tmp/elastic-key.asc
ELASTIC_KEY_RING=/tmp/elastic-verify.gpg

trap 'rm -f "${ELASTIC_KEY_TMP}" "${ELASTIC_KEY_RING}"' EXIT

curl --silent --show-error --fail \
  -o "${ELASTIC_KEY_TMP}" \
  https://artifacts.elastic.co/GPG-KEY-elasticsearch

gpg --no-default-keyring --keyring "${ELASTIC_KEY_RING}" \
  --import "${ELASTIC_KEY_TMP}" 2>/dev/null

# --with-colons produces machine-readable output; fpr records hold the fingerprint in field 10.
# Format: fpr::::::::::<fingerprint>: (see https://github.com/gpg/gnupg/blob/master/doc/DETAILS)
ACTUAL_FP="$(gpg --no-default-keyring --keyring "${ELASTIC_KEY_RING}" \
  --fingerprint --with-colons \
  | awk -F: '/^fpr/{print $10; exit}')"

if [[ -z "${ACTUAL_FP}" ]]; then
  echo "${TAG} ERROR: could not extract GPG fingerprint" >&2
  exit 1
fi

if [[ "${ACTUAL_FP}" != "${ELASTIC_KEY_FINGERPRINT}" ]]; then
  echo "${TAG} ERROR: GPG fingerprint mismatch!" >&2
  echo "${TAG}   expected: ${ELASTIC_KEY_FINGERPRINT}" >&2
  echo "${TAG}   got:      ${ACTUAL_FP}" >&2
  exit 1
fi
echo "${TAG} GPG fingerprint verified: OK"

# Write binary (dearmored) keyring that apt expects for signed-by.
gpg --no-default-keyring --keyring "${ELASTIC_KEY_RING}" --export \
  > /usr/share/keyrings/elastic-keyring.gpg
chmod 644 /usr/share/keyrings/elastic-keyring.gpg

echo "deb [signed-by=/usr/share/keyrings/elastic-keyring.gpg] https://artifacts.elastic.co/packages/9.x/apt stable main" \
  > /etc/apt/sources.list.d/elastic-9.x.list

apt-get update -y
echo "${TAG} Elastic repository added."

# ------------------------------------------------------------------------------
# Install Filebeat
# ------------------------------------------------------------------------------
echo "${TAG} Installing filebeat..."
apt-get install -y filebeat
echo "${TAG} Filebeat installed."

# ------------------------------------------------------------------------------
# Write Filebeat configuration
#
# At runtime, the operator must supply two environment variables in
# /etc/filebeat.env (or via the instance's user-data / secrets manager):
#
#   ELASTIC_HOST    — e.g. https://my-cluster.es.us-east-1.aws.elastic.cloud:443
#   ELASTIC_API_KEY — id:api_key pair issued by Elasticsearch (format: "<id>:<api_key>")
#
# Filebeat natively substitutes ${VAR} references in its config file.
# ------------------------------------------------------------------------------
echo "${TAG} Writing filebeat configuration..."
cat > /etc/filebeat/filebeat.yml <<'EOF'
# Unleash Edge – Filebeat configuration
# Managed by Packer provisioner 21-filebeat-service-unit.sh – do not edit by hand.

filebeat.inputs:
  - type: journald
    id: unleash-edge
    include_matches.match:
      - "_SYSTEMD_UNIT=unleash-edge.service"

processors:
  - add_host_metadata:
      when.not.contains.tags: forwarded
  - add_fields:
      target: service
      fields:
        name: unleash-edge
  - add_fields:
      target: labels
      fields:
        aws_region: "${AWS_REGION}"
        client_id: "${CLIENT_ID}"

output.elasticsearch:
  hosts: ["${ELASTIC_HOST}"]
  api_key: "${ELASTIC_API_KEY}"
  index: "unleash-edge-logs"

# The following setup fields are used for one-time initial setup of the index
# template and ILM policy in Elasticsearch. Enable them temporarily on first
# deployment with a privileged API key, then disable again.
#setup.template.name: "unleash-edge-logs"
#setup.template.pattern: "unleash-edge-logs*"
#setup.template.enabled: true
#setup.ilm.enabled: true

logging.level: warning
logging.to_files: false
EOF

chmod 600 /etc/filebeat/filebeat.yml
echo "${TAG} Filebeat configuration written."

# ------------------------------------------------------------------------------
# Wire runtime env vars into the Filebeat systemd unit
#
# /etc/filebeat.env is created at instance launch (e.g. via cloud-init /
# user-data). If the file is absent Filebeat will fail to start, which is
# intentional – logs won't be shipped but Edge itself will keep running.
#
# Note: EnvironmentFile without the leading '-' causes systemd to treat a
# missing file as a hard error, so Filebeat will not start until the file
# exists. This matches the stated intent above.
# ------------------------------------------------------------------------------
echo "${TAG} Patching filebeat systemd unit to load /etc/filebeat.env..."
mkdir -p /etc/systemd/system/filebeat.service.d
cat > /etc/systemd/system/filebeat.service.d/env.conf <<'UNIT'
[Unit]
After=cloud-final.service

[Service]
EnvironmentFile=/etc/filebeat.env
UNIT

systemctl daemon-reload
systemctl enable filebeat
echo "${TAG} Filebeat enabled (will start on first boot once /etc/filebeat.env is present)."

echo "${TAG} ====================================================="
echo "${TAG} Filebeat provisioning complete."
echo "${TAG} At launch, provide /etc/filebeat.env with:"
echo "${TAG}   ELASTIC_HOST=https://<your-cluster>:443"
echo "${TAG}   ELASTIC_API_KEY=<id>:<api_key>"
echo "${TAG}   AWS_REGION=<region>"
echo "${TAG}   CLIENT_ID=<client-id>"
echo "${TAG} ====================================================="
