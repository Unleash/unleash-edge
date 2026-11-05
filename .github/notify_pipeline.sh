#!/usr/bin/env bash
set -e
function script_echo() {
  echo "unleash-edge: $1"
}

function generate_buildinfo() {
  output=${1}
  trigger_event=${2}
  self_git_sha=$(git rev-parse --short=7 HEAD)

  cat <<EOT > ${output}
  {
    "commits": [
      {
        "slug": "Unleash/unleash-edge",
        "id": "${self_git_sha}"
      }
    ],
    "project": "${PROJECT}",
    "trigger": {
      "type": "commit",
      "source": "Unleash/unleash-edge",
      "commitIds": ["${self_git_sha}"]
    },
    "docker": {
      "image": "${DOCKER_IMAGE}",
      "tag": "sha-${self_git_sha}"
    },
    "unixTimestamp": "$(date +%s)"
  }
EOT
}
generate_buildinfo buildinfo.json
script_echo "$(cat buildinfo.json)"

: "${PIPELINE_CF_ACCESS_CLIENT_ID:?PIPELINE_CF_ACCESS_CLIENT_ID is required}"
: "${PIPELINE_CF_ACCESS_CLIENT_SECRET:?PIPELINE_CF_ACCESS_CLIENT_SECRET is required}"

curl --fail-with-body \
  -X POST \
  -H "Content-Type: application/json" \
  -H "CF-Access-Client-Id: ${PIPELINE_CF_ACCESS_CLIENT_ID}" \
  -H "CF-Access-Client-Secret: ${PIPELINE_CF_ACCESS_CLIENT_SECRET}" \
  https://platform.getunleash.dev/pipeline/build_info \
  -d @buildinfo.json

