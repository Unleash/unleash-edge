#!/usr/bin/env bash

set -euo pipefail

COMPOSE_FILE="${COMPOSE_FILE:-docker-compose.streaming-e2e.yml}"
COMPOSE=(docker compose -f "$COMPOSE_FILE")

UNLEASH_URL="${UNLEASH_URL:-http://127.0.0.1:4242}"
EDGE_A_URL="${EDGE_A_URL:-http://127.0.0.1:3064}"
EDGE_B_URL="${EDGE_B_URL:-http://127.0.0.1:3065}"
EDGE_X_URL="${EDGE_X_URL:-http://127.0.0.1:3067}"
ROUTER_URL="${ROUTER_URL:-http://127.0.0.1:3066}"

ADMIN_TOKEN="${ADMIN_TOKEN:-*:*.unleash-insecure-admin-api-token}"
ALL_PROJECTS_TOKEN="${ALL_PROJECTS_TOKEN:-*:development.unleash-insecure-client-api-token}"
DEFAULT_PROJECT_TOKEN="${DEFAULT_PROJECT_TOKEN:-default:development.unleash-default-project-token}"

DEFAULT_PROJECT="${DEFAULT_PROJECT:-default}"
SECOND_PROJECT="${SECOND_PROJECT:-streaming-e2e-second-project}"
ENVIRONMENT="${ENVIRONMENT:-development}"
DEFAULT_FEATURE_NAME="${DEFAULT_FEATURE_NAME:-streaming-e2e-default-feature}"
SECOND_PROJECT_FEATURE_NAME="${SECOND_PROJECT_FEATURE_NAME:-streaming-e2e-second-project-feature}"
EDGE_X_TOKENS="${EDGE_X_TOKENS:-${DEFAULT_PROJECT_TOKEN}}"

WAIT_TIMEOUT_SECONDS="${WAIT_TIMEOUT_SECONDS:-120}"
POLL_INTERVAL_SECONDS="${POLL_INTERVAL_SECONDS:-2}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROUTER_CONF="${SCRIPT_DIR}/streaming-e2e-router/conf.d/default.conf"
ROUTER_EDGE_A_CONF="${SCRIPT_DIR}/streaming-e2e-router/targets/edge-a.conf"
ROUTER_EDGE_B_CONF="${SCRIPT_DIR}/streaming-e2e-router/targets/edge-b.conf"

info() {
  printf '[streaming-e2e-project-scope] %s\n' "$*"
}

