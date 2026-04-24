#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

KUBECONFIG_PATH="${KUBECONFIG_PATH:-${KUBECONFIG:-$HOME/.kube/kind-remote}}"
FRONTEND_FORGE_NAMESPACE="${FRONTEND_FORGE_NAMESPACE:-extension-frontend-forge}"
FE_NAME="${FE_NAME:-inspecttask}"
SAMPLE_FILE="${SAMPLE_FILE:-$ROOT_DIR/config/samples/frontendextension-inspecttask.yaml}"
ARTIFACT_ROOT="${ARTIFACT_ROOT:-$ROOT_DIR/artifacts/fe-full-regression}"
RUN_ID="${RUN_ID:-$(date +%Y%m%d-%H%M%S)}"
ARTIFACT_DIR="${ARTIFACT_DIR:-$ARTIFACT_ROOT/$RUN_ID}"
DOWNLOAD_API_BASE_URL="${DOWNLOAD_API_BASE_URL:-http://127.0.0.1:18080/apis/frontend-forge.kubesphere.io/v1alpha1/frontendextensions}"
EXPECTED_PACKAGER_IMAGE="${EXPECTED_PACKAGER_IMAGE:-}"
TIMEOUT_SECONDS="${TIMEOUT_SECONDS:-300}"
POLL_INTERVAL_SECONDS="${POLL_INTERVAL_SECONDS:-2}"
CLEANUP_ON_SUCCESS="${CLEANUP_ON_SUCCESS:-false}"

KUBECTL=(kubectl --kubeconfig "$KUBECONFIG_PATH")

usage() {
  cat <<'EOF'
Usage:
  scripts/fe-full-regression.sh

Environment:
  KUBECONFIG_PATH           Default: $KUBECONFIG or ~/.kube/kind-remote
  FRONTEND_FORGE_NAMESPACE  Default: extension-frontend-forge
  FE_NAME                   Default: inspecttask
  SAMPLE_FILE               Default: config/samples/frontendextension-inspecttask.yaml
  ARTIFACT_DIR              Default: artifacts/fe-full-regression/<timestamp>
  DOWNLOAD_API_BASE_URL     Default: http://127.0.0.1:18080/apis/frontend-forge.kubesphere.io/v1alpha1/frontendextensions
  EXPECTED_PACKAGER_IMAGE   Optional. When set, assert the package Job uses this image.
  TIMEOUT_SECONDS           Default: 300
  POLL_INTERVAL_SECONDS     Default: 2
  CLEANUP_ON_SUCCESS        Default: false
EOF
}

log() {
  printf '[fe-full-regression] %s\n' "$*" >&2
}

