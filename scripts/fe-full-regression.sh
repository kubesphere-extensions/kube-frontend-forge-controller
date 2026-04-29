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
RUN_UPDATE_TEST="${RUN_UPDATE_TEST:-true}"

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
  RUN_UPDATE_TEST           Default: true. Apply a modified FrontendExtension after create succeeds.
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

sample_frontend_extension_name() {
  perl -0ne 'print "$1\n" if /^kind:\s*FrontendExtension\s*$.*?^metadata:\s*$.*?^\s{2}name:\s*([A-Za-z0-9_.-]+)\s*$/ms' "$SAMPLE_FILE"
}

jsonpath() {
  local resource="$1"
  local expression="$2"
  "${KUBECTL[@]}" get "$resource" -o "jsonpath=$expression"
}

assert_prereqs() {
  require_cmd kubectl
  require_cmd curl
  require_cmd perl
  require_cmd shasum
  [[ -f "$KUBECONFIG_PATH" ]] || die "kubeconfig not found: $KUBECONFIG_PATH"
  [[ -f "$SAMPLE_FILE" ]] || die "sample file not found: $SAMPLE_FILE"

  local sample_name
  sample_name="$(sample_frontend_extension_name)"
  [[ -n "$sample_name" ]] || die "failed to read FrontendExtension metadata.name from $SAMPLE_FILE"
  [[ "$sample_name" == "$FE_NAME" ]] || die "FE_NAME=$FE_NAME does not match $SAMPLE_FILE metadata.name=$sample_name"
}

prepare_artifact_dir() {
  mkdir -p "$ARTIFACT_DIR"
}

cleanup_previous_run() {
  log "checking whether FrontendExtension $FE_NAME exists before create"
  if "${KUBECTL[@]}" get frontendextension "$FE_NAME" >/dev/null 2>&1; then
    log "existing FrontendExtension found; deleting it and verifying related resources are removed"
    mkdir -p "$ARTIFACT_DIR/pre-delete"
    "${KUBECTL[@]}" get frontendextension "$FE_NAME" -o yaml > "$ARTIFACT_DIR/pre-delete/frontendextension.yaml"
    "${KUBECTL[@]}" -n "$FRONTEND_FORGE_NAMESPACE" get job -l "frontend-forge.io/fe-name=$FE_NAME" -o yaml > "$ARTIFACT_DIR/pre-delete/jobs.yaml" 2>/dev/null || true
    "${KUBECTL[@]}" -n "$FRONTEND_FORGE_NAMESPACE" get configmap -l "frontend-forge.io/fe-name=$FE_NAME" -o yaml > "$ARTIFACT_DIR/pre-delete/configmaps.yaml" 2>/dev/null || true

    "${KUBECTL[@]}" delete frontendextension "$FE_NAME" --wait=true | tee "$ARTIFACT_DIR/delete-existing.log"
    wait_for_frontend_extension_deleted
    wait_for_related_resources_deleted
  else
    log "no existing FrontendExtension found"
    cleanup_orphaned_related_resources
  fi
}

cleanup_orphaned_related_resources() {
  local orphan_jobs orphan_configmaps
  orphan_jobs="$("${KUBECTL[@]}" -n "$FRONTEND_FORGE_NAMESPACE" get job -l "frontend-forge.io/fe-name=$FE_NAME" -o name 2>/dev/null || true)"
  orphan_configmaps="$("${KUBECTL[@]}" -n "$FRONTEND_FORGE_NAMESPACE" get configmap -l "frontend-forge.io/fe-name=$FE_NAME" -o name 2>/dev/null || true)"
  if [[ -n "$orphan_jobs$orphan_configmaps" ]]; then
    log "cleaning orphaned related resources because FrontendExtension $FE_NAME is absent"
    mkdir -p "$ARTIFACT_DIR/orphan-cleanup"
    printf '%s\n' "$orphan_jobs" > "$ARTIFACT_DIR/orphan-cleanup/jobs.txt"
    printf '%s\n' "$orphan_configmaps" > "$ARTIFACT_DIR/orphan-cleanup/configmaps.txt"
    "${KUBECTL[@]}" -n "$FRONTEND_FORGE_NAMESPACE" delete job -l "frontend-forge.io/fe-name=$FE_NAME" --ignore-not-found
    "${KUBECTL[@]}" -n "$FRONTEND_FORGE_NAMESPACE" delete configmap -l "frontend-forge.io/fe-name=$FE_NAME" --ignore-not-found
    wait_for_related_resources_deleted
  fi
}