die() {
  printf '[streaming-e2e-project-scope] ERROR: %s\n' "$*" >&2
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

fetch_features_json() {
  local edge_url="$1"
  local token="$2"
  curl -fsS -H "Authorization: ${token}" "${edge_url}/api/client/features"
}

wait_edge_feature_visible() {
  local edge_url="$1"
  local token="$2"
  local feature_name="$3"
  local deadline=$((SECONDS + WAIT_TIMEOUT_SECONDS))

  until fetch_features_json "$edge_url" "$token" | jq -e --arg name "$feature_name" '.features | any(.name == $name)' >/dev/null; do
    if (( SECONDS >= deadline )); then
      die "Timed out waiting for ${feature_name} to appear in ${edge_url}/api/client/features"
    fi
    sleep "$POLL_INTERVAL_SECONDS"
  done
}

assert_edge_feature_hidden() {
  local edge_url="$1"
  local token="$2"
  local feature_name="$3"

  if fetch_features_json "$edge_url" "$token" | jq -e --arg name "$feature_name" '.features | any(.name == $name)' >/dev/null; then
    die "${feature_name} unexpectedly appeared in ${edge_url}/api/client/features"
  fi
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

prepare_router_startup() {
  info "Preparing router to point at edge-a"
  activate_router_target edge-a
}

switch_router_to_edge_b() {
  info "Switching router upstream to edge-b"
  activate_router_target edge-b
  "${COMPOSE[@]}" exec -T router nginx -s reload >/dev/null
}

create_project() {
  local project_id="$1"
  curl -fsS \
    -X POST \
    -H "Authorization: ${ADMIN_TOKEN}" \
    -H 'Content-Type: application/json' \
    -d "{\"id\":\"${project_id}\",\"name\":\"${project_id}\"}" \
    "${UNLEASH_URL}/api/admin/projects" >/dev/null || true
}

create_feature() {
  local project_id="$1"
  local feature_name="$2"
  curl -fsS \
    -X POST \
    -H "Authorization: ${ADMIN_TOKEN}" \
    -H 'Content-Type: application/json' \
    -d "{\"name\":\"${feature_name}\",\"type\":\"release\"}" \
    "${UNLEASH_URL}/api/admin/projects/${project_id}/features" >/dev/null || true
}

enable_feature() {
  local project_id="$1"
  local feature_name="$2"
  curl -fsS \
    -X POST \
    -H "Authorization: ${ADMIN_TOKEN}" \
    "${UNLEASH_URL}/api/admin/projects/${project_id}/features/${feature_name}/environments/${ENVIRONMENT}/on" >/dev/null
}

ensure_prerequisites() {
  require_tool docker
  require_tool curl
  require_tool jq
  [[ -n "${UNLEASH_LICENSE:-}" ]] || die "UNLEASH_LICENSE must be set"
  export EDGE_X_TOKENS
}

start_unleash_base() {
  info "Starting Unleash base services"
  "${COMPOSE[@]}" up -d db unleash >/dev/null
  wait_http_ok "${UNLEASH_URL}/health"
}

seed_projects_and_default_state() {
  info "Creating second project ${SECOND_PROJECT}"
  create_project "${SECOND_PROJECT}"

  info "Creating and enabling default-project feature ${DEFAULT_FEATURE_NAME} before any Edge node bootstraps"
  create_feature "${DEFAULT_PROJECT}" "${DEFAULT_FEATURE_NAME}"
  enable_feature "${DEFAULT_PROJECT}" "${DEFAULT_FEATURE_NAME}"
}

start_primary_chain() {
  prepare_router_startup

  info "Starting edge-a after default project state exists"
  "${COMPOSE[@]}" up -d edge-a >/dev/null
  wait_http_ok "${EDGE_A_URL}/internal-backstage/ready"

  info "Starting router after edge-a is ready"
  "${COMPOSE[@]}" up -d router >/dev/null
  wait_http_ok "${ROUTER_URL}/internal-backstage/health"

  info "Starting edge-x with a default-project token"
  "${COMPOSE[@]}" up -d edge-x >/dev/null
  wait_http_ok "${EDGE_X_URL}/internal-backstage/ready"

  wait_edge_feature_visible "${EDGE_A_URL}" "${ALL_PROJECTS_TOKEN}" "${DEFAULT_FEATURE_NAME}"
  wait_edge_feature_visible "${EDGE_X_URL}" "${DEFAULT_PROJECT_TOKEN}" "${DEFAULT_FEATURE_NAME}"
}

disconnect_edge_x_before_out_of_scope_update() {
  info "Stopping edge-x before the out-of-scope second-project update"
  "${COMPOSE[@]}" stop edge-x >/dev/null
}

introduce_second_project_update_while_edge_x_is_disconnected() {
  info "Creating and enabling second-project feature ${SECOND_PROJECT_FEATURE_NAME} while edge-x is disconnected"
  create_feature "${SECOND_PROJECT}" "${SECOND_PROJECT_FEATURE_NAME}"
  enable_feature "${SECOND_PROJECT}" "${SECOND_PROJECT_FEATURE_NAME}"

  wait_edge_feature_visible "${EDGE_A_URL}" "${ALL_PROJECTS_TOKEN}" "${SECOND_PROJECT_FEATURE_NAME}"
}

start_secondary_upstream_after_out_of_scope_update() {
  info "Starting edge-b after the second-project update"
  "${COMPOSE[@]}" up -d edge-b >/dev/null
  wait_http_ok "${EDGE_B_URL}/internal-backstage/ready"

  wait_edge_feature_visible "${EDGE_B_URL}" "${ALL_PROJECTS_TOKEN}" "${DEFAULT_FEATURE_NAME}"
  wait_edge_feature_visible "${EDGE_B_URL}" "${ALL_PROJECTS_TOKEN}" "${SECOND_PROJECT_FEATURE_NAME}"
}

reconnect_edge_x_through_edge_b() {
  switch_router_to_edge_b

  info "Starting edge-x to reconnect through edge-b"
  "${COMPOSE[@]}" start edge-x >/dev/null
  wait_http_ok "${EDGE_X_URL}/internal-backstage/ready"

  wait_edge_feature_visible "${EDGE_X_URL}" "${DEFAULT_PROJECT_TOKEN}" "${DEFAULT_FEATURE_NAME}"
  assert_edge_feature_hidden "${EDGE_X_URL}" "${DEFAULT_PROJECT_TOKEN}" "${SECOND_PROJECT_FEATURE_NAME}"
}

assert_project_scoping() {
  info "Capturing feature names from edge-a, edge-b, and edge-x"
  local edge_a_names edge_b_names edge_x_names
  edge_a_names="$(fetch_features_json "${EDGE_A_URL}" "${ALL_PROJECTS_TOKEN}" | jq -S '[.features[].name]')"
  edge_b_names="$(fetch_features_json "${EDGE_B_URL}" "${ALL_PROJECTS_TOKEN}" | jq -S '[.features[].name]')"
  edge_x_names="$(fetch_features_json "${EDGE_X_URL}" "${DEFAULT_PROJECT_TOKEN}" | jq -S '[.features[].name]')"

  [[ "${edge_a_names}" == "${edge_b_names}" ]] || die "edge-a and edge-b do not expose the same all-project feature names"
  fetch_features_json "${EDGE_X_URL}" "${DEFAULT_PROJECT_TOKEN}" | jq -e --arg expected "${DEFAULT_FEATURE_NAME}" '
    (.features | length) == 1 and
    (.features[0].name == $expected) and
    (.features[0].project == "default")
  ' >/dev/null || die "edge-x exposed an unexpected feature set for the default-project token"

  info "Project-scoped E2E scenario passed"
}

main() {
  ensure_prerequisites
  start_unleash_base
  seed_projects_and_default_state
  start_primary_chain
  disconnect_edge_x_before_out_of_scope_update
  introduce_second_project_update_while_edge_x_is_disconnected
  start_secondary_upstream_after_out_of_scope_update
  reconnect_edge_x_through_edge_b
  assert_project_scoping
}

main "$@"
