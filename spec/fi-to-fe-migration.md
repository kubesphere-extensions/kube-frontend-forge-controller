# FI 到 FE 迁移 Job

本文记录 `FrontendIntegration` 到 `FrontendExtension` 的自动迁移行为。迁移入口是 Helm chart 渲染的 `fi-to-fe-migrator` Job，对应二进制为：

```text
/usr/local/bin/fi-to-fe-migrator
```

## 1. 目标

新版本停用 FI runtime controller，使用 FE package/publish 链路承载原 FI 功能。迁移 Job 负责把现有 cluster-scoped `FrontendIntegration` 资源转换为 cluster-scoped `FrontendExtension` 资源，并在原 FI 启用时触发 FE publish。

迁移流程是：

```text
扫描所有 FI
  -> 为每个 FI 创建或更新 migrator 管理的 FE
  -> 等 FE package Ready
  -> 删除原 FI 并确认不存在
  -> 如果原 FI spec.enabled 缺省或为 true，通过 direct fe-api 触发 publish
```

单个 FI 失败不会阻断后续 FI。全部扫描完成后，只要存在失败项，Job 会以非零状态退出，失败 Job 会保留日志。

## 2. 资源作用域

FI 和 FE 都是 cluster-scoped CRD：

- `frontendintegrations.frontend-forge.kubesphere.io`
- `frontendextensions.frontend-forge.kubesphere.io`

因此 migrator 使用 cluster-scoped Kubernetes API 操作 FI/FE。package artifact、package Job、publish Job 和 publish target 是 namespaced 资源，默认 namespace 来自 Helm release namespace。

## 3. FE 命名

FE 名称固定由 FI 名称派生：

```text
fi-<fi.metadata.name>
```

不会去重已有 `fi-` 前缀：

```text
foo    -> fi-foo
fi-foo -> fi-fi-foo
```

如果组合后的名称超过 DNS label 63 字符限制，使用稳定的 slice-hash 形式：

```text
fi-<slice>-<12-char-sha256>
```

## 4. 管理标记

migrator 创建或更新的 FE 必须带管理标记：

```yaml
metadata:
  labels:
    frontend-forge.io/managed-by: frontend-forge-fi-migrator
  annotations:
    frontend-forge.io/source-fi-name: <fi name>
    frontend-forge.io/source-fi-uid: <fi uid>
```

如果目标 FE 已存在但没有 `frontend-forge.io/managed-by=frontend-forge-fi-migrator`，migrator 会跳过该 FI 并记录失败，避免覆盖用户已有 FE。

如果目标 FE 是 migrator 管理的，但 `source-fi-name` 指向其他 FI，也会记录失败。

## 5. 字段映射

FI 到 FE 的主要字段映射如下：

| FI | FE |
| --- | --- |
| `metadata.name` | `metadata.name=fi-<fi name>` |
| `metadata.annotations["kubesphere.io/description"]` | `spec.package.description.en` |
| `metadata.annotations["kubesphere.io/creator"]` | `spec.package.provider.en.name`、`spec.package.provider.zh.name` |
| `spec.displayName` | `spec.package.displayName.en`、`spec.source.inline.frontend.displayName` |
| `spec.locales` | `spec.source.inline.frontend.locales` |
| `spec.menus` | `spec.source.inline.frontend.menus` |
| `spec.pages` | `spec.source.inline.frontend.pages` |
| `spec.builder.engineVersion` | `spec.source.inline.schemaVersion` |
| `spec.enabled` | 仅用于决定是否 publish，不写入 FE spec |

默认值：

```yaml
spec:
  package:
    version: "0.1.0"
    icon: ./static/favicon.svg
    category: dev-tools
    provider:
      en:
        name: <FI creator or "Fi Migration Bot">
      zh:
        name: <FI creator or "Fi Migration Bot">
```

其中：

- `package.version` 来自 Helm `migration.fiToFe.packageVersion`。
- `displayName` 缺省时回退到 FI 名称。
- `description` 缺省时回退到 displayName。
- `schemaVersion` 缺省时回退到 Helm `migration.fiToFe.schemaVersion`，默认 `v1`。
- `provider.*.name` 优先使用 FI annotation `kubesphere.io/creator`，为空时使用 `Fi Migration Bot`。

