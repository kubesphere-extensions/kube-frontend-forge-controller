#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

REGISTRY="${REGISTRY:-docker.io/spike2044}"
TAG="${TAG:-dev-$(date +%Y%m%d%H%M%S)}"
BUILDER="${BUILDER:-mybuilder}"
PLATFORM="${PLATFORM:-linux/amd64}"
KIND_CLUSTER="${KIND_CLUSTER:-fe}"
NAMESPACE="${NAMESPACE:-extension-frontend-forge}"
RELEASE="${RELEASE:-frontend-forge}"
REMOTE="${REMOTE:-root@172.31.19.2}"
REMOTE_KIND_BIN="${REMOTE_KIND_BIN:-/root/go/bin/kind}"
KUBECONFIG_PATH="${KUBECONFIG_PATH:-${KUBECONFIG:-$HOME/.kube/kind-remote}}"
CHART_DIR="${CHART_DIR:-$ROOT_DIR/config/charts/frontend-forge}"
ARTIFACT_ROOT="${ARTIFACT_ROOT:-$ROOT_DIR/artifacts/manual-build-kind-helm-install}"
RUN_ID="${RUN_ID:-$(date +%Y%m%d-%H%M%S)}"
ARTIFACT_DIR="${ARTIFACT_DIR:-$ARTIFACT_ROOT/$RUN_ID}"

INSTALL_PROFILE="${INSTALL_PROFILE:-extension}"
BUILD_IMAGES="${BUILD_IMAGES:-true}"
PUSH_IMAGES="${PUSH_IMAGES:-true}"
KIND_LOAD_IMAGES="${KIND_LOAD_IMAGES:-true}"
HELM_INSTALL="${HELM_INSTALL:-true}"
HELM_REUSE_VALUES="${HELM_REUSE_VALUES:-auto}"
HELM_FORCE_CONFLICTS_ON_RETRY="${HELM_FORCE_CONFLICTS_ON_RETRY:-true}"
ROLLOUT_TIMEOUT="${ROLLOUT_TIMEOUT:-180s}"

EXTENSION_CONTROLLER_IMAGE="${EXTENSION_CONTROLLER_IMAGE:-${REGISTRY}/frontend-extension-controller:${TAG}}"
EXTENSION_PACKAGER_IMAGE="${EXTENSION_PACKAGER_IMAGE:-${REGISTRY}/frontend-forge-extension-packager:${TAG}}"
EXTENSION_PUBLISHER_IMAGE="${EXTENSION_PUBLISHER_IMAGE:-${REGISTRY}/frontend-forge-extension-publisher:${TAG}}"
RUNTIME_CONTROLLER_IMAGE="${RUNTIME_CONTROLLER_IMAGE:-${REGISTRY}/frontend-forge-controller:${TAG}}"
RUNNER_IMAGE="${RUNNER_IMAGE:-${REGISTRY}/frontend-forge-runner:${TAG}}"
EXTENSION_API_IMAGE="${EXTENSION_API_IMAGE:-${REGISTRY}/frontend-forge-extension-api:${TAG}}"
FRONTEND_FORGE_IMAGE="${FRONTEND_FORGE_IMAGE:-}"

KSBUILDER_VERSION="${KSBUILDER_VERSION:-}"

usage() {
  cat <<'EOF'
Usage:
  scripts/manual-build-kind-helm-install.sh

Profiles:
  INSTALL_PROFILE=extension  Build/load/install FrontendExtension controller, packager, publisher. Default.
  INSTALL_PROFILE=full       Also build/load/install runtime controller, runner, and extension API.

Environment:
  REGISTRY                   Default: docker.io/spike2044
  TAG                        Default: dev-<MMDDHHMM>
  BUILDER                    Default: mybuilder
  PLATFORM                   Default: linux/amd64
  KIND_CLUSTER               Default: fe
  NAMESPACE                  Default: extension-frontend-forge
  RELEASE                    Default: frontend-forge
  REMOTE                     Default: root@172.31.19.2. Set empty for local kind.
  REMOTE_KIND_BIN            Default: /root/go/bin/kind
  KUBECONFIG_PATH            Default: $KUBECONFIG or ~/.kube/kind-remote
  CHART_DIR                  Default: config/charts/frontend-forge
  ARTIFACT_DIR               Default: artifacts/manual-build-kind-helm-install/<timestamp>
  BUILD_IMAGES               Default: true
  PUSH_IMAGES                Default: true. Set false to docker buildx --load instead of --push.
  KIND_LOAD_IMAGES           Default: true
  HELM_INSTALL               Default: true
  HELM_REUSE_VALUES          Default: auto. auto=true for extension profile, false for full profile.
  HELM_FORCE_CONFLICTS_ON_RETRY
                             Default: true. Retry Helm upgrade with --force-conflicts after apply conflicts.
  KSBUILDER_VERSION          Optional build arg for extension-publisher Dockerfile.
  FRONTEND_FORGE_IMAGE       Optional build-service image. When set, Helm enables buildService with this image.

Image overrides:
  EXTENSION_CONTROLLER_IMAGE
  EXTENSION_PACKAGER_IMAGE
  EXTENSION_PUBLISHER_IMAGE
  RUNTIME_CONTROLLER_IMAGE
  RUNNER_IMAGE
  EXTENSION_API_IMAGE
EOF
}

