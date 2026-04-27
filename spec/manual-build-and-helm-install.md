# 本地手动构建镜像与 Helm 安装

本文档记录本地手动构建镜像、推送到镜像仓库、加载到远端 kind 集群，并通过 Helm 安装或更新 `frontend-forge` 的流程。

## 前提

本地需要：

```bash
docker buildx ls
kubectl version --client
helm version
```

远端 kind 集群需要可访问。当前常用 kubeconfig 是：

```bash
export KUBECONFIG_PATH="$HOME/.kube/kind-remote"
export KUBECONFIG="$KUBECONFIG_PATH"
```

如果 kubeconfig 指向本地转发端口，例如 `https://127.0.0.1:33631`，先启动 SSH 隧道：

```bash
ssh -N -L 33631:127.0.0.1:33631 root@172.31.19.2
```

常用变量：

```bash
export REGISTRY="docker.io/spike2044"
export TAG="dev-$(date +%m%d%H%M)"
export KIND_CLUSTER="fe"
export NAMESPACE="extension-frontend-forge"
export RELEASE="frontend-forge"
export REMOTE="root@172.31.19.2"
export BUILDER="mybuilder"
```

## 脚本化执行

仓库内提供了脚本封装本文档的构建、远端 kind load 和 Helm 安装流程：

```bash
scripts/manual-build-kind-helm-install.sh
```

默认 `INSTALL_PROFILE=extension`，会构建并推送：

- `frontend-extension-controller`
- `frontend-forge-extension-packager`
- `frontend-forge-extension-publisher`

然后把镜像加载到远端 kind，并通过 Helm 更新 `frontend-forge-extension-controller` 使用的 controller、packager、publisher 镜像。

完整安装 runtime controller、runner、extension API 时：

```bash
INSTALL_PROFILE=full scripts/manual-build-kind-helm-install.sh
```

常用跳过开关：

```bash
BUILD_IMAGES=false KIND_LOAD_IMAGES=false scripts/manual-build-kind-helm-install.sh
HELM_INSTALL=false scripts/manual-build-kind-helm-install.sh
```

只构建本地镜像、不推送 DockerHub：

```bash
PUSH_IMAGES=false scripts/manual-build-kind-helm-install.sh
```

此模式使用 `docker buildx build --load`。如果 `REMOTE` 非空，脚本会通过 `docker save | ssh docker load` 把本地镜像传到远端 Docker，再执行远端 `kind load docker-image`；如果 `REMOTE=""`，则直接加载到本地 kind。

如果 Helm upgrade 因为之前手工执行过 `kubectl set image`、`kubectl set env` 或 `kubectl scale` 出现 server-side apply field manager 冲突，脚本默认会用 `--force-conflicts` 自动重试一次。需要关闭时：

```bash
HELM_FORCE_CONFLICTS_ON_RETRY=false scripts/manual-build-kind-helm-install.sh
```

本地 kind 而不是远端 kind 时：

```bash
REMOTE="" KIND_CLUSTER=fe scripts/manual-build-kind-helm-install.sh
```

如果要测试 publish，还需要一个 publish target。`FrontendExtension.spec.publishPolicy.defaultTargetRef` 默认指向：

```bash
export PUBLISH_TARGET_KIND="ConfigMap"
export PUBLISH_TARGET_NAMESPACE="$NAMESPACE"
export PUBLISH_TARGET_NAME="ksbuilder-publish-config"
```

## 构建并推送镜像

### FrontendExtension controller 和 packager Job

`FrontendExtension` package 链路至少需要这两张镜像版本一致：

- `frontend-extension-controller`
- `frontend-forge-extension-packager`

```bash
export EXTENSION_CONTROLLER_IMAGE="${REGISTRY}/frontend-extension-controller:${TAG}"
export EXTENSION_PACKAGER_IMAGE="${REGISTRY}/frontend-forge-extension-packager:${TAG}"

docker buildx use "$BUILDER"

docker buildx build \
  --builder "$BUILDER" \
  --platform linux/amd64 \
  -f crates/frontend-extension-controller/Dockerfile \
  -t "$EXTENSION_CONTROLLER_IMAGE" \
  --push \
  .

docker buildx build \
  --builder "$BUILDER" \
  --platform linux/amd64 \
  -f crates/extension-packager/Dockerfile \
  -t "$EXTENSION_PACKAGER_IMAGE" \
  --push \
  .
```

