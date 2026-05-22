# Manual Build And Helm Install

Code owners: `scripts/manual-build-kind-helm-install.sh`,
`scripts/fe-full-regression.sh`, Dockerfiles under `crates/*`

## Status

| Workflow | Status | Entry point |
| --- | --- | --- |
| Build FE controller/packager/publisher images | Implemented | `scripts/manual-build-kind-helm-install.sh` |
| Build full runtime/API image set | Implemented | `INSTALL_PROFILE=full` |
| Load images into local or remote kind | Implemented | `KIND_LOAD_IMAGES=true` |
| Helm upgrade/install with image overrides | Implemented | `HELM_INSTALL=true` |
| FE package/download regression | Implemented | `scripts/fe-full-regression.sh` |
| Publish smoke test | Partially implemented | Manual target + API call |

## Scripted Image Build And Install

Status: Implemented

Default profile:

```bash
scripts/manual-build-kind-helm-install.sh
```

Profiles:

| `INSTALL_PROFILE` | Images | Helm scope |
| --- | --- | --- |
| `extension` | FE controller, packager, publisher | Updates FE package/publish path. |
| `full` | Extension images plus FI controller, runner, extension API | Installs runtime, FE path, API, optional build-service. |

Important env:

| Env | Default | Behavior |
| --- | --- | --- |
| `REGISTRY` | `hub.kubesphere.com.cn/kubesphere` | Image registry prefix. |
| `TAG` | `dev-<timestamp>` | Image tag. |
| `BUILDER` | `mybuilder` | Docker buildx builder. |
| `PLATFORM` | `linux/amd64` | Build platform. |
| `KIND_CLUSTER` | `fe` | kind cluster name. |
| `NAMESPACE` | `extension-frontend-forge` | Helm release namespace. |
| `RELEASE` | `frontend-forge` | Helm release name. |
| `REMOTE` | `root@172.31.19.2` | Remote host. Set empty for local kind. |
| `KUBECONFIG_PATH` | `$KUBECONFIG` or `~/.kube/kind-remote` | kubectl/helm kubeconfig. |
| `BUILD_IMAGES` | `true` | Build images. |
| `PUSH_IMAGES` | `true` | Push images; `false` uses local Docker load. |
| `KIND_LOAD_IMAGES` | `true` | Load images into kind. |
| `HELM_INSTALL` | `true` | Run Helm upgrade/install. |
| `HELM_REUSE_VALUES` | `auto` | `true` for extension profile, `false` for full profile. |
| `KSBUILDER_VERSION` | empty | Optional publisher Docker build arg. |
| `FRONTEND_FORGE_IMAGE` | empty | Enables chart build-service when set. |

Image override envs:

| Env | Default pattern |
| --- | --- |
| `EXTENSION_CONTROLLER_IMAGE` | `${REGISTRY}/frontend-extension-controller:${TAG}` |
| `EXTENSION_PACKAGER_IMAGE` | `${REGISTRY}/frontend-forge-extension-packager:${TAG}` |
| `EXTENSION_PUBLISHER_IMAGE` | `${REGISTRY}/frontend-forge-extension-publisher:${TAG}` |
| `RUNTIME_CONTROLLER_IMAGE` | `${REGISTRY}/frontend-forge-controller:${TAG}` |
| `RUNNER_IMAGE` | `${REGISTRY}/frontend-forge-runner:${TAG}` |
| `EXTENSION_API_IMAGE` | `${REGISTRY}/frontend-forge-extension-api:${TAG}` |

Full install with local/e2e build-service image:

```bash
INSTALL_PROFILE=full \
FRONTEND_FORGE_IMAGE="${REGISTRY}/frontend-forge:${TAG}" \
scripts/manual-build-kind-helm-install.sh
```

## Direct Docker Builds

Status: Implemented

FE package/publish path:

