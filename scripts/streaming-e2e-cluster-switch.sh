#!/usr/bin/env bash

set -euo pipefail

COMPOSE_FILE="${COMPOSE_FILE:-docker-compose.streaming-e2e.yml}"
COMPOSE=(docker compose -f "$COMPOSE_FILE")

UNLEASH_URL="${UNLEASH_URL:-http://127.0.0.1:4242}"
EDGE_A_URL="${EDGE_A_URL:-http://127.0.0.1:3064}"
EDGE_B_URL="${EDGE_B_URL:-http://edge-b:3063}"
EDGE_X_URL="${EDGE_X_URL:-http://127.0.0.1:3067}"
ROUTER_URL="${ROUTER_URL:-http://127.0.0.1:3066}"
CLIENT_TOKEN="${CLIENT_TOKEN:-*:development.unleash-insecure-client-api-token}"
ADMIN_TOKEN="${ADMIN_TOKEN:-*:*.unleash-insecure-admin-api-token}"
PROJECT="${PROJECT:-default}"
ENVIRONMENT="${ENVIRONMENT:-development}"
FEATURE_NAME="${FEATURE_NAME:-streaming-e2e-toggle}"
SKEW_FEATURE_NAME="${SKEW_FEATURE_NAME:-${FEATURE_NAME}-after-start}"
WAIT_TIMEOUT_SECONDS="${WAIT_TIMEOUT_SECONDS:-120}"
POLL_INTERVAL_SECONDS="${POLL_INTERVAL_SECONDS:-2}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROUTER_CONF="${SCRIPT_DIR}/streaming-e2e-router/conf.d/default.conf"
ROUTER_EDGE_A_CONF="${SCRIPT_DIR}/streaming-e2e-router/targets/edge-a.conf"
ROUTER_EDGE_B_CONF="${SCRIPT_DIR}/streaming-e2e-router/targets/edge-b.conf"

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

probe_http_ok() {
  local url="$1"
  docker compose -f "$COMPOSE_FILE" exec -T probe curl -fsS "$url" >/dev/null
}

probe_get_features() {
  local edge_url="$1"
  docker compose -f "$COMPOSE_FILE" exec -T probe \
    curl -fsS -H "Authorization: ${CLIENT_TOKEN}" "${edge_url}/api/client/features"
}

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

  until fetch_features_json "$edge_url" | jq -e --arg name "$expected_feature" '.features | any(.name == $name)' >/dev/null; do
    if (( SECONDS >= deadline )); then
      die "Timed out waiting for ${expected_feature} to appear in ${edge_url}/api/client/features"
    fi
    sleep "$POLL_INTERVAL_SECONDS"
  done
}

wait_probe_http_ok() {
  local url="$1"
  local deadline=$((SECONDS + WAIT_TIMEOUT_SECONDS))

  until probe_http_ok "$url"; do
    if (( SECONDS >= deadline )); then
      die "Timed out waiting for ${url}"
    fi
    sleep "$POLL_INTERVAL_SECONDS"
  done
}

fetch_features_json() {
  local edge_url="$1"
  case "$edge_url" in
    http://edge-b:3063*)
      probe_get_features "$edge_url"
      ;;
    *)
      curl -fsS -H "Authorization: ${CLIENT_TOKEN}" "${edge_url}/api/client/features"
      ;;
  esac
}

activate_router_target() {
  local target="$1"
  local source_conf

  case "$target" in
    edge-a) source_conf="$ROUTER_EDGE_A_CONF" ;;
    edge-b) source_conf="$ROUTER_EDGE_B_CONF" ;;
    *) die "Unknown router target: ${target}" ;;
  esac

  cp "$source_conf" "$ROUTER_CONF"
}

switch_router_to() {
  local target="$1"
  info "Switching router upstream to ${target}"
  activate_router_target "$target"
  "${COMPOSE[@]}" exec -T router nginx -s reload >/dev/null
}

prepare_router_startup() {
  info "Preparing router to point at edge-a"
  activate_router_target edge-a
}

create_feature() {
  local feature_name="${1}"
  curl -fsS \
    -X POST \
    -H "Authorization: ${ADMIN_TOKEN}" \
    -H 'Content-Type: application/json' \
    -d "{\"name\":\"${feature_name}\",\"type\":\"release\"}" \
    "${UNLEASH_URL}/api/admin/projects/${PROJECT}/features" >/dev/null || true
}