注意：这两个 Dockerfile 必须复制 `template` 目录，否则 `include_dir!` 会在镜像构建时找不到 `template/test-fe-demo`：

```dockerfile
COPY template ./template
```

### Publisher Job

如果要测试 `FrontendExtension` 的 publish 链路，还需要构建 publisher Job 镜像：

```bash
export EXTENSION_PUBLISHER_IMAGE="${REGISTRY}/frontend-forge-extension-publisher:${TAG}"

docker buildx build \
  --builder "$BUILDER" \
  --platform linux/amd64 \
  -f crates/extension-publisher/Dockerfile \
  -t "$EXTENSION_PUBLISHER_IMAGE" \
  --push \
  .
```

publisher 镜像内包含：

- `frontend-forge-extension-publisher` binary
- `ksbuilder` binary

默认 `ksbuilder` 版本在 `crates/extension-publisher/Dockerfile` 的 `KSBUILDER_VERSION` 中定义。需要覆盖时：

```bash
docker buildx build \
  --builder "$BUILDER" \
  --platform linux/amd64 \
  -f crates/extension-publisher/Dockerfile \
  --build-arg KSBUILDER_VERSION=0.4.7 \
  -t "$EXTENSION_PUBLISHER_IMAGE" \
  --push \
  .
```

### FrontendIntegration controller 和 runner Job

如果要测试 `FrontendIntegration` 链路，需要构建 runtime controller 和 runner：

```bash
export RUNTIME_CONTROLLER_IMAGE="${REGISTRY}/frontend-forge-controller:${TAG}"
export RUNNER_IMAGE="${REGISTRY}/frontend-forge-runner:${TAG}"

docker buildx build \
  --builder "$BUILDER" \
  --platform linux/amd64 \
  -f crates/frontend-forge-controller/Dockerfile \
  -t "$RUNTIME_CONTROLLER_IMAGE" \
  --push \
  .

docker buildx build \
  --builder "$BUILDER" \
  --platform linux/amd64 \
  -f crates/frontend-forge-runner/Dockerfile \
  -t "$RUNNER_IMAGE" \
  --push \
  .
```

### Extension API

如果希望通过 Helm 部署集群内 extension API，而不是本地 debug 进程：

```bash
export EXTENSION_API_IMAGE="${REGISTRY}/frontend-forge-extension-api:${TAG}"

docker buildx build \
  --builder "$BUILDER" \
  --platform linux/amd64 \
  -f crates/frontend-forge-extension-api/Dockerfile \
  -t "$EXTENSION_API_IMAGE" \
  --push \
  .
```

## 加载镜像到远端 kind

远端机器上需要能访问 Docker 和 kind。当前远端 `kind` 常见路径是 `/root/go/bin/kind`。

```bash
ssh "$REMOTE" "
  set -euo pipefail
  docker pull '$EXTENSION_CONTROLLER_IMAGE'
  docker pull '$EXTENSION_PACKAGER_IMAGE'
  docker pull '$EXTENSION_PUBLISHER_IMAGE'
  /root/go/bin/kind load docker-image --name '$KIND_CLUSTER' '$EXTENSION_CONTROLLER_IMAGE'
  /root/go/bin/kind load docker-image --name '$KIND_CLUSTER' '$EXTENSION_PACKAGER_IMAGE'
  /root/go/bin/kind load docker-image --name '$KIND_CLUSTER' '$EXTENSION_PUBLISHER_IMAGE'
"
```

如需加载 runtime、runner、extension API：

```bash
ssh "$REMOTE" "
  set -euo pipefail
  docker pull '$RUNTIME_CONTROLLER_IMAGE'
  docker pull '$RUNNER_IMAGE'
  docker pull '$EXTENSION_API_IMAGE'
  /root/go/bin/kind load docker-image --name '$KIND_CLUSTER' '$RUNTIME_CONTROLLER_IMAGE'
  /root/go/bin/kind load docker-image --name '$KIND_CLUSTER' '$RUNNER_IMAGE'
  /root/go/bin/kind load docker-image --name '$KIND_CLUSTER' '$EXTENSION_API_IMAGE'
"
```

## Helm 安装或更新

