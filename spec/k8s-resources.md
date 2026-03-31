# Kubernetes 资源清单

本文基于仓库最新 `main` 分支扫描以下范围汇总：

- 运行时代码：`crates/api`、`crates/controller`、`crates/runner`、`crates/manifest`
- 交付清单：`config/**`
- 调试脚本：`scripts/dev-webhook.sh`
- 样例与设计文档：`config/samples/**`、`README.md`、`spec/**`

口径说明：

- “使用到的资源”分成三类：
  - 运行时直接读写、监听、创建、校验的资源
  - 仓库部署清单会直接安装或依赖的资源
- 不单列 `Pod`、`AdmissionReview` 这类嵌入式/协议级对象。
  - 本项目会通过 `Deployment` / `Job` 模板间接生成 Pod，但不会直接用 Pod API 管理它们。

## 1. 运行时直接使用的资源

| Group / Version | Kind | 作用域 | 使用方式 | 说明 |
| --- | --- | --- | --- | --- |
| `frontend-forge.kubesphere.io/v1alpha1` | `FrontendIntegration` | Cluster | controller `watch/get/patch/patch_status`，runner `get/patch_status`，webhook `validate` | 项目的主入口 CR。Rust 类型定义在 `crates/api/src/lib.rs`，controller 和 runner 都直接操作它。 |
| `extensions.kubesphere.io/v1alpha1` | `JSBundle` | Cluster | controller `get/patch/patch_status`，runner `create-or-apply/patch/patch_status` | 构建产物 CR。仓库只声明 Rust 类型，不生成这个 CRD；`crates/api/examples/print_crds.rs` 明确说明它是第三方 CRD。 |
| `v1` | `ConfigMap` | Namespaced | runner `get/create/patch/update` | runner 把构建出的前端 bundle 落到 ConfigMap，再让 `JSBundle.spec.rawFrom.configMapKeyRef` 指向它。 |
| `batch/v1` | `Job` | Namespaced | controller `create/get/list/watch` | controller 为每次构建创建 runner Job，并根据 Job 状态驱动 `FrontendIntegration.status`。 |
| `admissionregistration.k8s.io/v1` | `ValidatingWebhookConfiguration` | Cluster | certgen Job `get/update/patch`，调试脚本 `get/patch` | 可选 webhook 开启后依赖该资源把 `FrontendIntegration` 的校验前移到 admission 阶段。 |
| `core/v1` | `Secret` | Namespaced | certgen Job `get/create/patch/update`，controller Deployment `mount` | webhook TLS 证书存放在 `frontend-forge-controller-webhook-tls` Secret 中。 |
| `apiextensions.k8s.io/v1` | `CustomResourceDefinition` | Cluster | `xtask` 生成，安装清单应用 | 仓库当前只生成并交付 `FrontendIntegration` 的 CRD。 |

补充说明：

- `FrontendIntegration` 的类型与 CRD 生成逻辑在 `crates/api/src/lib.rs`。
- controller 的主要 runtime 入口在 `crates/controller/src/main.rs`，其中直接创建 `Api::<FrontendIntegration>`、`Api::<Job>`、`Api::<JSBundle>`。
- runner 的主要 runtime 入口在 `crates/runner/src/main.rs`，其中直接创建 `Api::<FrontendIntegration>`、`Api::<ConfigMap>`、`Api::<JSBundle>`。
- webhook 的 HTTP 校验逻辑在 `crates/controller/src/webhook.rs`，校验对象是 `AdmissionReview<FrontendIntegration>`。

## 2. 仓库部署清单直接安装的资源

这些资源不会都被 Rust 代码直接操作，但仓库交付物会直接创建或依赖它们。

| Group / Version | Kind | 主要来源 | 用途 |
| --- | --- | --- | --- |
| `v1` | `Namespace` | `config/manager/controller-deployment.yaml` | 安装命名空间 `extension-frontend-forge`。 |
| `apps/v1` | `Deployment` | `config/manager/controller-deployment.yaml` | 部署 controller 进程，并可选承载 webhook。 |
| `v1` | `Service` | `config/webhook/controller-webhook.yaml` | 为 webhook 暴露集群内访问入口。 |
| `v1` | `ServiceAccount` | `config/rbac/controller-rbac.yaml`、`config/rbac/runner-rbac.yaml`、`config/rbac/webhook-certgen-rbac.yaml` | 分别供 controller、runner、certgen Job 使用。 |
| `rbac.authorization.k8s.io/v1` | `ClusterRole` | `config/rbac/controller-rbac.yaml`、`config/rbac/runner-rbac.yaml`、`config/rbac/webhook-certgen-rbac.yaml` | 赋予 cluster-scoped 资源访问权限。 |
| `rbac.authorization.k8s.io/v1` | `ClusterRoleBinding` | 同上 | 绑定 cluster-scoped 权限到对应 ServiceAccount。 |
| `rbac.authorization.k8s.io/v1` | `Role` | `config/rbac/runner-rbac.yaml`、`config/rbac/webhook-certgen-rbac.yaml` | 赋予 namespaced 资源权限，如 `ConfigMap`、`Secret`。 |
| `rbac.authorization.k8s.io/v1` | `RoleBinding` | 同上 | 绑定 namespaced 权限到对应 ServiceAccount。 |
| `batch/v1` | `Job` | `config/rbac/webhook-certgen-rbac.yaml` | 两个一次性 certgen Job，分别负责创建 TLS Secret 和回填 `caBundle`。 |
| `admissionregistration.k8s.io/v1` | `ValidatingWebhookConfiguration` | `config/webhook/controller-webhook.yaml` | 注册 webhook 到 apiserver。 |
| `apiextensions.k8s.io/v1` | `CustomResourceDefinition` | `config/crd/bases/frontend-forge.kubesphere.io_frontendintegrations.yaml` | 安装 `FrontendIntegration` CRD。 |

## 3. 只在权限或 schema 中出现的资源点

这部分值得单独记下来，因为它们容易被误解成“项目已经在直接使用”。

| Group / Version | Kind | 出现位置 | 当前状态 |
| --- | --- | --- | --- |
| `core/v1` | `Event` | `config/rbac/controller-rbac.yaml` | controller ClusterRole 预留了 `create/patch/update` 权限，但当前代码里没有发现事件记录实现。 |
| `core/v1` | `Secret` | `crates/api/src/lib.rs` 中 `JSBundle.spec.rawFrom.secretKeyRef` | `JSBundle` schema 允许 Secret 作为内容来源，但当前 runner 实际只写 `configMapKeyRef`。 |

## 4. 当前资源面结论

- 项目真正的控制面核心资源只有 3 个：`FrontendIntegration`、`Job`、`JSBundle`。
- runner 的产物落盘资源是 `ConfigMap`，它通过 `JSBundle` 间接暴露给前端系统。
- webhook 相关资源是可选交付面：`Service`、`Secret`、`ValidatingWebhookConfiguration`、certgen `Job` 及其 RBAC。
