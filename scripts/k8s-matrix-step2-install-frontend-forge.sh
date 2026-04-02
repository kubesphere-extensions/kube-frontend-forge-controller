#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

REMOTE_SSH_TARGET="${REMOTE_SSH_TARGET:-}"
REMOTE_KIND_BIN="${REMOTE_KIND_BIN:-/root/go/bin/kind}"
REMOTE_KUBECONFIG_ROOT="${REMOTE_KUBECONFIG_ROOT:-/root/.kube/frontend-forge-matrix}"
REMOTE_WORK_ROOT="${REMOTE_WORK_ROOT:-/root/.frontend-forge-matrix}"
ARTIFACT_ROOT="${ARTIFACT_ROOT:-$ROOT_DIR/artifacts/k8s-matrix}"

INSTALLPLAN_NAME="${INSTALLPLAN_NAME:-frontend-forge}"
INSTALLPLAN_CREATOR="${INSTALLPLAN_CREATOR:-admin}"
FRONTEND_FORGE_VERSION="${FRONTEND_FORGE_VERSION:-1.0.0-rc.1}"
INSTALLPLAN_WAIT_TIMEOUT_SECONDS="${INSTALLPLAN_WAIT_TIMEOUT_SECONDS:-600}"
POLL_INTERVAL_SECONDS="${POLL_INTERVAL_SECONDS:-5}"

CURRENT_CLUSTER=""
CURRENT_REMOTE_KUBECONFIG=""
CURRENT_REMOTE_WORK_DIR=""
CURRENT_ARTIFACT_DIR=""

usage() {
  cat <<'EOF'
Usage:
  scripts/k8s-matrix-step2-install-frontend-forge.sh <k8s-version>

Example:
  REMOTE_SSH_TARGET=root@<remote-host> scripts/k8s-matrix-step2-install-frontend-forge.sh 1.32

Environment:
  REMOTE_SSH_TARGET       Required (example: root@<remote-host>)
  REMOTE_KIND_BIN         Default: /root/go/bin/kind
  INSTALLPLAN_NAME        Default: frontend-forge
  INSTALLPLAN_CREATOR     Default: admin
  FRONTEND_FORGE_VERSION  Default: 1.0.0-rc.1
  INSTALLPLAN_WAIT_TIMEOUT_SECONDS Default: 600
EOF
}

log() {
  printf '[k8s-step2] %s\n' "$*" >&2
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
  [[ -n "$REMOTE_SSH_TARGET" ]] || die "REMOTE_SSH_TARGET is required"
}

prepare_context() {
  local version="$1"
  CURRENT_CLUSTER="$(cluster_name_for_version "$version")"
  CURRENT_REMOTE_KUBECONFIG="$(remote_kubeconfig_for_cluster "$CURRENT_CLUSTER")"
  CURRENT_REMOTE_WORK_DIR="$(remote_work_dir_for_cluster "$CURRENT_CLUSTER")"
  CURRENT_ARTIFACT_DIR="$(artifact_dir_for_version "$version")"
  mkdir -p "$CURRENT_ARTIFACT_DIR"
}

assert_cluster_ready_for_install() {
  log "检查目标集群是否存在"
  run_remote_quiet <<EOF
test -f "${CURRENT_REMOTE_KUBECONFIG}"
kind get clusters | grep -Fx "${CURRENT_CLUSTER}" >/dev/null
kubectl get nodes >/dev/null
EOF
}

apply_installplan() {
  log "apply frontend-forge InstallPlan"
  run_remote_logged "${CURRENT_ARTIFACT_DIR}/step2-install.log" <<EOF
mkdir -p "${CURRENT_REMOTE_WORK_DIR}"

deadline=\$((SECONDS + ${INSTALLPLAN_WAIT_TIMEOUT_SECONDS}))
until kubectl get crd installplans.kubesphere.io >/dev/null 2>&1; do
  (( SECONDS < deadline )) || exit 1
  sleep "${POLL_INTERVAL_SECONDS}"
done

cat > "${CURRENT_REMOTE_WORK_DIR}/frontend-forge-installplan.yaml" <<'YAML'
apiVersion: kubesphere.io/v1alpha1
kind: InstallPlan
metadata:
  name: ${INSTALLPLAN_NAME}
  annotations:
    kubesphere.io/creator: ${INSTALLPLAN_CREATOR}
spec:
  enabled: true
  extension:
    name: frontend-forge
    version: ${FRONTEND_FORGE_VERSION}
YAML

kubectl apply -f "${CURRENT_REMOTE_WORK_DIR}/frontend-forge-installplan.yaml"
kubectl get installplan "${INSTALLPLAN_NAME}" -o yaml
EOF
}

write_summary() {
  local version="$1"
  cat > "${CURRENT_ARTIFACT_DIR}/step2-summary.md" <<EOF
# Step2 Summary

- version: ${version}
- cluster: ${CURRENT_CLUSTER}
- remote_host: ${REMOTE_SSH_TARGET}
- remote_kubeconfig: ${CURRENT_REMOTE_KUBECONFIG}
- installplan_name: ${INSTALLPLAN_NAME}
- frontend_forge_version: ${FRONTEND_FORGE_VERSION}
- next_step_3: REMOTE_SSH_TARGET=root@<remote-host> ./scripts/k8s-matrix-step3-fi-test.sh ${version}
EOF
}

main() {
  local version

  if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    usage
    exit 0
  fi

  [[ $# -eq 1 ]] || die "expect exactly one version; see --help"
  version="$1"

  assert_local_prereqs
  prepare_context "$version"
  assert_cluster_ready_for_install
  apply_installplan
  write_summary "$version"

  log "Step2 完成。继续执行：REMOTE_SSH_TARGET=root@<remote-host> ./scripts/k8s-matrix-step3-fi-test.sh ${version}"
}

main "$@"