```bash
docker buildx build -f crates/frontend-extension-controller/Dockerfile \
  -t "$EXTENSION_CONTROLLER_IMAGE" --push .

docker buildx build -f crates/extension-packager/Dockerfile \
  -t "$EXTENSION_PACKAGER_IMAGE" --push .

docker buildx build -f crates/extension-publisher/Dockerfile \
  -t "$EXTENSION_PUBLISHER_IMAGE" --push .
```

Publisher with explicit `ksbuilder` version:

```bash
docker buildx build -f crates/extension-publisher/Dockerfile \
  --build-arg "KSBUILDER_VERSION=$KSBUILDER_VERSION" \
  -t "$EXTENSION_PUBLISHER_IMAGE" --push .
```

FI runtime/API path:

```bash
docker buildx build -f crates/frontend-forge-controller/Dockerfile \
  -t "$RUNTIME_CONTROLLER_IMAGE" --push .

docker buildx build -f crates/frontend-forge-runner/Dockerfile \
  -t "$RUNNER_IMAGE" --push .

docker buildx build -f crates/frontend-forge-extension-api/Dockerfile \
  -t "$EXTENSION_API_IMAGE" --push .
```

## Helm Image Overrides

Status: Implemented

Extension-only update:

```bash
helm upgrade --install frontend-forge config/charts/frontend-forge \
  --namespace extension-frontend-forge \
  --create-namespace \
  --reuse-values \
  --set extensionController.image.registry="" \
  --set extensionController.image.repository="$EXTENSION_CONTROLLER_IMAGE" \
  --set extensionController.image.tag="" \
  --set extensionController.packagerImage="$EXTENSION_PACKAGER_IMAGE" \
  --set extensionController.publisherImage="$EXTENSION_PUBLISHER_IMAGE"
```

Full runtime/API install:

```bash
helm upgrade --install frontend-forge config/charts/frontend-forge \
  --namespace extension-frontend-forge \
  --create-namespace \
  --set controller.enabled=true \
  --set crds.installJsBundle=true \
  --set image.registry="" \
  --set image.repository="$RUNTIME_CONTROLLER_IMAGE" \
  --set image.tag="" \
  --set runner.image.registry="" \
  --set runner.image.repository="$RUNNER_IMAGE" \
  --set runner.image.tag="" \
  --set extensionController.image.registry="" \
  --set extensionController.image.repository="$EXTENSION_CONTROLLER_IMAGE" \
  --set extensionController.image.tag="" \
  --set extensionController.packagerImage="$EXTENSION_PACKAGER_IMAGE" \
  --set extensionController.publisherImage="$EXTENSION_PUBLISHER_IMAGE" \
  --set extensionApi.image.registry="" \
  --set extensionApi.image.repository="$EXTENSION_API_IMAGE" \
  --set extensionApi.image.tag=""
```

Enable chart build-service stub:

```bash
--set buildService.enabled=true \
--set buildService.image.registry="" \
--set buildService.image.repository="$FRONTEND_FORGE_IMAGE" \
--set buildService.image.tag=""
```

## Extension API Access

Status: Implemented

Cluster API port-forward:

```bash
kubectl -n extension-frontend-forge port-forward \
  svc/frontend-forge-extension-api 18080:80
```

Local debug process:

```bash
EXTENSION_API_BIND_ADDR=127.0.0.1:18080 \
cargo run -p frontend-forge-extension-api
```

Default FE API base for scripts:

```text
http://127.0.0.1:18080/kapis/frontend-forge-api.kubesphere.io/v1alpha1/frontendextensions
```

## FE Regression Script

Status: Implemented

Entry point:

```bash
DOWNLOAD_API_BASE_URL="http://127.0.0.1:18080/kapis/frontend-forge-api.kubesphere.io/v1alpha1/frontendextensions" \
EXPECTED_PACKAGER_IMAGE="$EXTENSION_PACKAGER_IMAGE" \
scripts/fe-full-regression.sh
```

