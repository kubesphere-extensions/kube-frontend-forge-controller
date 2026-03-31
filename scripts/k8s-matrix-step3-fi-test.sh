#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

REMOTE_SSH_TARGET="${REMOTE_SSH_TARGET:-}"
REMOTE_KIND_BIN="${REMOTE_KIND_BIN:-/root/go/bin/kind}"
REMOTE_KUBECONFIG_ROOT="${REMOTE_KUBECONFIG_ROOT:-/root/.kube/frontend-forge-matrix}"
REMOTE_WORK_ROOT="${REMOTE_WORK_ROOT:-/root/.frontend-forge-matrix}"
ARTIFACT_ROOT="${ARTIFACT_ROOT:-$ROOT_DIR/artifacts/k8s-matrix}"

FRONTEND_FORGE_NAMESPACE="${FRONTEND_FORGE_NAMESPACE:-extension-frontend-forge}"
SAMPLE_FILE="${SAMPLE_FILE:-$ROOT_DIR/config/samples/fi-lifecycle-smoke.yaml}"

READINESS_TIMEOUT_SECONDS="${READINESS_TIMEOUT_SECONDS:-1800}"
LIFECYCLE_TIMEOUT_SECONDS="${LIFECYCLE_TIMEOUT_SECONDS:-600}"
DELETE_TIMEOUT_SECONDS="${DELETE_TIMEOUT_SECONDS:-300}"
POLL_INTERVAL_SECONDS="${POLL_INTERVAL_SECONDS:-5}"
CLEANUP_ON_SUCCESS="${CLEANUP_ON_SUCCESS:-false}"

FI_NAME="fi-lifecycle-smoke"
JSBUNDLE_NAME="fi-fi-lifecycle-smoke"
CONFIGMAP_NAME="fi-fi-lifecycle-smoke-config"

CURRENT_CLUSTER=""
CURRENT_REMOTE_KUBECONFIG=""
CURRENT_REMOTE_WORK_DIR=""
CURRENT_ARTIFACT_DIR=""

usage() {
  cat <<'EOF'
Usage:
  scripts/k8s-matrix-step3-fi-test.sh <k8s-version>

Example:
  REMOTE_SSH_TARGET=root@<remote-host> scripts/k8s-matrix-step3-fi-test.sh 1.32

Environment:
  REMOTE_SSH_TARGET       Required (example: root@<remote-host>)
  REMOTE_KIND_BIN         Default: /root/go/bin/kind
  CLEANUP_ON_SUCCESS      Default: false
EOF
}

log() {
  printf '[k8s-step3] %s\n' "$*" >&2
}

