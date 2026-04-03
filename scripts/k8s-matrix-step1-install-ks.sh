#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

REMOTE_SSH_TARGET="${REMOTE_SSH_TARGET:-}"
REMOTE_KIND_BIN="${REMOTE_KIND_BIN:-/root/go/bin/kind}"
REMOTE_KUBECONFIG_ROOT="${REMOTE_KUBECONFIG_ROOT:-/root/.kube/frontend-forge-matrix}"
REMOTE_WORK_ROOT="${REMOTE_WORK_ROOT:-/root/.frontend-forge-matrix}"
ARTIFACT_ROOT="${ARTIFACT_ROOT:-$ROOT_DIR/artifacts/k8s-matrix}"

KS_CHART="${KS_CHART:-oci://hub.kubesphere.com.cn/kse/ks-core}"
KS_VERSION="${KS_VERSION:-1.2.4}"
KS_HELM_TIMEOUT="${KS_HELM_TIMEOUT:-30m}"
KUBESPHERE_NAMESPACE="${KUBESPHERE_NAMESPACE:-kubesphere-system}"

KIND_WAIT="${KIND_WAIT:-5m}"
MIN_FREE_GB="${MIN_FREE_GB:-20}"
MIN_MEM_AVAILABLE_GB="${MIN_MEM_AVAILABLE_GB:-8}"
KIND_EXPOSE_30880_HOST_PORT="${KIND_EXPOSE_30880_HOST_PORT:-30880}"

CURRENT_CLUSTER=""
CURRENT_REMOTE_KUBECONFIG=""
CURRENT_REMOTE_WORK_DIR=""
CURRENT_ARTIFACT_DIR=""

usage() {
  cat <<'EOF'
Usage:
  scripts/k8s-matrix-step1-install-ks.sh <k8s-version>

Example:
  REMOTE_SSH_TARGET=root@<remote-host> scripts/k8s-matrix-step1-install-ks.sh 1.32

Environment:
  REMOTE_SSH_TARGET               Required (example: root@<remote-host>)
  REMOTE_KIND_BIN                 Default: /root/go/bin/kind
  KIND_EXPOSE_30880_HOST_PORT     Default: 30880
EOF
}

log() {
  printf '[k8s-step1] %s\n' "$*" >&2
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

node_image_for_version() {
  local version="$1"
  case "$version" in
    1.23) printf '%s\n' 'kindest/node:v1.23.17' ;;
    1.26) printf '%s\n' 'kindest/node:v1.26.15' ;;
    1.28) printf '%s\n' 'kindest/node:v1.28.15' ;;
    1.30) printf '%s\n' 'kindest/node:v1.30.13' ;;
    1.32) printf '%s\n' 'kindest/node:v1.32.11' ;;
    1.34) printf '%s\n' 'kindest/node:v1.34.3' ;;
    *) return 1 ;;
  esac
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
  rm -rf "$CURRENT_ARTIFACT_DIR"
  mkdir -p "$CURRENT_ARTIFACT_DIR"
}

assert_remote_prereqs() {
  log "检查远程依赖和资源余量"
  run_remote_quiet <<EOF
command -v docker >/dev/null 2>&1
command -v kubectl >/dev/null 2>&1
command -v helm >/dev/null 2>&1
test -x "${REMOTE_KIND_BIN}"

free_kb=\$(df -Pk / | awk 'NR==2 {print \$4}')
required_free_kb=$((MIN_FREE_GB * 1024 * 1024))
if (( free_kb < required_free_kb )); then
  echo "insufficient disk: free_kb=\$free_kb required_kb=\$required_free_kb" >&2
  exit 1
fi

mem_available_kb=\$(awk '/MemAvailable:/ {print \$2}' /proc/meminfo)
required_mem_kb=$((MIN_MEM_AVAILABLE_GB * 1024 * 1024))
if (( mem_available_kb < required_mem_kb )); then
  echo "insufficient memory: mem_available_kb=\$mem_available_kb required_kb=\$required_mem_kb" >&2
  exit 1
fi

mkdir -p "${REMOTE_KUBECONFIG_ROOT}" "${REMOTE_WORK_ROOT}"
EOF
}

