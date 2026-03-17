#!/usr/bin/env bash

set -euo pipefail

COMPOSE_FILE="${COMPOSE_FILE:-docker-compose.streaming-e2e.yml}"
COMPOSE=(docker compose -f "$COMPOSE_FILE")

UNLEASH_URL="${UNLEASH_URL:-http://127.0.0.1:4242}"
EDGE_A_PORT="${EDGE_A_PORT:-3064}"
EDGE_B_PORT="${EDGE_B_PORT:-3065}"
EDGE_X_PORT="${EDGE_X_PORT:-3067}"
ROUTER_PORT="${ROUTER_PORT:-3066}"
EDGE_A_URL="${EDGE_A_URL:-http://127.0.0.1:${EDGE_A_PORT}}"
EDGE_B_URL="${EDGE_B_URL:-http://127.0.0.1:${EDGE_B_PORT}}"
EDGE_X_URL="${EDGE_X_URL:-http://127.0.0.1:${EDGE_X_PORT}}"
CLIENT_TOKEN="${CLIENT_TOKEN:-*:development.unleash-insecure-client-api-token}"
ADMIN_TOKEN="${ADMIN_TOKEN:-*:*.unleash-insecure-admin-api-token}"
PROJECT="${PROJECT:-default}"
ENVIRONMENT="${ENVIRONMENT:-development}"
FEATURE_NAME="${FEATURE_NAME:-streaming-e2e-toggle}"
WAIT_TIMEOUT_SECONDS="${WAIT_TIMEOUT_SECONDS:-120}"
POLL_INTERVAL_SECONDS="${POLL_INTERVAL_SECONDS:-2}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROUTER_CONF="${SCRIPT_DIR}/streaming-e2e-router/conf.d/default.conf"

info() {
  printf '[streaming-e2e] %s\n' "$*"
}

die() {
  printf '[streaming-e2e] ERROR: %s\n' "$*" >&2
  exit 1
}

require_tool() {
  command -v "$1" >/dev/null 2>&1 || die "Missing required tool: $1"
}

cleanup() {
  if [[ "${KEEP_E2E_STACK:-0}" == "1" ]]; then
    info "Leaving compose stack running because KEEP_E2E_STACK=1"
    return
  fi

  info "Stopping compose stack"
  "${COMPOSE[@]}" down -v --remove-orphans >/dev/null 2>&1 || true
}

trap cleanup EXIT

wait_http_ok() {
  local url="$1"
  local deadline=$((SECONDS + WAIT_TIMEOUT_SECONDS))

  until curl -fsS "$url" >/dev/null 2>&1; do
    if (( SECONDS >= deadline )); then
      die "Timed out waiting for ${url}"
    fi
    sleep "$POLL_INTERVAL_SECONDS"
  done
}

wait_edge_features() {
  local edge_url="$1"
  local expected_feature="$2"
  local deadline=$((SECONDS + WAIT_TIMEOUT_SECONDS))

  until curl -fsS \
      -H "Authorization: ${CLIENT_TOKEN}" \
      "${edge_url}/api/client/features" \
      | jq -e --arg name "$expected_feature" '.features | any(.name == $name)' >/dev/null; do
    if (( SECONDS >= deadline )); then
      die "Timed out waiting for ${expected_feature} to appear in ${edge_url}/api/client/features"
    fi
    sleep "$POLL_INTERVAL_SECONDS"
  done
}

write_router_target() {
  local upstream="$1"
  cat >"$ROUTER_CONF" <<EOF
server {
    listen 3063;

    location / {
        proxy_http_version 1.1;
        proxy_set_header Host \$host;
        proxy_set_header Connection "";
        proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto \$scheme;
        proxy_read_timeout 1h;
        proxy_send_timeout 1h;
        proxy_buffering off;
        proxy_pass http://${upstream}:3063;
    }
}
EOF
}

switch_router_to() {
  local upstream="$1"
  info "Switching router upstream to ${upstream}"
  write_router_target "$upstream"
  "${COMPOSE[@]}" exec -T router nginx -s reload >/dev/null
}

create_feature() {
  curl -fsS \
    -X POST \
    -H "Authorization: ${ADMIN_TOKEN}" \
    -H 'Content-Type: application/json' \
    -d "{\"name\":\"${FEATURE_NAME}\",\"type\":\"release\"}" \
    "${UNLEASH_URL}/api/admin/projects/${PROJECT}/features" >/dev/null || true
}

enable_feature() {
  curl -fsS \
    -X POST \
    -H "Authorization: ${ADMIN_TOKEN}" \
    "${UNLEASH_URL}/api/admin/projects/${PROJECT}/features/${FEATURE_NAME}/environments/${ENVIRONMENT}/on" >/dev/null
}

main() {
  require_tool docker
  require_tool curl
  require_tool jq
  [[ -n "${UNLEASH_LICENSE:-}" ]] || die "UNLEASH_LICENSE must be set"

  info "Starting Unleash base services"
  "${COMPOSE[@]}" up -d db unleash >/dev/null

  wait_http_ok "${UNLEASH_URL}/health"

  info "Creating and enabling initial feature ${FEATURE_NAME} before any Edge node bootstraps"
  create_feature
  enable_feature

  info "Starting edge-a, router, and edge-x after Unleash has initial delta state"
  "${COMPOSE[@]}" up -d edge-a router edge-x >/dev/null

  wait_http_ok "${EDGE_A_URL}/internal-backstage/ready"
  wait_http_ok "${EDGE_X_URL}/internal-backstage/ready"

  wait_edge_features "$EDGE_A_URL" "$FEATURE_NAME"
  wait_edge_features "$EDGE_X_URL" "$FEATURE_NAME"

  info "Starting edge-b after edge-a has already ingested updates"
  "${COMPOSE[@]}" up -d edge-b >/dev/null
  wait_http_ok "${EDGE_B_URL}/internal-backstage/ready"
  wait_edge_features "$EDGE_B_URL" "$FEATURE_NAME"

  switch_router_to edge-b

  info "Restarting edge-x to force a fresh upstream connection through the switched router"
  "${COMPOSE[@]}" restart edge-x >/dev/null
  wait_http_ok "${EDGE_X_URL}/internal-backstage/ready"
  wait_edge_features "$EDGE_X_URL" "$FEATURE_NAME"

  info "Capturing semantic state from edge-a, edge-b, and edge-x"
  local edge_a_state edge_b_state edge_x_state
  edge_a_state="$(curl -fsS -H "Authorization: ${CLIENT_TOKEN}" "${EDGE_A_URL}/api/client/features" | jq -S '.features')"
  edge_b_state="$(curl -fsS -H "Authorization: ${CLIENT_TOKEN}" "${EDGE_B_URL}/api/client/features" | jq -S '.features')"
  edge_x_state="$(curl -fsS -H "Authorization: ${CLIENT_TOKEN}" "${EDGE_X_URL}/api/client/features" | jq -S '.features')"

  [[ "$edge_a_state" == "$edge_b_state" ]] || die "edge-a and edge-b do not expose the same semantic feature state"
  [[ "$edge_b_state" == "$edge_x_state" ]] || die "edge-x did not converge to the same semantic feature state after switching upstream nodes"

  info "E2E scenario passed"
}

main "$@"
