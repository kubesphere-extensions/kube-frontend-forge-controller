# Helm Chart 交付资源

本文记录 frontend-forge 当前的 Kubernetes 交付面。仓库不再维护逐个 `kubectl apply` 的安装清单；安装入口统一为 Helm chart：

```text
config/charts/frontend-forge
```

示例 CR 仍保留在 `config/samples/`，用于开发、文档和 e2e。

Helm chart 的设计细节见 [`helm-chart.md`](helm-chart.md)。

## 1. 安装方式

默认安装 runtime 与发布态组件：

```bash
helm upgrade --install frontend-forge config/charts/frontend-forge \
  --namespace extension-frontend-forge \
  --create-namespace
```

本地或 e2e 集群如果没有 KubeSphere 提供的 `JSBundle` CRD，可以显式开启：

```bash
helm upgrade --install frontend-forge config/charts/frontend-forge \
  --namespace extension-frontend-forge \
  --create-namespace \
  --set crds.installJsBundle=true
```

启用 admission webhook：

```bash
helm upgrade --install frontend-forge config/charts/frontend-forge \
  --namespace extension-frontend-forge \
  --create-namespace \
  --set webhook.enabled=true
```

## 2. Chart 结构

```text
config/charts/frontend-forge/
  Chart.yaml
  values.yaml
  values-e2e.yaml
  crds/
    frontend-forge.kubesphere.io_frontendintegrations.yaml
    frontend-forge.kubesphere.io_frontendextensions.yaml
  templates/
    _helpers.tpl
    frontend-forge-controller-deployment.yaml
    frontend-extension-controller-deployment.yaml
    frontend-forge-extension-api-deployment.yaml
    rbac-runtime.yaml
    rbac-extension.yaml
    serviceaccounts.yaml
    webhook.yaml
    webhook-certgen.yaml
    jsbundle-crd.yaml
    build-service.yaml
```

## 3. Chart 管理的资源

| 资源 | 默认 | 来源 | 说明 |
| --- | --- | --- | --- |
| `FrontendIntegration` CRD | 是 | `crds/` | FI runtime 主 CRD。 |
| `FrontendExtension` CRD | 是 | `crds/` | FE package/publish 主 CRD。 |
| `JSBundle` CRD | 否 | `templates/jsbundle-crd.yaml` | KubeSphere 通常外部提供；本地/e2e 用 `crds.installJsBundle=true` 开启。 |
| `Deployment/frontend-forge-controller` | 是 | `templates/frontend-forge-controller-deployment.yaml` | FI runtime controller 和可选 webhook server。 |
| `Deployment/frontend-extension-controller` | 是 | `templates/frontend-extension-controller-deployment.yaml` | FE package/publish controller。 |
| `Deployment/frontend-forge-extension-api` | 是 | `templates/frontend-forge-extension-api-deployment.yaml` | 前端访问 FE 列表、详情、下载、publish 的 HTTP API。 |
| `Service/frontend-forge-extension-api` | 是 | 同上 | 暴露 extension API。 |
| Runtime RBAC | 是 | `templates/rbac-runtime.yaml` | controller 与 runner 所需权限。 |
| Extension RBAC | 是 | `templates/rbac-extension.yaml` | FE controller、packager、publisher、extension API 所需权限。 |
| Webhook Service / VWC | 否 | `templates/webhook.yaml` | `webhook.enabled=true` 时安装。 |
| Webhook certgen RBAC / Job | 否 | `templates/webhook-certgen.yaml` | `webhook.enabled=true` 时安装。 |
| Dev/e2e build-service | 否 | `templates/build-service.yaml` | `buildService.enabled=true` 时安装。 |

## 4. 运行时直接使用的资源

| Group / Version | Kind | 作用域 | 使用方 | 说明 |
| --- | --- | --- | --- | --- |
| `frontend-forge.kubesphere.io/v1alpha1` | `FrontendIntegration` | Cluster | `frontend-forge-controller`、`frontend-forge-runner`、FI webhook | runtime 生效入口。 |
| `frontend-forge.kubesphere.io/v1alpha1` | `FrontendExtension` | Cluster | `frontend-extension-controller`、`frontend-forge-extension-api`、packager Job | package/publish 发布态入口。 |
| `extensions.kubesphere.io/v1alpha1` | `JSBundle` | Cluster | FI controller、runner | 安装后 runtime 前端 bundle CR。 |
| `batch/v1` | `Job` | Namespaced | FI controller、FE controller | runtime build、package、publish 都通过独立 Job 执行。 |
| `v1` | `ConfigMap` | Namespaced | runner、packager、extension API | runtime bundle 或 FE package artifact 存储。 |
| `v1` | `Secret` | Namespaced | webhook certgen、publisher Job | webhook TLS；publish target 凭据。 |
| `admissionregistration.k8s.io/v1` | `ValidatingWebhookConfiguration` | Cluster | webhook certgen、apiserver | 可选 FI admission 校验。 |

## 5. 关键 values

| values 路径 | 默认 | 说明 |
| --- | --- | --- |
| `crds.installJsBundle` | `false` | 是否安装本地/e2e 用 `JSBundle` CRD。 |
| `image.repository` | `kubesphere/frontend-forge-controller` | FI runtime controller 镜像。 |
| `runner.image.repository` | `kubesphere/frontend-forge-runner` | FI build Job 使用的 runner 镜像。 |
| `controller.buildServiceBaseUrl` | `http://<release-name>.<release-namespace>.svc` | runner 调用的外部 build-service；空值时由 chart 按 release namespace 派生。 |
| `extensionController.enabled` | `true` | 是否安装 FE package/publish controller。 |
| `extensionPackager.image.repository` | `kubesphere/frontend-forge-extension-packager` | package Job 镜像。 |
| `extensionPublisher.image.repository` | `kubesphere/frontend-forge-extension-publisher` | 独立 `ksbuilder publish` Job 镜像。 |
| `extensionApi.enabled` | `true` | 是否安装 FE HTTP API。 |
| `webhook.enabled` | `false` | 是否安装并启用 FI admission webhook。 |
| `buildService.enabled` | `false` | 是否安装本地/e2e build-service stub。 |

## 6. 设计约束

- Chart 是唯一安装入口，文档和 CI 不再要求用户逐个 `kubectl apply` 安装 manager/RBAC/webhook 清单。
- `crds/` 下的 FI/FE CRD 由 `cargo xtask gen-crd` 生成并提交。
- `JSBundle` CRD 默认视为外部依赖；只有本地/e2e 场景才由 chart 条件安装。
- `ksbuilder publish` 只允许在 `frontend-forge-extension-publisher` Job 中执行，不能进入 controller 或 API Deployment。
