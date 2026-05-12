# Frontend Forge

Frontend Forge 为 KubeSphere 前端扩展提供 Kubernetes controller 和 Job。它覆盖
`FrontendExtension` 的 package/download/publish/unpublish 流程，以及
`FrontendIntegration` 将当前集群前端入口构建为 `JSBundle` 的 runtime 流程。

默认 Helm 安装主线是 `FrontendExtension`：生成 extension artifact，通过
extension API 暴露下载，并可选通过 publisher Job 触发 publish。旧的 FI runtime
controller 仍已实现，但默认关闭；已有 FI 对象会通过默认 Helm hook 迁移为 FE。

## 功能范围

| 流程 | 状态 | 默认 Helm 行为 |
| --- | --- | --- |
| `FrontendExtension` package/download/publish/unpublish | 已实现 | 启用 |
| `FrontendIntegration` runtime build to `JSBundle` | 已实现 | 通过 `controller.enabled=false` 关闭 |
| FI to FE `migrator` | 已实现 | 通过 `migration.fiToFe.enabled=true` 启用 |
| FI admission webhook | 已实现 | 通过 `webhook.enabled=false` 关闭 |
| 本地/e2e build-service stub | 本地/e2e 已实现 | 通过 `buildService.enabled=false` 关闭 |

核心对象：

| Kind | Scope | 用途 |
| --- | --- | --- |
| `FrontendExtension` / `FE` | Cluster | package source、artifact 状态、download 和 publish 状态。 |
| `FrontendIntegration` / `FI` | Cluster | 当前集群 `JSBundle` runtime 构建源。 |
| `JSBundle` | Cluster | KubeSphere extension runtime 消费的前端 bundle。 |

extension API 提供 FE list、get、create、download、publish、unpublish 和 delete 操作。路由级行为见
[`spec/frontend-extension-design.md`](spec/frontend-extension-design.md)。

## 默认 Helm 行为

默认值来自 [`config/charts/frontend-forge/values.yaml`](config/charts/frontend-forge/values.yaml)：

| Value | Default | 影响 |
| --- | --- | --- |
| `extensionController.enabled` | `true` | 安装 FE package/publish controller。 |
| `extensionApi.enabled` | `true` | 安装 FE HTTP API Deployment 和 Service。 |
| `migration.fiToFe.enabled` | `true` | 安装/升级后运行 FI-to-FE migration hook。 |
| `controller.enabled` | `false` | 不安装 FI runtime controller。 |
| `webhook.enabled` | `false` | 不安装 FI validating webhook。 |
| `crds.installJsBundle` | `false` | 不安装外部依赖 `JSBundle` CRD。 |
| `buildService.enabled` | `false` | 不安装本地/e2e build-service stub。 |

FE packaging 和 FI runtime build 都会调用 `BUILD_SERVICE_BASE_URL`。在 chart
默认值下，即使 `buildService.enabled=false`，该 URL 仍会被派生；运行这些流程时，
需要在该地址提供服务、显式配置外部 URL，或在本地/e2e 中启用 stub。

## 快速安装

默认安装：

```bash
helm upgrade --install frontend-forge config/charts/frontend-forge \
  --namespace extension-frontend-forge \
  --create-namespace
```

当集群未提供 `JSBundle` CRD 时，安装本地/e2e CRD：

```bash
helm upgrade --install frontend-forge config/charts/frontend-forge \
  --namespace extension-frontend-forge \
  --create-namespace \
  --set crds.installJsBundle=true
```

为本地/e2e 测试启用 FI runtime：

```bash
helm upgrade --install frontend-forge config/charts/frontend-forge \
  --namespace extension-frontend-forge \
  --create-namespace \
  --set controller.enabled=true \
  --set crds.installJsBundle=true \
  --set buildService.enabled=true
```

## KubeSphere Extension 安装

KubeSphere extension wrapper 位于 `config/frontend-forge`。chart 源文件位于
`config/frontend-forge/charts/frontend-forge`，这样 `ksbuilder` 可以直接打包；
`config/charts/frontend-forge` 作为兼容 symlink 保留给 Helm 直装和现有脚本使用。

使用 `ksbuilder` 打包或发布 extension：

```bash
ksbuilder package config/frontend-forge
ksbuilder publish config/frontend-forge
```

通过 KubeSphere 配置 extension 时，chart values 需要按 dependency name 包一层：

