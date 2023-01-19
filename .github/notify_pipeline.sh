#!/usr/bin/env bash
set -e
function script_echo() {
  echo "unleash-operator-rs: $1"
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
    "project": "unleash-edge",
    "trigger": {
      "type": "commit",
      "source": "Unleash/unleash-operator-rs",
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

curl -X POST -H "Content-Type: application/json" https://sandbox.getunleash.io/pipeline/build_info -d @buildinfo.json