### 只更新 FrontendExtension controller、packager Job 和 publisher Job

适用于集群已经安装 `frontend-forge`，只需要切换 FE controller、packager Job 和 publisher Job 镜像：

```bash
helm upgrade --install "$RELEASE" config/charts/frontend-forge \
  --namespace "$NAMESPACE" \
  --create-namespace \
  --set crds.installJsBundle=true \
  --set extensionController.enabled=true \
  --set extensionController.image.registry="" \
  --set extensionController.image.repository="$EXTENSION_CONTROLLER_IMAGE" \
  --set extensionController.image.tag="" \
  --set extensionController.packagerImage="$EXTENSION_PACKAGER_IMAGE" \
  --set extensionController.publisherImage="$EXTENSION_PUBLISHER_IMAGE" \
  --set extensionPackager.image.registry="" \
  --set extensionPackager.image.repository="$EXTENSION_PACKAGER_IMAGE" \
  --set extensionPackager.image.tag="" \
  --set extensionPublisher.image.registry="" \
  --set extensionPublisher.image.repository="$EXTENSION_PUBLISHER_IMAGE" \
  --set extensionPublisher.image.tag=""
```

验证 rollout：

```bash
kubectl -n "$NAMESPACE" rollout status deploy/frontend-forge-extension-controller --timeout=180s
kubectl -n "$NAMESPACE" get deploy frontend-forge-extension-controller \
  -o jsonpath='{.spec.template.spec.containers[0].image}{"\n"}{.spec.template.spec.containers[0].env[?(@.name=="PACKAGER_IMAGE")].value}{"\n"}{.spec.template.spec.containers[0].env[?(@.name=="PUBLISHER_IMAGE")].value}{"\n"}'
```

也可以直接用 `kubectl` 快速更新现有部署：

```bash
kubectl -n "$NAMESPACE" set image deploy/frontend-forge-extension-controller \
  frontend-extension-controller="$EXTENSION_CONTROLLER_IMAGE"

kubectl -n "$NAMESPACE" set env deploy/frontend-forge-extension-controller \
  PACKAGER_IMAGE="$EXTENSION_PACKAGER_IMAGE" \
  PUBLISHER_IMAGE="$EXTENSION_PUBLISHER_IMAGE"

kubectl -n "$NAMESPACE" rollout status deploy/frontend-forge-extension-controller --timeout=180s
```

### 完整安装 runtime、runner、FE controller、packager、extension API

适用于干净集群或希望把所有组件都切到本地构建镜像：

```bash
helm upgrade --install "$RELEASE" config/charts/frontend-forge \
  --namespace "$NAMESPACE" \
  --create-namespace \
  --set crds.installJsBundle=true \
  --set image.registry="" \
  --set image.repository="$RUNTIME_CONTROLLER_IMAGE" \
  --set image.tag="" \
  --set runner.image.registry="" \
  --set runner.image.repository="$RUNNER_IMAGE" \
  --set runner.image.tag="" \
  --set extensionController.enabled=true \
  --set extensionController.image.registry="" \
  --set extensionController.image.repository="$EXTENSION_CONTROLLER_IMAGE" \
  --set extensionController.image.tag="" \
  --set extensionController.packagerImage="$EXTENSION_PACKAGER_IMAGE" \
  --set extensionController.publisherImage="$EXTENSION_PUBLISHER_IMAGE" \
  --set extensionPackager.image.registry="" \
  --set extensionPackager.image.repository="$EXTENSION_PACKAGER_IMAGE" \
  --set extensionPackager.image.tag="" \
  --set extensionPublisher.image.registry="" \
  --set extensionPublisher.image.repository="$EXTENSION_PUBLISHER_IMAGE" \
  --set extensionPublisher.image.tag="" \
  --set extensionApi.enabled=true \
  --set extensionApi.image.registry="" \
  --set extensionApi.image.repository="$EXTENSION_API_IMAGE" \
  --set extensionApi.image.tag=""
```

如果要用 chart 内置 build-service，还需要设置 build-service 镜像：

```bash
--set buildService.enabled=true \
--set buildService.image.registry="" \
--set buildService.image.repository="$FRONTEND_FORGE_IMAGE" \
--set buildService.image.tag=""
```

## Extension API 访问方式

完整回归脚本默认访问本地：