die() {
  log "$*"
  exit 1
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

jsonpath() {
  local resource="$1"
  local expression="$2"
  "${KUBECTL[@]}" get "$resource" -o "jsonpath=$expression"
}

assert_prereqs() {
  require_cmd kubectl
  require_cmd curl
  require_cmd shasum
  [[ -f "$KUBECONFIG_PATH" ]] || die "kubeconfig not found: $KUBECONFIG_PATH"
  [[ -f "$SAMPLE_FILE" ]] || die "sample file not found: $SAMPLE_FILE"
}

prepare_artifact_dir() {
  mkdir -p "$ARTIFACT_DIR"
}

cleanup_previous_run() {
  log "cleaning previous $FE_NAME resources"
  "${KUBECTL[@]}" delete frontendextension "$FE_NAME" --ignore-not-found
  "${KUBECTL[@]}" -n "$FRONTEND_FORGE_NAMESPACE" delete job -l "frontend-forge.io/fe-name=$FE_NAME" --ignore-not-found
  "${KUBECTL[@]}" -n "$FRONTEND_FORGE_NAMESPACE" delete configmap -l "frontend-forge.io/fe-name=$FE_NAME" --ignore-not-found
}

assert_cluster_ready() {
  log "checking cluster prerequisites"
  "${KUBECTL[@]}" get crd frontendextensions.frontend-forge.kubesphere.io >/dev/null
  "${KUBECTL[@]}" -n "$FRONTEND_FORGE_NAMESPACE" get deployment frontend-forge >/dev/null
  "${KUBECTL[@]}" -n "$FRONTEND_FORGE_NAMESPACE" get service frontend-forge >/dev/null
}

create_frontend_extension() {
  log "creating FrontendExtension from $SAMPLE_FILE"
  "${KUBECTL[@]}" apply -f "$SAMPLE_FILE" | tee "$ARTIFACT_DIR/apply.log"
}

wait_for_ready_or_failed() {
  log "waiting for FrontendExtension to reach Ready/Succeeded or Failed"
  local deadline=$((SECONDS + TIMEOUT_SECONDS))
  local phase=""

  while (( SECONDS < deadline )); do
    phase="$(jsonpath "frontendextension/$FE_NAME" '{.status.phase}' 2>/dev/null || true)"
    local package_phase
    local package_job
    package_phase="$(jsonpath "frontendextension/$FE_NAME" '{.status.packageJob.phase}' 2>/dev/null || true)"
    package_job="$(jsonpath "frontendextension/$FE_NAME" '{.status.packageJob.name}' 2>/dev/null || true)"
    log "phase=${phase:-<none>} packageJob=${package_job:-<none>} packageJob.phase=${package_phase:-<none>}"

    if [[ "$phase" == "Failed" ]]; then
      return 0
    fi
    if [[ "$phase" == "Ready" && "$package_phase" == "Succeeded" ]]; then
      return 0
    fi
    sleep "$POLL_INTERVAL_SECONDS"
  done

  die "timed out waiting for $FE_NAME to become Ready/Succeeded or Failed"
}

capture_snapshots() {
  log "capturing resource snapshots"
  "${KUBECTL[@]}" get "frontendextension/$FE_NAME" -o yaml > "$ARTIFACT_DIR/frontendextension.yaml"

  local package_job
  local artifact_cm
  package_job="$(jsonpath "frontendextension/$FE_NAME" '{.status.packageJob.name}')"
  artifact_cm="$(jsonpath "frontendextension/$FE_NAME" '{.status.artifact.storage.ref.name}')"

  [[ -n "$package_job" ]] || die "FrontendExtension status.packageJob.name is empty"
  "${KUBECTL[@]}" -n "$FRONTEND_FORGE_NAMESPACE" get "job/$package_job" -o yaml > "$ARTIFACT_DIR/package-job.yaml"
  "${KUBECTL[@]}" -n "$FRONTEND_FORGE_NAMESPACE" logs "job/$package_job" > "$ARTIFACT_DIR/package-job.log"

  if [[ -n "$artifact_cm" ]]; then
    "${KUBECTL[@]}" -n "$FRONTEND_FORGE_NAMESPACE" get "configmap/$artifact_cm" -o yaml > "$ARTIFACT_DIR/artifact-configmap.yaml"
  fi
}

assert_ready_status() {
  log "validating FrontendExtension Ready status"
  local phase package_phase download_ready artifact_cm digest size_bytes
  phase="$(jsonpath "frontendextension/$FE_NAME" '{.status.phase}')"
  package_phase="$(jsonpath "frontendextension/$FE_NAME" '{.status.packageJob.phase}')"
  download_ready="$(jsonpath "frontendextension/$FE_NAME" '{.status.download.ready}')"
  artifact_cm="$(jsonpath "frontendextension/$FE_NAME" '{.status.artifact.storage.ref.name}')"
  digest="$(jsonpath "frontendextension/$FE_NAME" '{.status.artifact.digest}')"
  size_bytes="$(jsonpath "frontendextension/$FE_NAME" '{.status.artifact.sizeBytes}')"

  [[ "$phase" == "Ready" ]] || die "expected FE phase Ready, got $phase"
  [[ "$package_phase" == "Succeeded" ]] || die "expected packageJob.phase Succeeded, got $package_phase"
  [[ "$download_ready" == "true" ]] || die "expected download.ready true, got $download_ready"
  [[ -n "$artifact_cm" ]] || die "artifact ConfigMap ref is empty"
  [[ "$digest" == sha256:* ]] || die "artifact digest is invalid: $digest"
  [[ "$size_bytes" =~ ^[0-9]+$ ]] || die "artifact sizeBytes is invalid: $size_bytes"
  (( size_bytes > 0 )) || die "artifact sizeBytes must be positive"
}

assert_package_job() {
  log "validating package Job"
  local package_job job_image succeeded
  package_job="$(jsonpath "frontendextension/$FE_NAME" '{.status.packageJob.name}')"
  job_image="$("${KUBECTL[@]}" -n "$FRONTEND_FORGE_NAMESPACE" get "job/$package_job" -o jsonpath='{.spec.template.spec.containers[0].image}')"
  succeeded="$("${KUBECTL[@]}" -n "$FRONTEND_FORGE_NAMESPACE" get "job/$package_job" -o jsonpath='{.status.succeeded}')"

  [[ "$succeeded" == "1" ]] || die "expected package Job succeeded=1, got ${succeeded:-<empty>}"
  if [[ -n "$EXPECTED_PACKAGER_IMAGE" ]]; then
    [[ "$job_image" == "$EXPECTED_PACKAGER_IMAGE" ]] || die "expected package Job image $EXPECTED_PACKAGER_IMAGE, got $job_image"
  fi

  printf 'packageJob=%s\nimage=%s\nsucceeded=%s\n' "$package_job" "$job_image" "$succeeded" > "$ARTIFACT_DIR/package-job-summary.txt"
}

download_and_verify_artifact() {
  log "downloading artifact through HTTP API"
  local filename digest expected_sha downloaded_sha output_file
  filename="$(jsonpath "frontendextension/$FE_NAME" '{.status.download.filename}')"
  digest="$(jsonpath "frontendextension/$FE_NAME" '{.status.artifact.digest}')"
  expected_sha="${digest#sha256:}"
  output_file="$ARTIFACT_DIR/$filename"

  curl -fL "$DOWNLOAD_API_BASE_URL/$FE_NAME/download" -o "$output_file"
  downloaded_sha="$(shasum -a 256 "$output_file" | awk '{print $1}')"
  [[ "$downloaded_sha" == "$expected_sha" ]] || die "download sha256 mismatch: expected $expected_sha, got $downloaded_sha"

  wc -c "$output_file" > "$ARTIFACT_DIR/download-size.txt"
  shasum -a 256 "$output_file" > "$ARTIFACT_DIR/download-sha256.txt"
}

cleanup_success_resources() {
  if [[ "$CLEANUP_ON_SUCCESS" == "true" ]]; then
    cleanup_previous_run
  fi
}

main() {
  if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    usage
    exit 0
  fi

  assert_prereqs
  prepare_artifact_dir
  assert_cluster_ready
  cleanup_previous_run
  create_frontend_extension
  wait_for_ready_or_failed
  capture_snapshots
  assert_ready_status
  assert_package_job
  download_and_verify_artifact
  cleanup_success_resources

  log "PASS artifacts=$ARTIFACT_DIR"
}

main "$@"