FE publish policy 会写入为手动发布：

```yaml
spec:
  publishPolicy:
    mode: Manual
    defaultTargetKind: ConfigMap
    defaultTargetRef:
      namespace: <migration.fiToFe.publishTarget.namespace or release namespace>
      name: ksbuilder-publish-config
```

## 6. Publish 行为

migrator 不创建 publish Job，也不 patch publish annotations。publish 统一通过 FE API 触发：

```http
POST {FE_API_BASE_URL}/apis/frontend-forge-api.kubesphere.io/v1alpha1/frontendextensions/{feName}/publish
```

请求体：

```json
{
  "requestId": "fi-migration-<feName>-<digest-prefix>",
  "expectedArtifactDigest": "<current FE artifact digest>"
}
```

默认 `FE_API_BASE_URL` 指向 chart 内 Service：

```text
http://<release-name>-extension-api.<release-namespace>.svc:<extensionApi.service.port>
```

该调用不经过 ks-apiserver，也不需要 KubeSphere token。

TLS 配置：

- `FE_API_INSECURE_SKIP_TLS_VERIFY=true` 时跳过 TLS 校验。
- `FE_API_CA_CERT_PATH` 为空时不加载自定义 CA。
- `FE_API_CA_CERT_PATH` 显式设置且文件不存在时，migrator 会启动失败并保留错误日志。

## 7. Helm 接口

默认 values：

```yaml
controller:
  enabled: false

migration:
  fiToFe:
    enabled: true
    packageVersion: "0.1.0"
    schemaVersion: v1
    readyTimeoutSeconds: 600
    pollIntervalSeconds: 5
    backoffLimit: 0
    hookDeletePolicy: before-hook-creation
    activeDeadlineSeconds: null
    feApiBaseUrl: ""
    feApiInsecureSkipTlsVerify: false
    feApiCaCertPath: ""
    publishTarget:
      kind: ConfigMap
      namespace: ""
      name: ksbuilder-publish-config
```

`hookDeletePolicy` 默认只包含 `before-hook-creation`，不包含 `hook-failed`，这样失败 Job 会保留日志用于排障。

`backoffLimit` 默认是 `0`。migrator 本身大部分步骤是幂等的，但当前流程在删除 FI 后才触发 publish。如果 Job 级重试发生在 FI 删除成功但 publish 失败之后，重试时扫描不到原 FI，不能保证补 publish。因此默认不依赖 Job 级重试隐藏失败。

## 8. 前置条件

migrator 启动后会等待：

- FI CRD Established。
- FE CRD Established，并且 `v1alpha1` 提供 status subresource。

direct fe-api Service 的可用性由 publish 请求本身验证。Helm 默认同时启用：

```yaml
extensionApi:
  enabled: true

extensionController:
  enabled: true
```

## 9. 删除 FI 与 finalizer

migrator 在 FE package Ready 后删除原 FI，并轮询确认 FI 已不存在。如果历史 FI 带有 finalizer，而负责清理 finalizer 的旧 controller 已关闭，删除可能卡住直到 `readyTimeoutSeconds` 超时。

当前实现不会无条件移除 FI finalizers。这样可以避免绕过未知 controller 的清理语义。若集群中存在历史 finalizer，需要先确认 finalizer 的来源和清理策略，再决定是否引入显式的、可审计的 finalizer 清理逻辑。

## 10. 验证

常用本地验证命令：

```bash
cargo test -p frontend-forge-controller --bin fi-to-fe-migrator
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
helm template frontend-forge config/charts/frontend-forge
```

本地调试 direct fe-api：

```bash
kubectl -n extension-frontend-forge port-forward svc/frontend-forge-extension-api 18080:80

KUBECONFIG=/path/to/kubeconfig \
FE_API_BASE_URL=http://127.0.0.1:18080 \
PUBLISH_TARGET_NAMESPACE=extension-frontend-forge \
cargo run -p frontend-forge-controller --bin fi-to-fe-migrator
```