wait_for_frontend_extension_deleted() {
  local deadline=$((SECONDS + TIMEOUT_SECONDS))
  while (( SECONDS < deadline )); do
    if ! "${KUBECTL[@]}" get frontendextension "$FE_NAME" >/dev/null 2>&1; then
      log "FrontendExtension $FE_NAME deleted"
      return 0
    fi
    sleep "$POLL_INTERVAL_SECONDS"
  done
  die "timed out waiting for FrontendExtension $FE_NAME to be deleted"
}

wait_for_related_resources_deleted() {
  local deadline=$((SECONDS + TIMEOUT_SECONDS))
  local jobs configmaps
  while (( SECONDS < deadline )); do
    jobs="$("${KUBECTL[@]}" -n "$FRONTEND_FORGE_NAMESPACE" get job -l "frontend-forge.io/fe-name=$FE_NAME" -o name 2>/dev/null || true)"
    configmaps="$("${KUBECTL[@]}" -n "$FRONTEND_FORGE_NAMESPACE" get configmap -l "frontend-forge.io/fe-name=$FE_NAME" -o name 2>/dev/null || true)"
    if [[ -z "$jobs" && -z "$configmaps" ]]; then
      log "related Jobs and ConfigMaps for $FE_NAME are deleted"
      return 0
    fi
    log "waiting related resources to be deleted jobs=${jobs:-<none>} configmaps=${configmaps:-<none>}"
    sleep "$POLL_INTERVAL_SECONDS"
  done

  "${KUBECTL[@]}" -n "$FRONTEND_FORGE_NAMESPACE" get job -l "frontend-forge.io/fe-name=$FE_NAME" -o yaml > "$ARTIFACT_DIR/delete-leftover-jobs.yaml" 2>/dev/null || true
  "${KUBECTL[@]}" -n "$FRONTEND_FORGE_NAMESPACE" get configmap -l "frontend-forge.io/fe-name=$FE_NAME" -o yaml > "$ARTIFACT_DIR/delete-leftover-configmaps.yaml" 2>/dev/null || true
  die "timed out waiting for related Jobs and ConfigMaps for $FE_NAME to be deleted"
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
  local expected_generation="${1:-}"
  log "waiting for FrontendExtension to reach Ready/Succeeded or Failed"
  local deadline=$((SECONDS + TIMEOUT_SECONDS))
  local phase=""

  while (( SECONDS < deadline )); do
    phase="$(jsonpath "frontendextension/$FE_NAME" '{.status.phase}' 2>/dev/null || true)"
    local package_phase
    local package_job
    local observed_generation
    package_phase="$(jsonpath "frontendextension/$FE_NAME" '{.status.packageJob.phase}' 2>/dev/null || true)"
    package_job="$(jsonpath "frontendextension/$FE_NAME" '{.status.packageJob.name}' 2>/dev/null || true)"
    observed_generation="$(jsonpath "frontendextension/$FE_NAME" '{.status.observedGeneration}' 2>/dev/null || true)"
    log "observedGeneration=${observed_generation:-<none>} phase=${phase:-<none>} packageJob=${package_job:-<none>} packageJob.phase=${package_phase:-<none>}"

    if [[ -n "$expected_generation" && "$observed_generation" != "$expected_generation" ]]; then
      sleep "$POLL_INTERVAL_SECONDS"
      continue
    fi

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
  local stage="${1:-current}"
  log "capturing resource snapshots"
  "${KUBECTL[@]}" get "frontendextension/$FE_NAME" -o yaml > "$ARTIFACT_DIR/${stage}-frontendextension.yaml"

  local package_job
  local artifact_cm
  package_job="$(jsonpath "frontendextension/$FE_NAME" '{.status.packageJob.name}')"
  artifact_cm="$(jsonpath "frontendextension/$FE_NAME" '{.status.artifact.storage.ref.name}')"

  [[ -n "$package_job" ]] || die "FrontendExtension status.packageJob.name is empty"
  "${KUBECTL[@]}" -n "$FRONTEND_FORGE_NAMESPACE" get "job/$package_job" -o yaml > "$ARTIFACT_DIR/${stage}-package-job.yaml"
  "${KUBECTL[@]}" -n "$FRONTEND_FORGE_NAMESPACE" logs "job/$package_job" > "$ARTIFACT_DIR/${stage}-package-job.log"

  if [[ -n "$artifact_cm" ]]; then
    "${KUBECTL[@]}" -n "$FRONTEND_FORGE_NAMESPACE" get "configmap/$artifact_cm" -o yaml > "$ARTIFACT_DIR/${stage}-artifact-configmap.yaml"
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
  local stage="${1:-current}"
  log "validating package Job"
  local package_job job_image succeeded
  package_job="$(jsonpath "frontendextension/$FE_NAME" '{.status.packageJob.name}')"
  job_image="$("${KUBECTL[@]}" -n "$FRONTEND_FORGE_NAMESPACE" get "job/$package_job" -o jsonpath='{.spec.template.spec.containers[0].image}')"
  succeeded="$("${KUBECTL[@]}" -n "$FRONTEND_FORGE_NAMESPACE" get "job/$package_job" -o jsonpath='{.status.succeeded}')"

  [[ "$succeeded" == "1" ]] || die "expected package Job succeeded=1, got ${succeeded:-<empty>}"
  if [[ -n "$EXPECTED_PACKAGER_IMAGE" ]]; then
    [[ "$job_image" == "$EXPECTED_PACKAGER_IMAGE" ]] || die "expected package Job image $EXPECTED_PACKAGER_IMAGE, got $job_image"
  fi

  printf 'packageJob=%s\nimage=%s\nsucceeded=%s\n' "$package_job" "$job_image" "$succeeded" > "$ARTIFACT_DIR/${stage}-package-job-summary.txt"
}