```bash
http://127.0.0.1:18080/apis/frontend-forge.kubesphere.io/v1alpha1/frontendextensions
```

如果使用集群内 extension API，可以 port-forward：

```bash
kubectl -n "$NAMESPACE" port-forward svc/frontend-forge-extension-api \
  --address 127.0.0.1 18080:80
```

如果使用本地 debug API：

```bash
EXTENSION_API_BIND_ADDR=127.0.0.1:18080 \
KUBECONFIG="$KUBECONFIG_PATH" \
RUST_LOG=info,frontend_forge_extension_api=debug \
target/debug/frontend-forge-extension-api
```

## 完整回归测试

`scripts/fe-full-regression.sh` 会执行：

1. 如果 `FrontendExtension/inspecttask` 已存在，删除 CR。
2. 验证关联 Job 和 ConfigMap 已删除。
3. 使用 `config/samples/frontendextension-inspecttask.yaml` 创建 CR。
4. 等待 `status.phase=Ready` 且 `status.packageJob.phase=Succeeded`。
5. 校验 packager Job 镜像。
6. 校验 artifact ConfigMap 和 HTTP 下载 sha256。
7. 默认生成修改版 CR 并 apply。
8. 验证修改后 source hash、Job、ConfigMap、digest 都发生变化，并再次下载校验。

当前脚本覆盖 package/download，不触发 publish。publish 需要单独准备 publish target 和调用 publish API。

执行：

```bash
KUBECONFIG_PATH="$KUBECONFIG_PATH" \
FRONTEND_FORGE_NAMESPACE="$NAMESPACE" \
SAMPLE_FILE="$(pwd)/config/samples/frontendextension-inspecttask.yaml" \
EXPECTED_PACKAGER_IMAGE="$EXTENSION_PACKAGER_IMAGE" \
DOWNLOAD_API_BASE_URL="http://127.0.0.1:18080/apis/frontend-forge.kubesphere.io/v1alpha1/frontendextensions" \
scripts/fe-full-regression.sh
```

只测创建，不测修改：

```bash
RUN_UPDATE_TEST=false scripts/fe-full-regression.sh
```

成功后 artifacts 默认在：

```bash
artifacts/fe-full-regression/<timestamp>
```

关键文件：

- `pre-delete/`：删除前现场
- `delete-existing.log`：CR 删除记录
- `create-frontendextension.yaml`
- `create-package-job.yaml`
- `create-artifact-configmap.yaml`
- `create-download-sha256.txt`
- `frontendextension-update.yaml`
- `update-frontendextension.yaml`
- `update-package-job.yaml`
- `update-artifact-configmap.yaml`
- `update-download-sha256.txt`
- `update-summary.txt`

## Publish Job 手动测试

### 准备 publish target

`FrontendExtension` 示例默认 publish target 是：

```yaml
publishPolicy:
  mode: Manual
  defaultTargetRef:
    namespace: extension-frontend-forge
    name: ksbuilder-publish-config
```

publisher Job 支持 `ConfigMap` 或 `Secret` target。target 数据处理规则：

- `env.<NAME>`：注入为 `ksbuilder publish` 进程环境变量 `<NAME>`。
- `args`：按空白字符切分后追加到 `ksbuilder publish <artifact>` 参数后面。
- 其他 key：写入工作目录下的 `.frontend-forge-publish-target/<key>` 文件。

用于 smoke test 时，可以先创建一个 ConfigMap。下面只是占位示例，真实 key 需要按当前 `ksbuilder publish` 使用的 registry 配置调整：

```bash
kubectl -n "$NAMESPACE" create configmap "$PUBLISH_TARGET_NAME" \
  --from-literal=env.REGISTRY=docker.io \
  --from-literal=env.REPOSITORY=spike2044/inspecttask \
  --dry-run=client -o yaml | kubectl apply -f -
```

实际发布所需 key 取决于当前 `ksbuilder publish` 版本和 registry 配置要求。凭据类数据应使用 `Secret`，并在 API 请求里指定 `targetKind: Secret`。

### 触发 publish

先确保 package 已 Ready：

```bash
kubectl get frontendextension inspecttask \
  -o jsonpath='{.status.phase}{"\n"}{.status.artifact.digest}{"\n"}'
```

通过 extension API 触发 publish：

