#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

ARTIFACT_DIR="${ARTIFACT_DIR:-$ROOT_DIR/artifacts/ci-kind-e2e}"
E2E_MANIFEST_DIR="${E2E_MANIFEST_DIR:-$ROOT_DIR/config/e2e}"
FRONTEND_FORGE_NAMESPACE="${FRONTEND_FORGE_NAMESPACE:-extension-frontend-forge}"
SAMPLE_FILE="${SAMPLE_FILE:-$ROOT_DIR/config/samples/fi-lifecycle-smoke.yaml}"
FRONTEND_INTEGRATION_CRD="${FRONTEND_INTEGRATION_CRD:-$ROOT_DIR/config/crd/bases/frontend-forge.kubesphere.io_frontendintegrations.yaml}"
CONTROLLER_RBAC_FILE="${CONTROLLER_RBAC_FILE:-$ROOT_DIR/config/rbac/frontend-forge-controller-rbac.yaml}"
RUNNER_RBAC_FILE="${RUNNER_RBAC_FILE:-$ROOT_DIR/config/rbac/frontend-forge-runner-rbac.yaml}"
KIND_CLUSTER_NAME="${KIND_CLUSTER_NAME:-frontend-forge-e2e}"

READINESS_TIMEOUT_SECONDS="${READINESS_TIMEOUT_SECONDS:-600}"
LIFECYCLE_TIMEOUT_SECONDS="${LIFECYCLE_TIMEOUT_SECONDS:-300}"
DELETE_TIMEOUT_SECONDS="${DELETE_TIMEOUT_SECONDS:-180}"
POLL_INTERVAL_SECONDS="${POLL_INTERVAL_SECONDS:-5}"

FI_NAME="fi-lifecycle-smoke"
JSBUNDLE_NAME="fi-fi-lifecycle-smoke"
CONFIGMAP_NAME="fi-fi-lifecycle-smoke-config"

usage() {
  cat <<'EOF'
Usage:
  scripts/ci-kind-e2e.sh

Environment:
  ARTIFACT_DIR                 Default: artifacts/ci-kind-e2e
  E2E_MANIFEST_DIR             Default: config/e2e
  FRONTEND_FORGE_NAMESPACE     Default: extension-frontend-forge
  KIND_CLUSTER_NAME            Default: frontend-forge-e2e
EOF
}

log() {
  printf '[ci-kind-e2e] %s\n' "$*" >&2
}