download_and_verify_artifact() {
  local stage="${1:-current}"
  log "downloading artifact through HTTP API"
  local filename digest expected_sha downloaded_sha output_file
  filename="$(jsonpath "frontendextension/$FE_NAME" '{.status.download.filename}')"
  digest="$(jsonpath "frontendextension/$FE_NAME" '{.status.artifact.digest}')"
  expected_sha="${digest#sha256:}"
  output_file="$ARTIFACT_DIR/${stage}-${filename}"

  curl -fL "$DOWNLOAD_API_BASE_URL/$FE_NAME/download" -o "$output_file"
  downloaded_sha="$(shasum -a 256 "$output_file" | awk '{print $1}')"
  [[ "$downloaded_sha" == "$expected_sha" ]] || die "download sha256 mismatch: expected $expected_sha, got $downloaded_sha"

  wc -c "$output_file" > "$ARTIFACT_DIR/${stage}-download-size.txt"
  shasum -a 256 "$output_file" > "$ARTIFACT_DIR/${stage}-download-sha256.txt"
}

make_update_manifest() {
  local update_file="$ARTIFACT_DIR/frontendextension-update.yaml"

  perl -ne '
    if (/^spec:\s*$/) {
      $in_spec = 1;
      $in_package = 0;
    } elsif ($in_spec && /^[A-Za-z0-9_-]+:/) {
      $in_spec = $in_package = 0;
    }

    if ($in_spec && /^  package:\s*$/) {
      $in_package = 1;
    } elsif ($in_package && /^  [A-Za-z0-9_-]+:/) {
      $in_package = 0;
    }

    if ($in_package && /^(\s*version:\s*)(["'\'']?)([0-9]+(?:\.[0-9]+)*)(["'\'']?)\s*$/) {
      my ($prefix, $open_quote, $version, $close_quote) = ($1, $2, $3, $4);
      my @parts = split(/\./, $version);
      $parts[-1]++;
      print $prefix . $open_quote . join(".", @parts) . $close_quote . "\n";
      $changed = 1;
      next;
    }

    print;
    END {
      exit($changed ? 0 : 2);
    }
  ' "$SAMPLE_FILE" > "$update_file" || die "failed to generate modified FrontendExtension manifest from $SAMPLE_FILE"
  if cmp -s "$SAMPLE_FILE" "$update_file"; then
    die "failed to generate modified FrontendExtension manifest from $SAMPLE_FILE"
  fi
  log "generated update manifest at $update_file"
  printf '%s\n' "$update_file"
}