```bash
artifact_digest="$(kubectl get frontendextension inspecttask -o jsonpath='{.status.artifact.digest}')"
request_id="manual-$(date +%Y%m%d%H%M%S)"

curl -fsS -X POST \
  "http://127.0.0.1:18080/apis/frontend-forge.kubesphere.io/v1alpha1/frontendextensions/inspecttask/publish" \
  -H 'Content-Type: application/json' \
  -d "{
    \"requestId\": \"${request_id}\",
    \"artifactDigest\": \"${artifact_digest}\",
    \"targetKind\": \"${PUBLISH_TARGET_KIND}\",
    \"targetRef\": {
      \"namespace\": \"${PUBLISH_TARGET_NAMESPACE}\",
      \"name\": \"${PUBLISH_TARGET_NAME}\"
    }
  }"
```

查询 publish 状态：

```bash
curl -fsS \
  "http://127.0.0.1:18080/apis/frontend-forge.kubesphere.io/v1alpha1/frontendextensions/inspecttask/publish"

kubectl get frontendextension inspecttask \
  -o jsonpath='{.status.publish.phase}{"\n"}{.status.publish.jobRef.name}{"\n"}{.status.publish.lastError}{"\n"}'
```

### 查看 publisher Job

```bash
publish_job="$(kubectl get frontendextension inspecttask -o jsonpath='{.status.publish.jobRef.name}')"

kubectl -n "$NAMESPACE" get job "$publish_job" -o yaml
kubectl -n "$NAMESPACE" logs "job/$publish_job"
```

验证 publisher Job 镜像：

```bash
kubectl -n "$NAMESPACE" get job "$publish_job" \
  -o jsonpath='{.spec.template.spec.containers[0].image}{"\n"}'
```

publisher Job 的关键环境变量由 controller 注入：

- `PUBLISH_REQUEST_ID`
- `ARTIFACT_DIGEST`
- `ARTIFACT_CONFIGMAP_NAMESPACE`
- `ARTIFACT_CONFIGMAP_NAME`
- `ARTIFACT_CONFIGMAP_KEY`
- `ARTIFACT_FILENAME`
- `PUBLISH_TARGET_KIND`
- `PUBLISH_TARGET_NAMESPACE`
- `PUBLISH_TARGET_NAME`

publisher binary 支持 `KSBUILDER_PUBLISH_ARGS` 环境变量，但当前 controller 没有把它注入 publisher Job。如需传额外参数，优先在 publish target 里使用 `args` key；如果要从 controller 统一注入，则需要扩展 controller 的 publisher Job env。

## 常用排查命令

查看当前镜像：

```bash
kubectl -n "$NAMESPACE" get deploy frontend-forge-extension-controller \
  -o jsonpath='{.spec.template.spec.containers[0].image}{"\n"}{.spec.template.spec.containers[0].env[?(@.name=="PACKAGER_IMAGE")].value}{"\n"}{.spec.template.spec.containers[0].env[?(@.name=="PUBLISHER_IMAGE")].value}{"\n"}'
```

查看 FE 状态：

```bash
kubectl get frontendextension inspecttask -o yaml
```

查看 package Job：

```bash
job_name="$(kubectl get frontendextension inspecttask -o jsonpath='{.status.packageJob.name}')"
kubectl -n "$NAMESPACE" get job "$job_name" -o yaml
kubectl -n "$NAMESPACE" logs "job/$job_name"
```

查看 publish Job：

```bash
publish_job="$(kubectl get frontendextension inspecttask -o jsonpath='{.status.publish.jobRef.name}')"
kubectl -n "$NAMESPACE" get job "$publish_job" -o yaml
kubectl -n "$NAMESPACE" logs "job/$publish_job"
```

查看 artifact ConfigMap：

```bash
cm_name="$(kubectl get frontendextension inspecttask -o jsonpath='{.status.artifact.storage.ref.name}')"
kubectl -n "$NAMESPACE" get cm "$cm_name" -o yaml
```

下载产物并校验：

```bash
curl -fL "http://127.0.0.1:18080/apis/frontend-forge.kubesphere.io/v1alpha1/frontendextensions/inspecttask/download" \
  -o /tmp/inspecttask.tgz
shasum -a 256 /tmp/inspecttask.tgz
```