cleanup_existing_cluster() {
  log "清理历史 cluster: ${CURRENT_CLUSTER}"
  run_remote_quiet <<EOF
mkdir -p "${REMOTE_KUBECONFIG_ROOT}" "${CURRENT_REMOTE_WORK_DIR}"
kind delete cluster --name "${CURRENT_CLUSTER}" >/dev/null 2>&1 || true
rm -f "${CURRENT_REMOTE_KUBECONFIG}"
rm -rf "${CURRENT_REMOTE_WORK_DIR}"
mkdir -p "${CURRENT_REMOTE_WORK_DIR}"
EOF
}

create_kind_cluster() {
  local version="$1"
  local image
  image="$(node_image_for_version "$version" || true)"
  [[ -n "$image" ]] || die "unsupported version: $version"

  log "创建 kind 集群 ${CURRENT_CLUSTER} (${image})，暴露 host:${KIND_EXPOSE_30880_HOST_PORT} -> node:30880"
  run_remote_logged "${CURRENT_ARTIFACT_DIR}/step1-install.log" <<EOF
cat > "${CURRENT_REMOTE_WORK_DIR}/kind-config.yaml" <<'KINDCFG'
kind: Cluster
apiVersion: kind.x-k8s.io/v1alpha4
nodes:
- role: control-plane
  extraPortMappings:
  - containerPort: 30880
    hostPort: ${KIND_EXPOSE_30880_HOST_PORT}
    listenAddress: "0.0.0.0"
    protocol: TCP
KINDCFG

kind create cluster \
  --name "${CURRENT_CLUSTER}" \
  --image "${image}" \
  --config "${CURRENT_REMOTE_WORK_DIR}/kind-config.yaml" \
  --kubeconfig "${CURRENT_REMOTE_KUBECONFIG}" \
  --wait "${KIND_WAIT}"

kubectl cluster-info
kubectl get nodes -o wide
EOF
}

install_ks() {
  log "安装 KubeSphere"
  run_remote_logged "${CURRENT_ARTIFACT_DIR}/step1-install.log" <<EOF
chart="${KS_CHART}"
version="${KS_VERSION}"
extensionRepo="release-11.2-nightly-\$(TZ=Asia/Shanghai date +%Y%m%d)"

helm upgrade --install -n "${KUBESPHERE_NAMESPACE}" --create-namespace ks-core "\$chart" \
  --debug \
  --wait \
  --timeout "${KS_HELM_TIMEOUT}" \
  --version "\$version" \
  --reset-values \
  --set kseExtensionRepository.image.tag="\$extensionRepo"

kubectl patch repositories.kubesphere.io extensions-museum --type=merge \
  -p='{"status":{"lastSyncTime":null}}'
EOF
}

write_summary() {
  local version="$1"
  cat > "${CURRENT_ARTIFACT_DIR}/step1-summary.md" <<EOF
# Step1 Summary

- version: ${version}
- cluster: ${CURRENT_CLUSTER}
- remote_host: ${REMOTE_SSH_TARGET}
- remote_kubeconfig: ${CURRENT_REMOTE_KUBECONFIG}
- exposed_console_port: ${KIND_EXPOSE_30880_HOST_PORT}
- next_step_2: REMOTE_SSH_TARGET=root@<remote-host> ./scripts/k8s-matrix-step2-install-frontend-forge.sh ${version}
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
  node_image_for_version "$version" >/dev/null || die "unsupported Kubernetes version: ${version}"

  assert_local_prereqs
  prepare_context "$version"
  assert_remote_prereqs
  cleanup_existing_cluster
  create_kind_cluster "$version"
  install_ks
  write_summary "$version"

  log "Step1 完成。继续执行：REMOTE_SSH_TARGET=root@<remote-host> ./scripts/k8s-matrix-step2-install-frontend-forge.sh ${version}"
}

main "$@"
