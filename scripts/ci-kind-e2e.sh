#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

ARTIFACT_DIR="${ARTIFACT_DIR:-$ROOT_DIR/artifacts/ci-kind-e2e}"
CHART_DIR="${CHART_DIR:-$ROOT_DIR/config/charts/frontend-forge}"
HELM_RELEASE="${HELM_RELEASE:-frontend-forge}"
FRONTEND_FORGE_NAMESPACE="${FRONTEND_FORGE_NAMESPACE:-extension-frontend-forge}"
SAMPLE_FILE="${SAMPLE_FILE:-$ROOT_DIR/config/samples/fi-lifecycle-smoke.yaml}"
KIND_CLUSTER_NAME="${KIND_CLUSTER_NAME:-frontend-forge-e2e}"
FRONTEND_FORGE_CONTROLLER_IMAGE="${FRONTEND_FORGE_CONTROLLER_IMAGE:-}"
FRONTEND_FORGE_RUNNER_IMAGE="${FRONTEND_FORGE_RUNNER_IMAGE:-}"
FRONTEND_FORGE_IMAGE="${FRONTEND_FORGE_IMAGE:-}"
RUN_FE_E2E="${RUN_FE_E2E:-false}"
FRONTEND_EXTENSION_CONTROLLER_IMAGE="${FRONTEND_EXTENSION_CONTROLLER_IMAGE:-}"
FRONTEND_EXTENSION_PACKAGER_IMAGE="${FRONTEND_EXTENSION_PACKAGER_IMAGE:-}"
FRONTEND_EXTENSION_API_IMAGE="${FRONTEND_EXTENSION_API_IMAGE:-}"
FRONTEND_EXTENSION_PUBLISHER_IMAGE="${FRONTEND_EXTENSION_PUBLISHER_IMAGE:-${FRONTEND_EXTENSION_PACKAGER_IMAGE:-}}"
EXTENSION_API_LOCAL_PORT="${EXTENSION_API_LOCAL_PORT:-18080}"
FE_FULL_REGRESSION_SCRIPT="${FE_FULL_REGRESSION_SCRIPT:-$ROOT_DIR/scripts/fe-full-regression.sh}"

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
  CHART_DIR                    Default: config/charts/frontend-forge
  HELM_RELEASE                 Default: frontend-forge
  FRONTEND_FORGE_NAMESPACE     Default: extension-frontend-forge
  KIND_CLUSTER_NAME            Default: frontend-forge-e2e
  RUN_FE_E2E                   Default: false. Set true to run FrontendExtension packaging/download e2e.
  FRONTEND_EXTENSION_CONTROLLER_IMAGE
                               Required when RUN_FE_E2E=true
  FRONTEND_EXTENSION_PACKAGER_IMAGE
                               Required when RUN_FE_E2E=true
  FRONTEND_EXTENSION_API_IMAGE Required when RUN_FE_E2E=true
  FRONTEND_EXTENSION_PUBLISHER_IMAGE
                               Optional when RUN_FE_E2E=true. Defaults to FRONTEND_EXTENSION_PACKAGER_IMAGE.
  EXTENSION_API_LOCAL_PORT     Default: 18080
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

is_true() {
  [[ "$1" == "true" || "$1" == "1" || "$1" == "yes" ]]
}