update_frontend_extension() {
  local update_file="$1"
  log "updating FrontendExtension from $update_file"
  "${KUBECTL[@]}" apply -f "$update_file" | tee "$ARTIFACT_DIR/update.apply.log"
}

assert_update_changed_package() {
  local old_source_hash="$1"
  local old_job="$2"
  local old_artifact_cm="$3"
  local old_digest="$4"
  local new_source_hash new_job new_artifact_cm new_digest

  new_source_hash="$(jsonpath "frontendextension/$FE_NAME" '{.status.observedSourceHash}')"
  new_job="$(jsonpath "frontendextension/$FE_NAME" '{.status.packageJob.name}')"
  new_artifact_cm="$(jsonpath "frontendextension/$FE_NAME" '{.status.artifact.storage.ref.name}')"
  new_digest="$(jsonpath "frontendextension/$FE_NAME" '{.status.artifact.digest}')"

  [[ "$new_source_hash" != "$old_source_hash" ]] || die "update did not change observedSourceHash"
  [[ "$new_job" != "$old_job" ]] || die "update reused package Job $new_job"
  [[ "$new_artifact_cm" != "$old_artifact_cm" ]] || die "update reused artifact ConfigMap $new_artifact_cm"
  [[ "$new_digest" != "$old_digest" ]] || die "update did not change artifact digest"

  {
    printf 'oldSourceHash=%s\nnewSourceHash=%s\n' "$old_source_hash" "$new_source_hash"
    printf 'oldPackageJob=%s\nnewPackageJob=%s\n' "$old_job" "$new_job"
    printf 'oldArtifactConfigMap=%s\nnewArtifactConfigMap=%s\n' "$old_artifact_cm" "$new_artifact_cm"
    printf 'oldDigest=%s\nnewDigest=%s\n' "$old_digest" "$new_digest"
  } > "$ARTIFACT_DIR/update-summary.txt"
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
  create_generation="$(jsonpath "frontendextension/$FE_NAME" '{.metadata.generation}')"
  wait_for_ready_or_failed "$create_generation"
  capture_snapshots "create"
  assert_ready_status
  assert_package_job "create"
  download_and_verify_artifact "create"

  if [[ "$RUN_UPDATE_TEST" == "true" ]]; then
    create_source_hash="$(jsonpath "frontendextension/$FE_NAME" '{.status.observedSourceHash}')"
    create_job="$(jsonpath "frontendextension/$FE_NAME" '{.status.packageJob.name}')"
    create_artifact_cm="$(jsonpath "frontendextension/$FE_NAME" '{.status.artifact.storage.ref.name}')"
    create_digest="$(jsonpath "frontendextension/$FE_NAME" '{.status.artifact.digest}')"

    update_file="$(make_update_manifest)"
    update_frontend_extension "$update_file"
    update_generation="$(jsonpath "frontendextension/$FE_NAME" '{.metadata.generation}')"
    wait_for_ready_or_failed "$update_generation"
    capture_snapshots "update"
    assert_ready_status
    assert_package_job "update"
    assert_update_changed_package "$create_source_hash" "$create_job" "$create_artifact_cm" "$create_digest"
    download_and_verify_artifact "update"
  fi

  cleanup_success_resources

  log "PASS artifacts=$ARTIFACT_DIR"
}

main "$@"