```yaml
frontend-forge:
  extensionController:
    enabled: true
  extensionApi:
    enabled: true
```

如果直接使用 Helm 安装，继续使用 `config/charts/frontend-forge`。

## 基本使用

创建 `FrontendExtension` 示例：

```bash
kubectl apply -f config/samples/frontendextension-inspecttask.yaml
kubectl get frontendextensions.frontend-forge.kubesphere.io inspecttask
```

在启用 FI runtime 后创建 `FrontendIntegration` 示例：

```bash
kubectl apply -f config/samples/frontend-forge_v1alpha1_frontendintegration.yaml
kubectl get frontendintegrations.frontend-forge.kubesphere.io demo-fi
```

其他示例：

| 文件 | 用途 |
| --- | --- |
| `config/samples/fi-crdtable.yaml` | FI `crdTable` 页面示例。 |
| `config/samples/fi-nested-menu-demo.yaml` | FI 两级菜单示例。 |
| `config/samples/fi-lifecycle-smoke.yaml` | FI lifecycle smoke test 示例。 |

## 仓库结构

| 路径 | 职责 |
| --- | --- |
| `crates/api` | Rust CRD 类型和 CRD 生成入口。 |
| `crates/frontend-extension-controller` | FE package/publish/unpublish reconcile。 |
| `crates/frontend-forge-controller` | FI runtime controller、FI webhook、FI-to-FE migrator。 |
| `crates/frontend-forge-extension-api` | FE list/get/create/download/publish/unpublish/delete HTTP API。 |
| `config/frontend-forge` | 用于 `ksbuilder package/publish` 的 KubeSphere extension wrapper 和被打包的 Helm chart。 |
| `config/charts/frontend-forge` | 指向 Helm chart 的兼容 symlink，供 Helm 直装和脚本使用。 |
| `config/samples` | FI/FE 示例 manifest。 |
| `spec` | 与 Rust 类型、controller、route、chart 默认值对应的实现说明。 |
| `skills/frontend-forge-fi-operations` | 仓库内置 FI 操作 Codex skill。 |

更细的 crate 和 Job 行为见 `spec/`。

## 开发

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo xtask gen-crd
```

构建主要 binary：

```bash
cargo build --release -p frontend-forge-controller
cargo build --release -p frontend-forge-runner
cargo build --release -p frontend-extension-controller
cargo build --release -p frontend-forge-extension-api
cargo build --release -p frontend-forge-extension-packager
cargo build --release -p frontend-forge-extension-publisher
```

安装 git hooks：

```bash
lefthook install
```

注册仓库内置 Codex skill：

```bash
mkdir -p "${CODEX_HOME:-$HOME/.codex}/skills"
ln -s "$(pwd)/skills/frontend-forge-fi-operations" \
  "${CODEX_HOME:-$HOME/.codex}/skills/frontend-forge-fi-operations"
```

## 延伸阅读

| 主题 | 文档 |
| --- | --- |
| CRD 字段和状态 | [`spec/crds.md`](spec/crds.md) |
| Manifest renderer | [`spec/Manifest.md`](spec/Manifest.md) |
| FI runtime controller、runner、webhook | [`spec/fi-runtime.md`](spec/fi-runtime.md) |
| FE package/publish/API 行为 | [`spec/frontend-extension-design.md`](spec/frontend-extension-design.md) |
| Helm values 和模板条件 | [`spec/helm-chart.md`](spec/helm-chart.md) |
| Kubernetes 资源 | [`spec/k8s-resources.md`](spec/k8s-resources.md) |
| FI-to-FE migration | [`spec/fi-to-fe-migration.md`](spec/fi-to-fe-migration.md) |
| 手动构建镜像和安装 | [`spec/manual-build-and-helm-install.md`](spec/manual-build-and-helm-install.md) |
| Kubernetes 版本矩阵 | [`spec/k8s-matrix-plan.md`](spec/k8s-matrix-plan.md) |

`spec/` 中的实现细节会对照 Rust API 类型、controller 行为、Helm 默认值和 HTTP
routes。

## TODO / Open Questions

- 生产环境 build-service 的部署归属未在本仓库定义；当前文档只覆盖 chart stub 和外部
  URL 配置。
- 具体 `ksbuilder publish` 凭据字段依赖所选 `ksbuilder` 和 registry 配置。publisher
  只定义 target data 如何传递给进程。