assert_local_prereqs() {
  require_cmd kubectl
  require_cmd helm
  require_cmd kind
  require_cmd perl
  [[ -f "$SAMPLE_FILE" ]] || die "sample file not found: $SAMPLE_FILE"
  [[ -f "$CHART_DIR/Chart.yaml" ]] || die "chart not found: $CHART_DIR"
  [[ -n "$FRONTEND_FORGE_CONTROLLER_IMAGE" ]] || die "FRONTEND_FORGE_CONTROLLER_IMAGE is required"
  [[ -n "$FRONTEND_FORGE_RUNNER_IMAGE" ]] || die "FRONTEND_FORGE_RUNNER_IMAGE is required"
  [[ -n "$FRONTEND_FORGE_IMAGE" ]] || die "FRONTEND_FORGE_IMAGE is required"

  if is_true "$RUN_FE_E2E"; then
    require_cmd curl
    require_cmd shasum
    [[ -x "$FE_FULL_REGRESSION_SCRIPT" ]] || die "FE_FULL_REGRESSION_SCRIPT is not executable: $FE_FULL_REGRESSION_SCRIPT"
    [[ -n "$FRONTEND_EXTENSION_CONTROLLER_IMAGE" ]] || die "FRONTEND_EXTENSION_CONTROLLER_IMAGE is required when RUN_FE_E2E=true"
    [[ -n "$FRONTEND_EXTENSION_PACKAGER_IMAGE" ]] || die "FRONTEND_EXTENSION_PACKAGER_IMAGE is required when RUN_FE_E2E=true"
    [[ -n "$FRONTEND_EXTENSION_API_IMAGE" ]] || die "FRONTEND_EXTENSION_API_IMAGE is required when RUN_FE_E2E=true"
  fi
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

install_chart() {
  log "通过 Helm 安装 e2e 测试 Chart"
  local values_file="$ARTIFACT_DIR/frontend-forge-e2e-values.yaml"

  cat > "$values_file" <<EOF
crds:
  installJsBundle: true

image:
  registry: ""
  repository: ${FRONTEND_FORGE_CONTROLLER_IMAGE}
  tag: ""

runner:
  image:
    registry: ""
    repository: ${FRONTEND_FORGE_RUNNER_IMAGE}
    tag: ""

buildService:
  enabled: true
  image:
    registry: ""
    repository: ${FRONTEND_FORGE_IMAGE}
    tag: ""
EOF

  if is_true "$RUN_FE_E2E"; then
    cat >> "$values_file" <<EOF

extensionController:
  enabled: true
  image:
    registry: ""
    repository: ${FRONTEND_EXTENSION_CONTROLLER_IMAGE}
    tag: ""
  packagerImage: ${FRONTEND_EXTENSION_PACKAGER_IMAGE}
  publisherImage: ${FRONTEND_EXTENSION_PUBLISHER_IMAGE}

extensionPackager:
  image:
    registry: ""
    repository: ${FRONTEND_EXTENSION_PACKAGER_IMAGE}
    tag: ""

extensionPublisher:
  image:
    registry: ""
    repository: ${FRONTEND_EXTENSION_PUBLISHER_IMAGE}
    tag: ""

extensionApi:
  enabled: true
  image:
    registry: ""
    repository: ${FRONTEND_EXTENSION_API_IMAGE}
    tag: ""
EOF
  else
    cat >> "$values_file" <<EOF

extensionController:
  enabled: false

extensionApi:
  enabled: false
EOF
  fi

  helm upgrade --install "$HELM_RELEASE" "$CHART_DIR" \
    --namespace "$FRONTEND_FORGE_NAMESPACE" \
    --create-namespace \
    --values "$values_file" \
    | tee "$ARTIFACT_DIR/helm-install.log"
}

wait_for_frontend_forge_readiness() {
  log "等待 controller 和 frontend-forge 就绪"

  wait_until "$READINESS_TIMEOUT_SECONDS" kubectl get crd frontendintegrations.frontend-forge.kubesphere.io || return 1
  wait_until "$READINESS_TIMEOUT_SECONDS" kubectl get crd jsbundles.extensions.kubesphere.io || return 1
  if is_true "$RUN_FE_E2E"; then
    wait_until "$READINESS_TIMEOUT_SECONDS" kubectl get crd frontendextensions.frontend-forge.kubesphere.io || return 1
  fi

  kubectl rollout status deployment/frontend-forge -n "$FRONTEND_FORGE_NAMESPACE" --timeout="${READINESS_TIMEOUT_SECONDS}s" \
    | tee "$ARTIFACT_DIR/frontend-forge-readiness.log"
  kubectl rollout status deployment/frontend-forge-controller -n "$FRONTEND_FORGE_NAMESPACE" --timeout="${READINESS_TIMEOUT_SECONDS}s" \
    | tee "$ARTIFACT_DIR/frontend-forge-controller-readiness.log"

  if is_true "$RUN_FE_E2E"; then
    kubectl rollout status deployment/frontend-forge-extension-controller -n "$FRONTEND_FORGE_NAMESPACE" --timeout="${READINESS_TIMEOUT_SECONDS}s" \
      | tee "$ARTIFACT_DIR/frontend-extension-controller-readiness.log"
    kubectl rollout status deployment/frontend-forge-extension-api -n "$FRONTEND_FORGE_NAMESPACE" --timeout="${READINESS_TIMEOUT_SECONDS}s" \
      | tee "$ARTIFACT_DIR/frontend-forge-extension-api-readiness.log"
  fi
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

run_fe_e2e_test() {
  log "执行 FrontendExtension 打包/下载回归测试"

  local service_name=""
  local port_forward_pid=""
  local port_forward_log="$ARTIFACT_DIR/extension-api-port-forward.log"
  local download_api_base_url="http://127.0.0.1:${EXTENSION_API_LOCAL_PORT}/apis/frontend-forge.kubesphere.io/v1alpha1/frontendextensions"
  local kubeconfig_path="${KUBECONFIG:-$HOME/.kube/config}"

  service_name="$(kubectl -n "$FRONTEND_FORGE_NAMESPACE" get svc \
    -l "app.kubernetes.io/instance=${HELM_RELEASE},app.kubernetes.io/component=extension-api" \
    -o jsonpath='{.items[0].metadata.name}' 2>/dev/null || true)"
  assert_non_empty "$service_name" "extension API service not found"

  kubectl -n "$FRONTEND_FORGE_NAMESPACE" port-forward "svc/$service_name" \
    --address 127.0.0.1 "${EXTENSION_API_LOCAL_PORT}:80" > "$port_forward_log" 2>&1 &
  port_forward_pid="$!"

  cleanup_extension_api_port_forward() {
    trap - RETURN
    if [[ -n "$port_forward_pid" ]] && kill -0 "$port_forward_pid" >/dev/null 2>&1; then
      kill "$port_forward_pid" >/dev/null 2>&1 || true
      wait "$port_forward_pid" >/dev/null 2>&1 || true
    fi
  }
  trap cleanup_extension_api_port_forward RETURN

  wait_until "$READINESS_TIMEOUT_SECONDS" curl -fsS "$download_api_base_url" || return 1

  KUBECONFIG_PATH="$kubeconfig_path" \
  FRONTEND_FORGE_NAMESPACE="$FRONTEND_FORGE_NAMESPACE" \
  ARTIFACT_DIR="$ARTIFACT_DIR/fe-full-regression" \
  DOWNLOAD_API_BASE_URL="$download_api_base_url" \
  EXPECTED_PACKAGER_IMAGE="$FRONTEND_EXTENSION_PACKAGER_IMAGE" \
  "$FE_FULL_REGRESSION_SCRIPT" | tee "$ARTIFACT_DIR/fe-full-regression.log"
}

collect_failure_artifacts() {
  log "收集失败现场"
  kubectl get all -A -o wide > "$ARTIFACT_DIR/kubectl-get-all-wide.txt" 2>/dev/null || true
  kubectl get fi,frontendextension,jsbundle -A -o yaml > "$ARTIFACT_DIR/kubectl-get-fi-fe-jsbundle.yaml" 2>/dev/null || true
  kubectl -n "$FRONTEND_FORGE_NAMESPACE" describe deploy frontend-forge-controller > "$ARTIFACT_DIR/describe-frontend-forge-controller.txt" 2>/dev/null || true
  kubectl -n "$FRONTEND_FORGE_NAMESPACE" describe deploy frontend-forge-extension-controller > "$ARTIFACT_DIR/describe-frontend-extension-controller.txt" 2>/dev/null || true
  kubectl -n "$FRONTEND_FORGE_NAMESPACE" describe deploy frontend-forge-extension-api > "$ARTIFACT_DIR/describe-frontend-forge-extension-api.txt" 2>/dev/null || true
  kubectl -n "$FRONTEND_FORGE_NAMESPACE" logs deploy/frontend-forge-controller --all-containers=true --tail=-1 > "$ARTIFACT_DIR/frontend-forge-controller.log" 2>/dev/null || true
  kubectl -n "$FRONTEND_FORGE_NAMESPACE" logs deploy/frontend-forge-extension-controller --all-containers=true --tail=-1 > "$ARTIFACT_DIR/frontend-extension-controller.log" 2>/dev/null || true
  kubectl -n "$FRONTEND_FORGE_NAMESPACE" logs deploy/frontend-forge-extension-api --all-containers=true --tail=-1 > "$ARTIFACT_DIR/frontend-forge-extension-api.log" 2>/dev/null || true
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
  "namespace": "${FRONTEND_FORGE_NAMESPACE}",
  "run_fe_e2e": "${RUN_FE_E2E}"
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
  install_chart
  create_lifecycle_manifests

  if ! wait_for_frontend_forge_readiness; then
    status="FAIL_READINESS"
  elif ! run_lifecycle_test; then
    status="FAIL_LIFECYCLE"
  elif is_true "$RUN_FE_E2E" && ! run_fe_e2e_test; then
    status="FAIL_FE_E2E"
  fi

  if [[ "$status" != "PASS" ]]; then
    collect_failure_artifacts
  fi

  write_summary_json "$status"
  [[ "$status" == "PASS" ]] || die "ci-kind-e2e failed with status ${status}"
  log "ci-kind-e2e 通过"
}

main "$@"