log() {
  printf '[manual-build-kind-helm-install] %s\n' "$*" >&2
}

die() {
  log "$*"
  exit 1
}

is_true() {
  [[ "$1" == "true" || "$1" == "1" || "$1" == "yes" ]]
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

remote_exec() {
  local command="$1"
  ssh -T "$REMOTE" "bash --noprofile --norc -euo pipefail -c $(printf '%q' "$command")"
}

assert_prereqs() {
  require_cmd docker
  require_cmd helm
  require_cmd kubectl
  if [[ -n "$REMOTE" ]]; then
    require_cmd ssh
  else
    require_cmd kind
  fi
  [[ -f "$CHART_DIR/Chart.yaml" ]] || die "chart not found: $CHART_DIR"
  [[ -f "$KUBECONFIG_PATH" ]] || die "kubeconfig not found: $KUBECONFIG_PATH"
  case "$INSTALL_PROFILE" in
    extension|full) ;;
    *) die "unsupported INSTALL_PROFILE=$INSTALL_PROFILE; expected extension or full" ;;
  esac
}

prepare_artifacts() {
  mkdir -p "$ARTIFACT_DIR"
}

build_image() {
  local dockerfile="$1"
  local image="$2"
  shift 2
  local output_arg="--push"
  local action="building and pushing"

  if ! is_true "$PUSH_IMAGES"; then
    output_arg="--load"
    action="building into local Docker"
  fi

  log "$action $image from $dockerfile"
  docker buildx build \
    --builder "$BUILDER" \
    --platform "$PLATFORM" \
    -f "$dockerfile" \
    -t "$image" \
    "$output_arg" \
    "$@" \
    "$ROOT_DIR"
}

build_images() {
  if ! is_true "$BUILD_IMAGES"; then
    log "skipping image build because BUILD_IMAGES=$BUILD_IMAGES"
    return
  fi

  docker buildx use "$BUILDER"

  build_image "$ROOT_DIR/crates/frontend-extension-controller/Dockerfile" "$EXTENSION_CONTROLLER_IMAGE"
  build_image "$ROOT_DIR/crates/extension-packager/Dockerfile" "$EXTENSION_PACKAGER_IMAGE"

  if [[ -n "$KSBUILDER_VERSION" ]]; then
    build_image "$ROOT_DIR/crates/extension-publisher/Dockerfile" "$EXTENSION_PUBLISHER_IMAGE" \
      --build-arg "KSBUILDER_VERSION=$KSBUILDER_VERSION"
  else
    build_image "$ROOT_DIR/crates/extension-publisher/Dockerfile" "$EXTENSION_PUBLISHER_IMAGE"
  fi

  if [[ "$INSTALL_PROFILE" == "full" ]]; then
    build_image "$ROOT_DIR/crates/frontend-forge-controller/Dockerfile" "$RUNTIME_CONTROLLER_IMAGE"
    build_image "$ROOT_DIR/crates/frontend-forge-runner/Dockerfile" "$RUNNER_IMAGE"
    build_image "$ROOT_DIR/crates/frontend-forge-extension-api/Dockerfile" "$EXTENSION_API_IMAGE"
  fi
}