die() {
  log "$*"
  exit 1
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

assert_local_prereqs() {
  require_cmd kubectl
  require_cmd kind
  require_cmd perl
  require_cmd python3
  [[ -f "$SAMPLE_FILE" ]] || die "sample file not found: $SAMPLE_FILE"
  [[ -f "$FRONTEND_INTEGRATION_CRD" ]] || die "frontendintegration CRD not found: $FRONTEND_INTEGRATION_CRD"
  [[ -f "$CONTROLLER_RBAC_FILE" ]] || die "controller RBAC not found: $CONTROLLER_RBAC_FILE"
  [[ -f "$RUNNER_RBAC_FILE" ]] || die "runner RBAC not found: $RUNNER_RBAC_FILE"
  [[ -f "$E2E_MANIFEST_DIR/namespace.yaml" ]] || die "missing e2e manifest: namespace.yaml"
  [[ -f "$E2E_MANIFEST_DIR/jsbundle-crd.yaml" ]] || die "missing e2e manifest: jsbundle-crd.yaml"
  [[ -f "$E2E_MANIFEST_DIR/frontend-forge-controller-serviceaccount.yaml" ]] || die "missing e2e manifest: frontend-forge-controller-serviceaccount.yaml"
  [[ -f "$E2E_MANIFEST_DIR/frontend-forge-controller-deployment-ci.yaml" ]] || die "missing e2e manifest: frontend-forge-controller-deployment-ci.yaml"
  [[ -f "$E2E_MANIFEST_DIR/frontend-forge-build-service.yaml" ]] || die "missing e2e manifest: frontend-forge-build-service.yaml"
}

prepare_artifacts() {
  rm -rf "$ARTIFACT_DIR"
  mkdir -p "$ARTIFACT_DIR"
}

capture_yaml_snapshot() {
  local file_name="$1"
  shift
  "$@" > "$ARTIFACT_DIR/$file_name"
}

wait_until() {
  local timeout_seconds="$1"
  shift
  local deadline=$((SECONDS + timeout_seconds))

  while (( SECONDS < deadline )); do
    if "$@" >/dev/null 2>&1; then
      return 0
    fi
    sleep "$POLL_INTERVAL_SECONDS"
  done
  return 1
}

assert_cluster_ready() {
  kind get clusters | grep -Fx "$KIND_CLUSTER_NAME" >/dev/null || die "kind cluster not found: $KIND_CLUSTER_NAME"
  kubectl get nodes >/dev/null
}

apply_manifests() {
  log "安装 e2e 测试清单"
  kubectl apply -f "$E2E_MANIFEST_DIR/namespace.yaml" | tee "$ARTIFACT_DIR/apply-namespace.log"
  kubectl apply -f "$FRONTEND_INTEGRATION_CRD" | tee "$ARTIFACT_DIR/apply-frontendintegration-crd.log"
  kubectl apply -f "$E2E_MANIFEST_DIR/jsbundle-crd.yaml" | tee "$ARTIFACT_DIR/apply-jsbundle-crd.log"
  kubectl apply -f "$CONTROLLER_RBAC_FILE" | tee "$ARTIFACT_DIR/apply-controller-rbac.log"
  kubectl apply -f "$RUNNER_RBAC_FILE" | tee "$ARTIFACT_DIR/apply-runner-rbac.log"
  kubectl apply -f "$E2E_MANIFEST_DIR/frontend-forge-controller-serviceaccount.yaml" | tee "$ARTIFACT_DIR/apply-controller-serviceaccount.log"
  kubectl apply -f "$E2E_MANIFEST_DIR/frontend-forge-build-service.yaml" | tee "$ARTIFACT_DIR/apply-build-service.log"
  kubectl apply -f "$E2E_MANIFEST_DIR/frontend-forge-controller-deployment-ci.yaml" | tee "$ARTIFACT_DIR/apply-controller.log"
}

wait_for_frontend_forge_readiness() {
  log "等待 controller 和 frontend-forge 就绪"

  wait_until "$READINESS_TIMEOUT_SECONDS" kubectl get crd frontendintegrations.frontend-forge.kubesphere.io || return 1
  wait_until "$READINESS_TIMEOUT_SECONDS" kubectl get crd jsbundles.extensions.kubesphere.io || return 1

  kubectl rollout status deployment/frontend-forge -n "$FRONTEND_FORGE_NAMESPACE" --timeout="${READINESS_TIMEOUT_SECONDS}s" \
    | tee "$ARTIFACT_DIR/frontend-forge-readiness.log"
  kubectl rollout status deployment/frontend-forge-controller -n "$FRONTEND_FORGE_NAMESPACE" --timeout="${READINESS_TIMEOUT_SECONDS}s" \
    | tee "$ARTIFACT_DIR/frontend-forge-controller-readiness.log"
}

create_lifecycle_manifests() {
  log "生成生命周期测试样例"
  cp "$SAMPLE_FILE" "$ARTIFACT_DIR/fi-create.yaml"
  perl -0pe 's#http://example\.test/v1#http://example.test/v2#g' \
    "$ARTIFACT_DIR/fi-create.yaml" > "$ARTIFACT_DIR/fi-modify.yaml"
  perl -0pe 's/enabled: true/enabled: false/' \
    "$ARTIFACT_DIR/fi-modify.yaml" > "$ARTIFACT_DIR/fi-disable.yaml"
  cp "$ARTIFACT_DIR/fi-modify.yaml" "$ARTIFACT_DIR/fi-enable.yaml"
  cp "$ARTIFACT_DIR/fi-enable.yaml" "$ARTIFACT_DIR/fi-delete.yaml"
}

wait_for_fi_phase() {
  local phase="$1"
  wait_until "$LIFECYCLE_TIMEOUT_SECONDS" bash -lc \
    "test \"\$(kubectl get fi ${FI_NAME} -o jsonpath='{.status.phase}' 2>/dev/null)\" = \"${phase}\""
}

wait_for_fi_message() {
  local message="$1"
  wait_until "$LIFECYCLE_TIMEOUT_SECONDS" bash -lc \
    "test \"\$(kubectl get fi ${FI_NAME} -o jsonpath='{.status.message}' 2>/dev/null)\" = \"${message}\""
}

wait_for_jsbundle_state() {
  local state="$1"
  wait_until "$LIFECYCLE_TIMEOUT_SECONDS" bash -lc \
    "test \"\$(kubectl get jsbundle ${JSBUNDLE_NAME} -o jsonpath='{.status.state}' 2>/dev/null)\" = \"${state}\""
}

wait_for_jsbundle_enabled_label() {
  local expected="$1"
  wait_until "$LIFECYCLE_TIMEOUT_SECONDS" bash -lc \
    "test \"\$(kubectl get jsbundle ${JSBUNDLE_NAME} -o jsonpath='{.metadata.labels.frontend-forge\\.io/enabled}' 2>/dev/null)\" = \"${expected}\""
}

wait_for_jsbundle_source_spec_contains() {
  local needle="$1"
  wait_until "$LIFECYCLE_TIMEOUT_SECONDS" bash -lc \
    "[[ \"\$(kubectl get jsbundle ${JSBUNDLE_NAME} -o jsonpath='{.metadata.annotations.frontend-forge\\.io/source-spec}' 2>/dev/null)\" == *\"${needle}\"* ]]"
}

assert_non_empty() {
  local value="$1"
  local message="$2"
  [[ -n "$value" ]] || die "$message"
}

assert_equals() {
  local actual="$1"
  local expected="$2"
  local message="$3"
  [[ "$actual" == "$expected" ]] || die "${message}: expected=${expected} actual=${actual}"
}

assert_contains() {
  local haystack="$1"
  local needle="$2"
  local message="$3"
  grep -F "$needle" <<<"$haystack" >/dev/null 2>&1 || die "$message"
}

cleanup_previous_test_resources() {
  log "清理历史测试资源（如存在）"
  kubectl delete fi "$FI_NAME" --wait=true --ignore-not-found >/dev/null 2>&1 || true
  wait_until "$DELETE_TIMEOUT_SECONDS" bash -lc "! kubectl get fi ${FI_NAME} >/dev/null 2>&1" || true
  wait_until "$DELETE_TIMEOUT_SECONDS" bash -lc "! kubectl get jsbundle ${JSBUNDLE_NAME} >/dev/null 2>&1" || true
  wait_until "$DELETE_TIMEOUT_SECONDS" bash -lc "! kubectl -n ${FRONTEND_FORGE_NAMESPACE} get cm ${CONFIGMAP_NAME} >/dev/null 2>&1" || true
}

run_lifecycle_test() {
  local create_hash=""
  local modify_hash=""
  local job_name=""
  local source_spec=""

  log "执行 FrontendIntegration 创建测试"
  kubectl apply -f "$ARTIFACT_DIR/fi-create.yaml" > "$ARTIFACT_DIR/fi-create.apply.log"
  wait_for_fi_phase "Succeeded" || return 1
  wait_for_jsbundle_state "Available" || return 1

  create_hash="$(kubectl get fi "$FI_NAME" -o jsonpath='{.status.observed_spec_hash}')"
  assert_non_empty "$create_hash" "create step observed_spec_hash is empty"
  assert_equals \
    "$(kubectl get fi "$FI_NAME" -o jsonpath='{.status.bundle_ref.name}')" \
    "$JSBUNDLE_NAME" \
    "create step bundle_ref.name mismatch"
  wait_for_jsbundle_enabled_label "true" || return 1
  assert_equals \
    "$(kubectl get jsbundle "$JSBUNDLE_NAME" -o jsonpath='{.metadata.labels.frontend-forge\.io/enabled}')" \
    "true" \
    "create step jsbundle enabled label mismatch"
  kubectl get jsbundle "$JSBUNDLE_NAME" >/dev/null
  kubectl -n "$FRONTEND_FORGE_NAMESPACE" get cm "$CONFIGMAP_NAME" >/dev/null

  capture_yaml_snapshot "fi-create.result.yaml" kubectl get fi "$FI_NAME" -o yaml
  capture_yaml_snapshot "jsbundle-create.result.yaml" kubectl get jsbundle "$JSBUNDLE_NAME" -o yaml
  capture_yaml_snapshot "configmap-create.result.yaml" kubectl -n "$FRONTEND_FORGE_NAMESPACE" get cm "$CONFIGMAP_NAME" -o yaml

  log "执行 FrontendIntegration 修改测试"
  kubectl apply -f "$ARTIFACT_DIR/fi-modify.yaml" > "$ARTIFACT_DIR/fi-modify.apply.log"
  wait_for_fi_phase "Succeeded" || return 1
  wait_for_jsbundle_state "Available" || return 1

  modify_hash="$(kubectl get fi "$FI_NAME" -o jsonpath='{.status.observed_spec_hash}')"
  assert_non_empty "$modify_hash" "modify step observed_spec_hash is empty"
  [[ "$modify_hash" != "$create_hash" ]] || die "modify step observed_spec_hash did not change"

  wait_for_jsbundle_source_spec_contains "http://example.test/v2" || return 1
  source_spec="$(kubectl get jsbundle "$JSBUNDLE_NAME" -o jsonpath='{.metadata.annotations.frontend-forge\.io/source-spec}')"
  assert_contains "$source_spec" 'http://example.test/v2' "modify step source-spec annotation missing v2 src"
  job_name="$(kubectl get fi "$FI_NAME" -o jsonpath='{.status.last_build.job_ref.name}')"
  assert_contains "$job_name" "fi-fi-lifecycle-smoke-build-" "modify step job name prefix mismatch"

  capture_yaml_snapshot "fi-modify.result.yaml" kubectl get fi "$FI_NAME" -o yaml
  capture_yaml_snapshot "jsbundle-modify.result.yaml" kubectl get jsbundle "$JSBUNDLE_NAME" -o yaml

  log "执行 FrontendIntegration 禁用测试"
  kubectl apply -f "$ARTIFACT_DIR/fi-disable.yaml" > "$ARTIFACT_DIR/fi-disable.apply.log"
  wait_for_fi_phase "Pending" || return 1
  wait_for_fi_message "Disabled" || return 1
  wait_for_jsbundle_state "Disabled" || return 1

  wait_for_jsbundle_enabled_label "false" || return 1
  assert_equals \
    "$(kubectl get jsbundle "$JSBUNDLE_NAME" -o jsonpath='{.metadata.labels.frontend-forge\.io/enabled}')" \
    "false" \
    "disable step jsbundle enabled label mismatch"
  assert_equals \
    "$(kubectl get fi "$FI_NAME" -o jsonpath='{.status.last_build}')" \
    "" \
    "disable step last_build should be empty"

  capture_yaml_snapshot "fi-disable.result.yaml" kubectl get fi "$FI_NAME" -o yaml
  capture_yaml_snapshot "jsbundle-disable.result.yaml" kubectl get jsbundle "$JSBUNDLE_NAME" -o yaml

  log "执行 FrontendIntegration 启用测试"
  kubectl apply -f "$ARTIFACT_DIR/fi-enable.yaml" > "$ARTIFACT_DIR/fi-enable.apply.log"
  wait_for_fi_phase "Succeeded" || return 1
  wait_for_jsbundle_state "Available" || return 1

  wait_for_jsbundle_enabled_label "true" || return 1
  assert_equals \
    "$(kubectl get jsbundle "$JSBUNDLE_NAME" -o jsonpath='{.metadata.labels.frontend-forge\.io/enabled}')" \
    "true" \
    "enable step jsbundle enabled label mismatch"
  assert_equals \
    "$(kubectl get fi "$FI_NAME" -o jsonpath='{.status.bundle_ref.name}')" \
    "$JSBUNDLE_NAME" \
    "enable step bundle_ref.name mismatch"

  capture_yaml_snapshot "fi-enable.result.yaml" kubectl get fi "$FI_NAME" -o yaml
  capture_yaml_snapshot "jsbundle-enable.result.yaml" kubectl get jsbundle "$JSBUNDLE_NAME" -o yaml

  log "执行 FrontendIntegration 删除测试"
  kubectl delete -f "$ARTIFACT_DIR/fi-delete.yaml" --wait=true > "$ARTIFACT_DIR/fi-delete.apply.log"

  wait_until "$DELETE_TIMEOUT_SECONDS" bash -lc "! kubectl get fi ${FI_NAME} >/dev/null 2>&1" || return 1
  wait_until "$DELETE_TIMEOUT_SECONDS" bash -lc "! kubectl get jsbundle ${JSBUNDLE_NAME} >/dev/null 2>&1" || return 1
  wait_until "$DELETE_TIMEOUT_SECONDS" bash -lc "! kubectl -n ${FRONTEND_FORGE_NAMESPACE} get cm ${CONFIGMAP_NAME} >/dev/null 2>&1" || return 1
}

collect_failure_artifacts() {
  log "收集失败现场"
  kubectl get all -A -o wide > "$ARTIFACT_DIR/kubectl-get-all-wide.txt" 2>/dev/null || true
  kubectl get fi,jsbundle -A -o yaml > "$ARTIFACT_DIR/kubectl-get-fi-jsbundle.yaml" 2>/dev/null || true
  kubectl -n "$FRONTEND_FORGE_NAMESPACE" describe deploy frontend-forge-controller > "$ARTIFACT_DIR/describe-frontend-forge-controller.txt" 2>/dev/null || true
  kubectl -n "$FRONTEND_FORGE_NAMESPACE" logs deploy/frontend-forge-controller --all-containers=true --tail=-1 > "$ARTIFACT_DIR/frontend-forge-controller.log" 2>/dev/null || true
  kubectl -n "$FRONTEND_FORGE_NAMESPACE" logs deploy/frontend-forge --all-containers=true --tail=-1 > "$ARTIFACT_DIR/frontend-forge.log" 2>/dev/null || true
  mkdir -p "$ARTIFACT_DIR/kind-export"
  kind export logs "$ARTIFACT_DIR/kind-export" --name "$KIND_CLUSTER_NAME" >/dev/null 2>&1 || true
}

write_summary_json() {
  local status="$1"
  cat > "$ARTIFACT_DIR/summary.json" <<EOF
{
  "kind_cluster_name": "${KIND_CLUSTER_NAME}",
  "status": "${status}",
  "namespace": "${FRONTEND_FORGE_NAMESPACE}"
}
EOF
}

main() {
  local status="PASS"

  if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    usage
    exit 0
  fi

  assert_local_prereqs
  prepare_artifacts
  assert_cluster_ready
  cleanup_previous_test_resources
  apply_manifests
  create_lifecycle_manifests

  if ! wait_for_frontend_forge_readiness; then
    status="FAIL_READINESS"
  elif ! run_lifecycle_test; then
    status="FAIL_LIFECYCLE"
  fi

  if [[ "$status" != "PASS" ]]; then
    collect_failure_artifacts
  fi

  write_summary_json "$status"
  [[ "$status" == "PASS" ]] || die "ci-kind-e2e failed with status ${status}"
  log "ci-kind-e2e 通过"
}

main "$@"