die() {
  log "$*"
  exit 1
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

remote_kind_dir() {
  dirname "$REMOTE_KIND_BIN"
}

cluster_name_for_version() {
  local version="$1"
  printf 'ff-k%s' "${version//./}"
}

remote_kubeconfig_for_cluster() {
  local cluster="$1"
  printf '%s/%s.yaml' "$REMOTE_KUBECONFIG_ROOT" "$cluster"
}

remote_work_dir_for_cluster() {
  local cluster="$1"
  printf '%s/%s' "$REMOTE_WORK_ROOT" "$cluster"
}

artifact_dir_for_version() {
  local version="$1"
  printf '%s/%s' "$ARTIFACT_ROOT" "$version"
}

timestamp_to_epoch() {
  local timestamp="$1"
  python3 - "$timestamp" <<'PY'
from datetime import datetime, timezone
import sys

value = sys.argv[1]
print(int(datetime.strptime(value, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=timezone.utc).timestamp()))
PY
}

remote_capture_cmd() {
  local cmd="$1"
  ssh -o BatchMode=yes "$REMOTE_SSH_TARGET" /bin/bash -seuo pipefail <<EOF
export PATH="$(remote_kind_dir):\$PATH"
if [[ -n "${CURRENT_REMOTE_KUBECONFIG}" ]]; then
  export KUBECONFIG="${CURRENT_REMOTE_KUBECONFIG}"
fi
$cmd
EOF
}

run_remote_logged() {
  local log_file="$1"
  local script
  script="$(cat)"
  ssh -o BatchMode=yes "$REMOTE_SSH_TARGET" /bin/bash -seuo pipefail <<EOF 2>&1 | tee -a "$log_file"
export PATH="$(remote_kind_dir):\$PATH"
if [[ -n "${CURRENT_REMOTE_KUBECONFIG}" ]]; then
  export KUBECONFIG="${CURRENT_REMOTE_KUBECONFIG}"
fi
$script
EOF
}

run_remote_quiet() {
  local script
  script="$(cat)"
  ssh -o BatchMode=yes "$REMOTE_SSH_TARGET" /bin/bash -seuo pipefail <<EOF
export PATH="$(remote_kind_dir):\$PATH"
if [[ -n "${CURRENT_REMOTE_KUBECONFIG}" ]]; then
  export KUBECONFIG="${CURRENT_REMOTE_KUBECONFIG}"
fi
$script
EOF
}

assert_local_prereqs() {
  require_cmd ssh
  require_cmd scp
  require_cmd python3
  [[ -n "$REMOTE_SSH_TARGET" ]] || die "REMOTE_SSH_TARGET is required"
  [[ -f "$SAMPLE_FILE" ]] || die "sample file not found: $SAMPLE_FILE"
}

prepare_context() {
  local version="$1"
  CURRENT_CLUSTER="$(cluster_name_for_version "$version")"
  CURRENT_REMOTE_KUBECONFIG="$(remote_kubeconfig_for_cluster "$CURRENT_CLUSTER")"
  CURRENT_REMOTE_WORK_DIR="$(remote_work_dir_for_cluster "$CURRENT_CLUSTER")"
  CURRENT_ARTIFACT_DIR="$(artifact_dir_for_version "$version")"
  mkdir -p "$CURRENT_ARTIFACT_DIR"
}

assert_cluster_ready_for_test() {
  log "检查目标集群是否存在"
  run_remote_quiet <<EOF
test -f "${CURRENT_REMOTE_KUBECONFIG}"
kind get clusters | grep -Fx "${CURRENT_CLUSTER}" >/dev/null
kubectl get nodes >/dev/null
EOF
}

wait_until() {
  local timeout_seconds="$1"
  local command="$2"
  local deadline=$((SECONDS + timeout_seconds))

  while (( SECONDS < deadline )); do
    if remote_capture_cmd "$command" >/dev/null 2>&1; then
      return 0
    fi
    sleep "$POLL_INTERVAL_SECONDS"
  done
  return 1
}

wait_for_frontend_forge_readiness() {
  log "等待 frontend-forge 就绪（手动安装后）"
  run_remote_logged "${CURRENT_ARTIFACT_DIR}/step3-readiness.log" <<EOF
deadline=\$((SECONDS + ${READINESS_TIMEOUT_SECONDS}))

until kubectl get namespace "${FRONTEND_FORGE_NAMESPACE}" >/dev/null 2>&1; do
  (( SECONDS < deadline )) || exit 1
  sleep "${POLL_INTERVAL_SECONDS}"
done

until kubectl get crd frontendintegrations.frontend-forge.kubesphere.io >/dev/null 2>&1; do
  (( SECONDS < deadline )) || exit 1
  sleep "${POLL_INTERVAL_SECONDS}"
done

until kubectl get crd jsbundles.extensions.kubesphere.io >/dev/null 2>&1; do
  (( SECONDS < deadline )) || exit 1
  sleep "${POLL_INTERVAL_SECONDS}"
done

kubectl rollout status deployment/frontend-forge -n "${FRONTEND_FORGE_NAMESPACE}" --timeout="${READINESS_TIMEOUT_SECONDS}s"
kubectl rollout status deployment/frontend-forge-controller -n "${FRONTEND_FORGE_NAMESPACE}" --timeout="${READINESS_TIMEOUT_SECONDS}s"
EOF
}

create_lifecycle_manifests() {
  log "生成生命周期测试样例"
  cp "$SAMPLE_FILE" "${CURRENT_ARTIFACT_DIR}/fi-create.yaml"
  perl -0pe 's#http://example\.test/v1#http://example.test/v2#g' \
    "${CURRENT_ARTIFACT_DIR}/fi-create.yaml" > "${CURRENT_ARTIFACT_DIR}/fi-modify.yaml"
  perl -0pe 's/enabled: true/enabled: false/' \
    "${CURRENT_ARTIFACT_DIR}/fi-modify.yaml" > "${CURRENT_ARTIFACT_DIR}/fi-disable.yaml"
  cp "${CURRENT_ARTIFACT_DIR}/fi-modify.yaml" "${CURRENT_ARTIFACT_DIR}/fi-enable.yaml"
  cp "${CURRENT_ARTIFACT_DIR}/fi-enable.yaml" "${CURRENT_ARTIFACT_DIR}/fi-delete.yaml"
}

upload_lifecycle_manifests() {
  log "上传生命周期测试样例到远程"
  run_remote_quiet <<EOF
mkdir -p "${CURRENT_REMOTE_WORK_DIR}/manifests"
EOF
  scp \
    "${CURRENT_ARTIFACT_DIR}/fi-create.yaml" \
    "${CURRENT_ARTIFACT_DIR}/fi-modify.yaml" \
    "${CURRENT_ARTIFACT_DIR}/fi-disable.yaml" \
    "${CURRENT_ARTIFACT_DIR}/fi-enable.yaml" \
    "${CURRENT_ARTIFACT_DIR}/fi-delete.yaml" \
    "${REMOTE_SSH_TARGET}:${CURRENT_REMOTE_WORK_DIR}/manifests/"
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

capture_yaml_snapshot() {
  local file_name="$1"
  local command="$2"
  remote_capture_cmd "$command" > "${CURRENT_ARTIFACT_DIR}/${file_name}"
}

wait_for_fi_phase() {
  local phase="$1"
  wait_until "${LIFECYCLE_TIMEOUT_SECONDS}" \
    "test \"\$(kubectl get fi ${FI_NAME} -o jsonpath='{.status.phase}' 2>/dev/null)\" = \"${phase}\""
}

wait_for_fi_message() {
  local message="$1"
  wait_until "${LIFECYCLE_TIMEOUT_SECONDS}" \
    "test \"\$(kubectl get fi ${FI_NAME} -o jsonpath='{.status.message}' 2>/dev/null)\" = \"${message}\""
}

wait_for_jsbundle_state() {
  local state="$1"
  wait_until "${LIFECYCLE_TIMEOUT_SECONDS}" \
    "test \"\$(kubectl get jsbundle ${JSBUNDLE_NAME} -o jsonpath='{.status.state}' 2>/dev/null)\" = \"${state}\""
}

wait_for_jsbundle_enabled_label() {
  local expected="$1"
  wait_until "${LIFECYCLE_TIMEOUT_SECONDS}" \
    "test \"\$(kubectl get jsbundle ${JSBUNDLE_NAME} -o jsonpath='{.metadata.labels.frontend-forge\\.io/enabled}' 2>/dev/null)\" = \"${expected}\""
}

wait_for_jsbundle_source_spec_contains() {
  local needle="$1"
  wait_until "${LIFECYCLE_TIMEOUT_SECONDS}" \
    "[[ \"\$(kubectl get jsbundle ${JSBUNDLE_NAME} -o jsonpath='{.metadata.annotations.frontend-forge\\.io/source-spec}' 2>/dev/null)\" == *\"${needle}\"* ]]"
}

cleanup_previous_test_resources() {
  log "清理历史测试资源（如存在）"
  remote_capture_cmd "kubectl delete fi ${FI_NAME} --wait=true --ignore-not-found" >/dev/null 2>&1 || true
  wait_until "${DELETE_TIMEOUT_SECONDS}" "! kubectl get fi ${FI_NAME} >/dev/null 2>&1" || true
  wait_until "${DELETE_TIMEOUT_SECONDS}" "! kubectl get jsbundle ${JSBUNDLE_NAME} >/dev/null 2>&1" || true
  wait_until "${DELETE_TIMEOUT_SECONDS}" "! kubectl -n ${FRONTEND_FORGE_NAMESPACE} get cm ${CONFIGMAP_NAME} >/dev/null 2>&1" || true
}

run_lifecycle_test() {
  local create_hash=""
  local modify_hash=""
  local job_name=""
  local source_spec=""

  log "执行 FrontendIntegration 创建测试"
  remote_capture_cmd "kubectl apply -f '${CURRENT_REMOTE_WORK_DIR}/manifests/fi-create.yaml'" \
    > "${CURRENT_ARTIFACT_DIR}/fi-create.apply.log"
  wait_for_fi_phase "Succeeded" || return 1
  wait_for_jsbundle_state "Available" || return 1

  create_hash="$(remote_capture_cmd "kubectl get fi ${FI_NAME} -o jsonpath='{.status.observed_spec_hash}'")"
  assert_non_empty "$create_hash" "create step observed_spec_hash is empty"
  assert_equals \
    "$(remote_capture_cmd "kubectl get fi ${FI_NAME} -o jsonpath='{.status.bundle_ref.name}'")" \
    "${JSBUNDLE_NAME}" \
    "create step bundle_ref.name mismatch"
  wait_for_jsbundle_enabled_label "true" || return 1
  assert_equals \
    "$(remote_capture_cmd "kubectl get jsbundle ${JSBUNDLE_NAME} -o jsonpath='{.metadata.labels.frontend-forge\\.io/enabled}'")" \
    "true" \
    "create step jsbundle enabled label mismatch"
  remote_capture_cmd "kubectl get jsbundle ${JSBUNDLE_NAME}" >/dev/null
  remote_capture_cmd "kubectl -n ${FRONTEND_FORGE_NAMESPACE} get cm ${CONFIGMAP_NAME}" >/dev/null

  capture_yaml_snapshot "fi-create.result.yaml" "kubectl get fi ${FI_NAME} -o yaml"
  capture_yaml_snapshot "jsbundle-create.result.yaml" "kubectl get jsbundle ${JSBUNDLE_NAME} -o yaml"
  capture_yaml_snapshot "configmap-create.result.yaml" "kubectl -n ${FRONTEND_FORGE_NAMESPACE} get cm ${CONFIGMAP_NAME} -o yaml"

  log "执行 FrontendIntegration 修改测试"
  remote_capture_cmd "kubectl apply -f '${CURRENT_REMOTE_WORK_DIR}/manifests/fi-modify.yaml'" \
    > "${CURRENT_ARTIFACT_DIR}/fi-modify.apply.log"
  wait_for_fi_phase "Succeeded" || return 1
  wait_for_jsbundle_state "Available" || return 1

  modify_hash="$(remote_capture_cmd "kubectl get fi ${FI_NAME} -o jsonpath='{.status.observed_spec_hash}'")"
  assert_non_empty "$modify_hash" "modify step observed_spec_hash is empty"
  [[ "$modify_hash" != "$create_hash" ]] || die "modify step observed_spec_hash did not change"

  wait_for_jsbundle_source_spec_contains "http://example.test/v2" || return 1
  source_spec="$(remote_capture_cmd "kubectl get jsbundle ${JSBUNDLE_NAME} -o jsonpath='{.metadata.annotations.frontend-forge\\.io/source-spec}'")"
  assert_contains "$source_spec" 'http://example.test/v2' "modify step source-spec annotation missing v2 src"
  job_name="$(remote_capture_cmd "kubectl get fi ${FI_NAME} -o jsonpath='{.status.last_build.job_ref.name}'")"
  assert_contains "$job_name" "fi-fi-lifecycle-smoke-build-" "modify step job name prefix mismatch"

  capture_yaml_snapshot "fi-modify.result.yaml" "kubectl get fi ${FI_NAME} -o yaml"
  capture_yaml_snapshot "jsbundle-modify.result.yaml" "kubectl get jsbundle ${JSBUNDLE_NAME} -o yaml"

  log "执行 FrontendIntegration 禁用测试"
  remote_capture_cmd "kubectl apply -f '${CURRENT_REMOTE_WORK_DIR}/manifests/fi-disable.yaml'" \
    > "${CURRENT_ARTIFACT_DIR}/fi-disable.apply.log"
  wait_for_fi_phase "Pending" || return 1
  wait_for_fi_message "Disabled" || return 1
  wait_for_jsbundle_state "Disabled" || return 1

  wait_for_jsbundle_enabled_label "false" || return 1
  assert_equals \
    "$(remote_capture_cmd "kubectl get jsbundle ${JSBUNDLE_NAME} -o jsonpath='{.metadata.labels.frontend-forge\\.io/enabled}'")" \
    "false" \
    "disable step jsbundle enabled label mismatch"
  assert_equals \
    "$(remote_capture_cmd "kubectl get fi ${FI_NAME} -o jsonpath='{.status.last_build}'")" \
    "" \
    "disable step last_build should be empty"

  capture_yaml_snapshot "fi-disable.result.yaml" "kubectl get fi ${FI_NAME} -o yaml"
  capture_yaml_snapshot "jsbundle-disable.result.yaml" "kubectl get jsbundle ${JSBUNDLE_NAME} -o yaml"

  log "执行 FrontendIntegration 启用测试"
  remote_capture_cmd "kubectl apply -f '${CURRENT_REMOTE_WORK_DIR}/manifests/fi-enable.yaml'" \
    > "${CURRENT_ARTIFACT_DIR}/fi-enable.apply.log"
  wait_for_fi_phase "Succeeded" || return 1
  wait_for_jsbundle_state "Available" || return 1

  wait_for_jsbundle_enabled_label "true" || return 1
  assert_equals \
    "$(remote_capture_cmd "kubectl get jsbundle ${JSBUNDLE_NAME} -o jsonpath='{.metadata.labels.frontend-forge\\.io/enabled}'")" \
    "true" \
    "enable step jsbundle enabled label mismatch"
  assert_equals \
    "$(remote_capture_cmd "kubectl get fi ${FI_NAME} -o jsonpath='{.status.bundle_ref.name}'")" \
    "${JSBUNDLE_NAME}" \
    "enable step bundle_ref.name mismatch"

  capture_yaml_snapshot "fi-enable.result.yaml" "kubectl get fi ${FI_NAME} -o yaml"
  capture_yaml_snapshot "jsbundle-enable.result.yaml" "kubectl get jsbundle ${JSBUNDLE_NAME} -o yaml"

  log "执行 FrontendIntegration 删除测试"
  remote_capture_cmd "kubectl delete -f '${CURRENT_REMOTE_WORK_DIR}/manifests/fi-delete.yaml' --wait=true" \
    > "${CURRENT_ARTIFACT_DIR}/fi-delete.apply.log"

  wait_until "${DELETE_TIMEOUT_SECONDS}" "! kubectl get fi ${FI_NAME} >/dev/null 2>&1" || return 1
  wait_until "${DELETE_TIMEOUT_SECONDS}" "! kubectl get jsbundle ${JSBUNDLE_NAME} >/dev/null 2>&1" || return 1
  wait_until "${DELETE_TIMEOUT_SECONDS}" "! kubectl -n ${FRONTEND_FORGE_NAMESPACE} get cm ${CONFIGMAP_NAME} >/dev/null 2>&1" || return 1
}

collect_failure_artifacts() {
  log "收集失败现场"
  remote_capture_cmd "kubectl get all -A -o wide" > "${CURRENT_ARTIFACT_DIR}/kubectl-get-all-wide.txt" 2>/dev/null || true
  remote_capture_cmd "kubectl get fi,jsbundle -A -o yaml" > "${CURRENT_ARTIFACT_DIR}/kubectl-get-fi-jsbundle.yaml" 2>/dev/null || true
  remote_capture_cmd "kubectl -n ${FRONTEND_FORGE_NAMESPACE} describe deploy frontend-forge-controller" > "${CURRENT_ARTIFACT_DIR}/describe-frontend-forge-controller.txt" 2>/dev/null || true
  remote_capture_cmd "kubectl -n ${FRONTEND_FORGE_NAMESPACE} logs deploy/frontend-forge-controller --all-containers=true --tail=-1" > "${CURRENT_ARTIFACT_DIR}/frontend-forge-controller.log" 2>/dev/null || true
  remote_capture_cmd "kubectl -n ${FRONTEND_FORGE_NAMESPACE} logs deploy/frontend-forge --all-containers=true --tail=-1" > "${CURRENT_ARTIFACT_DIR}/frontend-forge.log" 2>/dev/null || true

  run_remote_quiet <<EOF
rm -rf "${CURRENT_REMOTE_WORK_DIR}/kind-export"
mkdir -p "${CURRENT_REMOTE_WORK_DIR}"
kind export logs "${CURRENT_REMOTE_WORK_DIR}/kind-export" --name "${CURRENT_CLUSTER}" >/dev/null 2>&1 || true
EOF

  mkdir -p "${CURRENT_ARTIFACT_DIR}/kind-export"
  ssh -o BatchMode=yes "$REMOTE_SSH_TARGET" "test -d '${CURRENT_REMOTE_WORK_DIR}/kind-export' && tar -C '${CURRENT_REMOTE_WORK_DIR}' -cf - kind-export" \
    | tar -C "${CURRENT_ARTIFACT_DIR}" -xf - 2>/dev/null || true
}

cleanup_success_cluster() {
  log "清理成功版本 cluster: ${CURRENT_CLUSTER}"
  run_remote_quiet <<EOF
kind delete cluster --name "${CURRENT_CLUSTER}" >/dev/null 2>&1 || true
rm -f "${CURRENT_REMOTE_KUBECONFIG}"
rm -rf "${CURRENT_REMOTE_WORK_DIR}"
EOF
}

write_summary_json() {
  local version="$1"
  local status="$2"
  local started_at="$3"
  local finished_at="$4"
  local duration_seconds="$5"

  cat > "${CURRENT_ARTIFACT_DIR}/step3-summary.json" <<EOF
{
  "version": "${version}",
  "cluster": "${CURRENT_CLUSTER}",
  "remote_host": "${REMOTE_SSH_TARGET}",
  "status": "${status}",
  "remote_kubeconfig": "${CURRENT_REMOTE_KUBECONFIG}",
  "started_at": "${started_at}",
  "finished_at": "${finished_at}",
  "duration_seconds": ${duration_seconds}
}
EOF
}

main() {
  local version status started_at finished_at duration_seconds

  if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    usage
    exit 0
  fi

  [[ $# -eq 1 ]] || die "expect exactly one version; see --help"
  version="$1"

  assert_local_prereqs
  prepare_context "$version"
  status="PASS"
  started_at="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"

  assert_cluster_ready_for_test || status="FAIL_PRECHECK"

  if [[ "$status" == "PASS" ]]; then
    cleanup_previous_test_resources
    create_lifecycle_manifests
    upload_lifecycle_manifests
    if ! wait_for_frontend_forge_readiness; then
      status="FAIL_READINESS"
    elif ! run_lifecycle_test; then
      status="FAIL_LIFECYCLE"
    fi
  fi

  if [[ "$status" != "PASS" ]]; then
    collect_failure_artifacts
  elif [[ "$CLEANUP_ON_SUCCESS" == "true" ]]; then
    cleanup_success_cluster
  fi

  finished_at="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
  duration_seconds="$(( $(timestamp_to_epoch "$finished_at") - $(timestamp_to_epoch "$started_at") ))"
  write_summary_json "$version" "$status" "$started_at" "$finished_at" "$duration_seconds"

  [[ "$status" == "PASS" ]] || die "step3 failed with status ${status}; cluster kept: ${CURRENT_CLUSTER}"
  log "Step3 通过"
}

main "$@"