images_to_load() {
  printf '%s\n' "$EXTENSION_CONTROLLER_IMAGE"
  printf '%s\n' "$EXTENSION_PACKAGER_IMAGE"
  printf '%s\n' "$EXTENSION_PUBLISHER_IMAGE"
  if [[ "$INSTALL_PROFILE" == "full" ]]; then
    printf '%s\n' "$RUNTIME_CONTROLLER_IMAGE"
    printf '%s\n' "$RUNNER_IMAGE"
    printf '%s\n' "$EXTENSION_API_IMAGE"
  fi
  if [[ -n "$FRONTEND_FORGE_IMAGE" ]]; then
    printf '%s\n' "$FRONTEND_FORGE_IMAGE"
  fi
}

kind_load_images() {
  if ! is_true "$KIND_LOAD_IMAGES"; then
    log "skipping kind load because KIND_LOAD_IMAGES=$KIND_LOAD_IMAGES"
    return
  fi

  local images_file="$ARTIFACT_DIR/images.txt"
  images_to_load > "$images_file"

  if [[ -n "$REMOTE" ]]; then
    log "loading images into remote kind cluster $KIND_CLUSTER on $REMOTE"
    while IFS= read -r image; do
      [[ -n "$image" ]] || continue
      if is_true "$PUSH_IMAGES"; then
        remote_exec "docker pull '$image' && '$REMOTE_KIND_BIN' load docker-image --name '$KIND_CLUSTER' '$image'"
      else
        log "streaming local image $image to remote Docker"
        docker image inspect "$image" >/dev/null
        docker save "$image" | remote_exec "docker load >/dev/null && '$REMOTE_KIND_BIN' load docker-image --name '$KIND_CLUSTER' '$image'"
      fi
    done < "$images_file"
  else
    log "loading images into local kind cluster $KIND_CLUSTER"
    while IFS= read -r image; do
      [[ -n "$image" ]] || continue
      if is_true "$PUSH_IMAGES"; then
        docker pull "$image"
      else
        docker image inspect "$image" >/dev/null
      fi
      kind load docker-image --name "$KIND_CLUSTER" "$image"
    done < "$images_file"
  fi
}

helm_should_reuse_values() {
  if [[ "$HELM_REUSE_VALUES" == "auto" ]]; then
    [[ "$INSTALL_PROFILE" == "extension" ]]
  else
    is_true "$HELM_REUSE_VALUES"
  fi
}

write_helm_values() {
  local values_file="$ARTIFACT_DIR/frontend-forge-values.yaml"
  cat > "$values_file" <<EOF
crds:
  installJsBundle: true

extensionController:
  enabled: true
  image:
    registry: ""
    repository: ${EXTENSION_CONTROLLER_IMAGE}
    tag: ""
  packagerImage: ${EXTENSION_PACKAGER_IMAGE}
  publisherImage: ${EXTENSION_PUBLISHER_IMAGE}

extensionPackager:
  image:
    registry: ""
    repository: ${EXTENSION_PACKAGER_IMAGE}
    tag: ""

extensionPublisher:
  image:
    registry: ""
    repository: ${EXTENSION_PUBLISHER_IMAGE}
    tag: ""
EOF

  if [[ "$INSTALL_PROFILE" == "full" ]]; then
    cat >> "$values_file" <<EOF

image:
  registry: ""
  repository: ${RUNTIME_CONTROLLER_IMAGE}
  tag: ""

runner:
  image:
    registry: ""
    repository: ${RUNNER_IMAGE}
    tag: ""

extensionApi:
  enabled: true
  image:
    registry: ""
    repository: ${EXTENSION_API_IMAGE}
    tag: ""
EOF
  fi

  if [[ -n "$FRONTEND_FORGE_IMAGE" ]]; then
    cat >> "$values_file" <<EOF

buildService:
  enabled: true
  image:
    registry: ""
    repository: ${FRONTEND_FORGE_IMAGE}
    tag: ""
EOF
  fi

  printf '%s\n' "$values_file"
}