enable_feature() {
  local feature_name="${1}"
  curl -fsS \
    -X POST \
    -H "Authorization: ${ADMIN_TOKEN}" \
    "${UNLEASH_URL}/api/admin/projects/${PROJECT}/features/${feature_name}/environments/${ENVIRONMENT}/on" >/dev/null
}

ensure_prerequisites() {
  require_tool docker
  require_tool curl
  require_tool jq
  [[ -n "${UNLEASH_LICENSE:-}" ]] || die "UNLEASH_LICENSE must be set"
}

start_unleash_base() {
  info "Starting Unleash base services"
  "${COMPOSE[@]}" up -d db unleash >/dev/null
  wait_http_ok "${UNLEASH_URL}/health"
}

seed_initial_state() {
  info "Creating and enabling initial feature ${FEATURE_NAME} before any Edge node bootstraps"
  create_feature "${FEATURE_NAME}"
  enable_feature "${FEATURE_NAME}"
}

start_primary_chain() {
  prepare_router_startup

  info "Starting edge-a after Unleash has initial delta state"
  "${COMPOSE[@]}" up -d edge-a >/dev/null
  wait_http_ok "${EDGE_A_URL}/internal-backstage/ready"

  info "Starting router after edge-a is ready"
  "${COMPOSE[@]}" up -d router >/dev/null
  wait_http_ok "${ROUTER_URL}/internal-backstage/health"

  info "Starting edge-x after router is reachable"
  "${COMPOSE[@]}" up -d edge-x >/dev/null
  wait_http_ok "${EDGE_X_URL}/internal-backstage/ready"

  wait_edge_features "${EDGE_A_URL}" "${FEATURE_NAME}"
  wait_edge_features "${EDGE_X_URL}" "${FEATURE_NAME}"
}

introduce_history_skew() {
  info "Creating skew feature ${SKEW_FEATURE_NAME} after edge-a and edge-x are already running"
  create_feature "${SKEW_FEATURE_NAME}"
  enable_feature "${SKEW_FEATURE_NAME}"

  wait_edge_features "${EDGE_A_URL}" "${SKEW_FEATURE_NAME}"
  wait_edge_features "${EDGE_X_URL}" "${SKEW_FEATURE_NAME}"
}

start_secondary_upstream() {
  info "Starting edge-b after edge-a has already ingested the post-start update"
  "${COMPOSE[@]}" up -d edge-b probe >/dev/null
  wait_probe_http_ok "${EDGE_B_URL}/internal-backstage/ready"
  wait_edge_features "${EDGE_B_URL}" "${FEATURE_NAME}"
  wait_edge_features "${EDGE_B_URL}" "${SKEW_FEATURE_NAME}"
}

switch_edge_x_upstream() {
  switch_router_to edge-b

  info "Restarting edge-x to force a fresh upstream connection through the switched router"
  "${COMPOSE[@]}" restart edge-x >/dev/null
  wait_http_ok "${EDGE_X_URL}/internal-backstage/ready"
  wait_edge_features "${EDGE_X_URL}" "${FEATURE_NAME}"
  wait_edge_features "${EDGE_X_URL}" "${SKEW_FEATURE_NAME}"
}

assert_semantic_equivalence() {
  info "Capturing semantic state from edge-a, edge-b, and edge-x"
  local edge_a_state edge_b_state edge_x_state
  edge_a_state="$(fetch_features_json "${EDGE_A_URL}" | jq -S '.features')"
  edge_b_state="$(fetch_features_json "${EDGE_B_URL}" | jq -S '.features')"
  edge_x_state="$(fetch_features_json "${EDGE_X_URL}" | jq -S '.features')"

  [[ "$edge_a_state" == "$edge_b_state" ]] || die "edge-a and edge-b do not expose the same semantic feature state"
  [[ "$edge_b_state" == "$edge_x_state" ]] || die "edge-x did not converge to the same semantic feature state after switching upstream nodes"

  info "E2E scenario passed"
}

main() {
  ensure_prerequisites
  start_unleash_base
  seed_initial_state
  start_primary_chain
  introduce_history_skew
  start_secondary_upstream
  switch_edge_x_upstream
  assert_semantic_equivalence
}

main "$@"
