# Helm Chart 设计

frontend-forge 的 Kubernetes 交付入口统一为：

```text
config/charts/frontend-forge
```

不再维护 `config/manager`、`config/rbac`、`config/webhook`、`config/e2e`、`config/crd/bases` 这类逐文件 `kubectl apply` 安装路径。

## 1. Chart 职责

chart 负责安装：

- `FrontendIntegration` CRD
- `FrontendExtension` CRD
- `frontend-forge-controller` Deployment 和 runtime RBAC
- `frontend-forge-runner` ServiceAccount / RBAC，供 FI build Job 使用
- `frontend-extension-controller` Deployment 和 FE controller RBAC
- `frontend-forge-extension-packager` ServiceAccount / RBAC，供 package Job 使用
- `frontend-forge-extension-publisher` ServiceAccount / RBAC，供独立 publish Job 使用
- `frontend-forge-extension-api` Deployment / Service / RBAC
- 可选 FI admission webhook
- 可选 webhook certgen Job
- 可选本地/e2e `JSBundle` CRD
- 可选本地/e2e build-service stub

## 2. CRD 放置规则

FI/FE 是本项目主 CRD，放在 Helm 标准 `crds/` 目录：

```text
config/charts/frontend-forge/crds/
  frontend-forge.kubesphere.io_frontendintegrations.yaml
  frontend-forge.kubesphere.io_frontendextensions.yaml
```

`cargo xtask gen-crd` 直接写入上述路径。CI 和 git hook 也只检查 chart 内 CRD。

`JSBundle` 是 KubeSphere extension 系统资源，默认不安装。仅当本地或 e2e 集群缺少该 CRD 时，通过以下 values 开启：

```yaml
crds:
  installJsBundle: true
```

该 CRD 放在 `templates/jsbundle-crd.yaml`，因为它是条件资源，不适合放在 Helm `crds/` 目录。

## 3. 常用安装命令

默认安装：

```bash
helm upgrade --install frontend-forge config/charts/frontend-forge \
  --namespace extension-frontend-forge \
  --create-namespace
```

启用本地/e2e `JSBundle` CRD：

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

## 4. values 分组

| values 分组 | 职责 |
| --- | --- |
| `frontendForgeController` | FI runtime controller、runner Job 镜像、build-service 地址、runtime Job 参数。 |
| `frontendForgeRunner` | runner ServiceAccount。 |
| `frontendExtensionController` | FE package/publish controller、packager/publisher Job 镜像、artifact namespace。 |
| `frontendForgeExtensionPackager` | package Job ServiceAccount。 |
| `frontendForgeExtensionPublisher` | publish Job ServiceAccount。 |
| `frontendForgeExtensionApi` | FE HTTP API Deployment / Service。 |
| `webhook` | FI admission webhook、certgen 和 webhook TLS Secret。 |
| `buildService` | 本地/e2e build-service stub。生产环境通常关闭。 |
| `crds` | 条件 CRD，例如本地/e2e `JSBundle` CRD。 |

## 5. Namespace 派生

chart 默认使用 Helm release namespace：

- `JSBUNDLE_CONFIGMAP_NAMESPACE`
- `ARTIFACT_CONFIGMAP_NAMESPACE`
- 默认 build-service URL：`http://frontend-forge.<release-namespace>.svc`

如果必须跨 namespace 存储 artifact 或访问外部 build-service，可通过 values 显式覆盖。

## 6. Webhook 行为

`webhook.enabled=false` 是默认值。

开启后 chart 会安装：

- webhook Service
- `ValidatingWebhookConfiguration`
- certgen ServiceAccount / RBAC
- certgen create Job
- certgen patch Job

certgen Job 使用 Helm hook：

- `post-install`
- `post-upgrade`
- `before-hook-creation`
- `hook-succeeded`

这样避免 Helm upgrade 时因为同名 completed Job 的 immutable 字段导致升级失败。

## 7. e2e 行为

`scripts/ci-kind-e2e.sh` 不再逐个 apply 安装清单，而是生成临时 values 文件后执行：

```bash
helm upgrade --install frontend-forge config/charts/frontend-forge ...
```

e2e 会开启：

- `crds.installJsBundle=true`
- `buildService.enabled=true`
- `frontendExtensionController.enabled=false`
- `frontendForgeExtensionApi.enabled=false`

FI 生命周期样例本身仍通过 `kubectl apply -f` 创建和修改，这是测试业务对象，不属于安装交付路径。