Main env:

| Env | Default | Behavior |
| --- | --- | --- |
| `FE_NAME` | `inspecttask` | FE sample object name. |
| `SAMPLE_FILE` | `config/samples/frontendextension-inspecttask.yaml` | FE sample path. |
| `FRONTEND_FORGE_NAMESPACE` | `extension-frontend-forge` | Job/ConfigMap namespace. |
| `DOWNLOAD_API_BASE_URL` | local FE API `/kapis/...` path | Download endpoint base. |
| `EXPECTED_PACKAGER_IMAGE` | empty | Optional package Job image assertion. |
| `RUN_UPDATE_TEST` | `true` | Applies modified FE after create succeeds. |
| `RUN_REBUILD_TEST` | `true` | Patches rebuild token and verifies rebuild. |
| `APPLY_CRD` | `true` | Applies FE CRD before testing. |

Checks performed:

- Deletes existing FE and related owned Jobs/ConfigMaps.
- Applies FE sample.
- Waits for `status.phase=Ready` and `status.packageJob.phase=Succeeded`.
- Verifies package Job image when `EXPECTED_PACKAGER_IMAGE` is set.
- Downloads artifact through FE API.
- Verifies downloaded package digest against `status.artifact.digest`.
- Optionally applies source update and rebuild-token update.

Artifacts are written under:

```text
artifacts/fe-full-regression/<timestamp>/
```

## Publish Smoke Test

Status: Partially implemented

The controller/API/publisher path is implemented. A working target requires
external `ksbuilder publish` configuration.

Sample target shape:

```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: ksbuilder-publish-config
  namespace: extension-frontend-forge
data:
  args: ""
  env.EXAMPLE: "value"
```

Publisher target data handling:

| Target key | Behavior |
| --- | --- |
| `env.<NAME>` | Passed as environment variable `<NAME>` to `ksbuilder publish`. |
| `args` | Split by whitespace and appended to `ksbuilder publish <package-dir>`. |
| other key | Written under `<workdir>/.frontend-forge-publish-target/<key>`. |

Trigger publish:

```bash
artifact_digest="$(kubectl get frontendextension inspecttask \
  -o jsonpath='{.status.artifact.digest}')"

curl -sS -X POST \
  "http://127.0.0.1:18080/apis/frontend-forge-api.kubesphere.io/v1alpha1/frontendextensions/inspecttask/publish" \
  -H 'Content-Type: application/json' \
  -d "{
    \"requestId\": \"manual-$(date +%s)\",
    \"expectedArtifactDigest\": \"${artifact_digest}\"
  }"
```

Inspect publish:

```bash
kubectl get frontendextension inspecttask \
  -o jsonpath='{.status.publish.phase}{"\n"}{.status.publish.jobRef.name}{"\n"}{.status.publish.lastError}{"\n"}'

publish_job="$(kubectl get frontendextension inspecttask \
  -o jsonpath='{.status.publish.jobRef.name}')"

kubectl -n extension-frontend-forge get job "$publish_job" -o yaml
kubectl -n extension-frontend-forge logs "job/$publish_job"
```

## Troubleshooting Commands

Status: Implemented

```bash
kubectl get frontendextension inspecttask -o yaml

job_name="$(kubectl get frontendextension inspecttask \
  -o jsonpath='{.status.packageJob.name}')"
kubectl -n extension-frontend-forge get job "$job_name" -o yaml
kubectl -n extension-frontend-forge logs "job/$job_name"

cm_name="$(kubectl get frontendextension inspecttask \
  -o jsonpath='{.status.artifact.storage.ref.name}')"
kubectl -n extension-frontend-forge get configmap "$cm_name" -o yaml
```

## TODO / Open Question

Status: Planned / TODO

- Publish target key names beyond `env.*` and `args` depend on external `ksbuilder` configuration.
- The script defaults target a specific remote host; override `REMOTE` for other environments.