helm_install() {
  if ! is_true "$HELM_INSTALL"; then
    log "skipping Helm install because HELM_INSTALL=$HELM_INSTALL"
    return
  fi

  local values_file
  values_file="$(write_helm_values)"

  local helm_args=(
    upgrade --install "$RELEASE" "$CHART_DIR"
    --kubeconfig "$KUBECONFIG_PATH"
    --namespace "$NAMESPACE"
    --create-namespace
    --values "$values_file"
  )

  if helm_should_reuse_values; then
    helm_args+=(--reuse-values)
  fi

  log "running Helm upgrade/install release=$RELEASE namespace=$NAMESPACE profile=$INSTALL_PROFILE"
  set +e
  helm "${helm_args[@]}" 2>&1 | tee "$ARTIFACT_DIR/helm-upgrade-install.log"
  local helm_status=${PIPESTATUS[0]}
  set -e

  if (( helm_status == 0 )); then
    return
  fi

  if is_true "$HELM_FORCE_CONFLICTS_ON_RETRY" \
    && grep -q 'conflict occurred while applying object' "$ARTIFACT_DIR/helm-upgrade-install.log"; then
    log "Helm upgrade hit apply conflicts; retrying with --force-conflicts"
    helm "${helm_args[@]}" --force-conflicts 2>&1 | tee "$ARTIFACT_DIR/helm-upgrade-install-force-conflicts.log"
    return
  fi

  return "$helm_status"
}

verify_rollout() {
  if ! is_true "$HELM_INSTALL"; then
    return
  fi

  log "waiting for frontend extension controller rollout"
  kubectl --kubeconfig "$KUBECONFIG_PATH" -n "$NAMESPACE" \
    rollout status deploy/frontend-forge-extension-controller --timeout="$ROLLOUT_TIMEOUT" \
    | tee "$ARTIFACT_DIR/frontend-extension-controller-rollout.log"

  kubectl --kubeconfig "$KUBECONFIG_PATH" -n "$NAMESPACE" \
    get deploy frontend-forge-extension-controller \
    -o jsonpath='{.spec.template.spec.containers[0].image}{"\n"}{.spec.template.spec.containers[0].env[?(@.name=="PACKAGER_IMAGE")].value}{"\n"}{.spec.template.spec.containers[0].env[?(@.name=="PUBLISHER_IMAGE")].value}{"\n"}' \
    | tee "$ARTIFACT_DIR/frontend-extension-controller-images.txt"

  if [[ "$INSTALL_PROFILE" == "full" ]]; then
    log "waiting for runtime controller and extension API rollout"
    kubectl --kubeconfig "$KUBECONFIG_PATH" -n "$NAMESPACE" \
      rollout status deploy/frontend-forge-controller --timeout="$ROLLOUT_TIMEOUT" \
      | tee "$ARTIFACT_DIR/frontend-forge-controller-rollout.log"
    kubectl --kubeconfig "$KUBECONFIG_PATH" -n "$NAMESPACE" \
      rollout status deploy/frontend-forge-extension-api --timeout="$ROLLOUT_TIMEOUT" \
      | tee "$ARTIFACT_DIR/frontend-forge-extension-api-rollout.log"
  fi
}

write_summary() {
  cat > "$ARTIFACT_DIR/summary.env" <<EOF
REGISTRY=${REGISTRY}
TAG=${TAG}
BUILDER=${BUILDER}
PLATFORM=${PLATFORM}
INSTALL_PROFILE=${INSTALL_PROFILE}
BUILD_IMAGES=${BUILD_IMAGES}
PUSH_IMAGES=${PUSH_IMAGES}
KIND_LOAD_IMAGES=${KIND_LOAD_IMAGES}
HELM_INSTALL=${HELM_INSTALL}
HELM_FORCE_CONFLICTS_ON_RETRY=${HELM_FORCE_CONFLICTS_ON_RETRY}
KIND_CLUSTER=${KIND_CLUSTER}
NAMESPACE=${NAMESPACE}
RELEASE=${RELEASE}
REMOTE=${REMOTE}
KUBECONFIG_PATH=${KUBECONFIG_PATH}
EXTENSION_CONTROLLER_IMAGE=${EXTENSION_CONTROLLER_IMAGE}
EXTENSION_PACKAGER_IMAGE=${EXTENSION_PACKAGER_IMAGE}
EXTENSION_PUBLISHER_IMAGE=${EXTENSION_PUBLISHER_IMAGE}
RUNTIME_CONTROLLER_IMAGE=${RUNTIME_CONTROLLER_IMAGE}
RUNNER_IMAGE=${RUNNER_IMAGE}
EXTENSION_API_IMAGE=${EXTENSION_API_IMAGE}
FRONTEND_FORGE_IMAGE=${FRONTEND_FORGE_IMAGE}
EOF
}

main() {
  if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    usage
    exit 0
  fi

  assert_prereqs
  prepare_artifacts
  write_summary
  build_images
  kind_load_images
  helm_install
  verify_rollout

  log "PASS artifacts=$ARTIFACT_DIR"
}

main "$@"
