# FrontendExtension 发布态能力设计草案

## 1. 目标与边界

`FrontendExtension` 是 frontend-forge v2 的发布态主业务对象，用来表达“一个前端扩展如何被打包成可发布产物”。它不承担当前集群 runtime 安装职责，也不直接创建 `JSBundle`。

核心目标：

- 为前端扩展生成完整发布内容，包括 extension charts config、install-time `JSBundle` 声明、`RoleTemplate` 等资源声明。
- 将生成后的 package artifact 存入内部存储，当前建议使用 ConfigMap。
- 对前端提供 HTTP API，用于列表、详情、下载和可选 publish 操作。
- 支持系统触发 `ksbuilder publish` job，但不在当前阶段实现 install controller。
- 在同一 repo 内拆分模块和 binary，保持 runtime controller 与 package/publish controller 职责隔离。

明确边界：

- `FrontendExtension` apply 后不应让当前集群立即生效。
- `FrontendExtension` 不应复用 runtime `FrontendIntegration` 的资源生命周期语义。
- `FrontendExtension.spec.source` 是发布态 source，不嵌入完整 `FrontendIntegration` 对象。
- publish 和 install 都属于外部功能；当前只设计可选 publish job，不设计 install controller。

## 2. 总体架构

frontend-forge 建议划分为三层：

### 2.1 Runtime 层

runtime 层继续由现有模型承担：

```text
FrontendIntegration -> build Job -> bundle ConfigMap -> JSBundle
```

职责：

- 监听 `FrontendIntegration`。
- 按 `spec` 和 hash 创建 runner Job。
- runner 渲染 manifest、调用 build-service、写 bundle ConfigMap。
- runner/controller 创建和同步当前集群的 `JSBundle`。
- `FrontendIntegration.status` 表示当前集群 runtime 构建与 `JSBundle` 可用状态。

该层强调“apply 后在当前集群收敛生效”。

### 2.2 Publish / Package 层

发布态层新增 `FrontendExtension`：

```text
FrontendExtension -> package Job/worker -> artifact ConfigMap -> HTTP download API
                                      \
                                       -> optional publish Job -> ksbuilder publish
```

职责：

- 监听 `FrontendExtension`。
- 校验发布态 source。
- 生成 extension package artifact。
- 将 artifact 写入内部存储。
- 更新 `FrontendExtension.status` 中的 artifact、download、packageJob、publish 状态。
- 通过 HTTP API 对外提供列表、详情、下载和 publish 触发能力。

该层强调“生成发布产物”，不是“当前集群立即安装”。

### 2.3 Shared Render Core

现有 `frontend-forge-manifest` 已承载 `FrontendIntegration` 到 manifest 的渲染逻辑，但当前函数签名直接依赖 `FrontendIntegration`。v2 建议把可共享部分抽成中间模型：

```text
FrontendIntegration --------\
                             -> FrontendRenderInput -> render core -> ExtensionManifest
FrontendExtension.source ---/
```

建议模型：

- `FrontendSourceSpec`：菜单、页面、locales、schemaVersion 等发布/运行时都能共享的 source schema。
- `FrontendRenderInput`：渲染所需的归一化输入，包括 name、displayName、description、schemaVersion、menus、pages、locales。
- runtime adapter：`FrontendIntegration -> FrontendRenderInput`。
- package adapter：`FrontendExtension.spec.source -> FrontendRenderInput`。

共享的是 schema、校验和渲染逻辑，不共享 runtime controller 的 Job、ConfigMap、`JSBundle` 生命周期。

## 3. FrontendExtension CRD 草案

### 3.1 基本信息

建议 CRD：

```yaml
apiVersion: apiextensions.k8s.io/v1
kind: CustomResourceDefinition
metadata:
  name: frontendextensions.frontend-forge.kubesphere.io
spec:
  group: frontend-forge.kubesphere.io
  scope: Cluster
  names:
    plural: frontendextensions
    singular: frontendextension
    kind: FrontendExtension
    shortNames:
      - fe
  versions:
    - name: v1alpha1
      served: true
      storage: true
      subresources:
        status: {}
```

`FrontendExtension` 建议保持 cluster-scoped，原因是 extension package 通常面向整个平台发布，且产物、publish 凭据、extension metadata 不天然属于某个业务 namespace。实际 package/publish Job 和 artifact ConfigMap 可以放到 controller 的工作 namespace，例如 `extension-frontend-forge`。

### 3.2 示例对象

```yaml
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendExtension
metadata:
  name: inspecttask
spec:
  package:
    name: inspecttask
    version: 0.1.0
    displayName:
      zh: 巡检任务
      en: Inspect Task
    description:
      zh: InspectTask extension package
      en: InspectTask extension package
    category: dev-tools
    keywords:
      - Frontend
    sources:
      - https://github.com/kubesphere-extensions/frontend-forge
    kubeVersion: ">=1.23.0-0"
    ksVersion: ">=4.2.1-0"
    maintainers:
      - name: KubeSphere
        email: kubesphere@yunify.com
    home: https://kubesphere.com.cn/
    provider:
      zh:
        name: 北京青云科技股份有限公司
        email: kubesphere@yunify.com
        url: https://kubesphere.com.cn/
      en:
        name: QingCloud Technologies
        email: kubesphere@yunify.com
        url: https://kubesphere.co/
    icon: ./static/frontend-forge.ico
    dependencies:
      - name: frontend
        tags:
          - extension
      - name: frontend-forge
        tags:
          - extension
    installationMode: HostOnly
    images:
      - kubesphere/frontend-forge-console:v1.0.0
      - kubesphere/frontend-forge-controller:v1.0.0
      - kubesphere/frontend-forge-runner:v1.0.0
    charts:
      values: {}

  source:
    type: Inline
    inline:
      schemaVersion: v1
      frontend:
        locales:
          zh:
            title: 巡检任务
          en:
            title: Inspect Tasks
        menus:
          - displayName: Inspect Tasks
            key: inspecttasks
            placement: cluster
            type: page
        pages:
          - key: inspecttasks
            type: crdTable
            crdTable:
              group: kubeeye.kubesphere.io
              version: v1alpha2
              scope: Cluster
              names:
                kind: InspectTask
                plural: inspecttasks
              columns:
                - key: name
                  title: NAME
                  render:
                    type: text
                    path: metadata.name

      extensionResources:
        jsBundle:
          name: inspecttask
        roleTemplates:
          - name: inspecttask-view
            displayName: InspectTask Viewer
            rules:
              - apiGroups: ["kubeeye.kubesphere.io"]
                resources: ["inspecttasks"]
                verbs: ["get", "list", "watch"]

  publishPolicy:
    mode: Manual
    defaultTargetRef:
      namespace: extension-frontend-forge
      name: ksbuilder-publish-config
```

### 3.3 Spec 字段建议

顶层结构：

```yaml
spec:
  package: {}
  source: {}
  publishPolicy: {}
```

字段说明：

| 字段 | 说明 |
| --- | --- |
| `spec.package.name` | package 名称。生成 `extension.yaml.name`，并作为发布包内 frontend manifest 的 name/route 命名空间。可选，缺省时使用 `metadata.name`。 |
| `spec.package.version` | extension package 版本。建议后续按 SemVer 校验。 |
| `spec.package.displayName` | package 多语言展示名称，对应真实 `ksbuilder extension.yaml.displayName`。 |
| `spec.package.description` | package 多语言描述，对应真实 `ksbuilder extension.yaml.description`。 |
| `spec.package.category` | extension 分类，例如 `dev-tools`。 |
| `spec.package.keywords` | extension 关键词列表。 |
| `spec.package.sources` | extension 源码或项目地址列表。 |
| `spec.package.kubeVersion` | Kubernetes 版本约束，原样写入 `extension.yaml.kubeVersion`。 |
| `spec.package.ksVersion` | KubeSphere 版本约束，原样写入 `extension.yaml.ksVersion`。 |
| `spec.package.maintainers` | extension 维护者列表。 |
| `spec.package.home` | extension 主页。 |
| `spec.package.provider` | provider 多语言信息。 |
| `spec.package.icon` | extension icon 路径。 |
| `spec.package.dependencies` | ksbuilder extension dependencies。 |
| `spec.package.installationMode` | 安装模式，例如 `HostOnly`。 |
| `spec.package.images` | extension 相关镜像列表。 |
| `spec.package.charts.values` | 生成 extension chart config 时使用的 values。 |
| `spec.source.type` | source 类型。当前先支持 `Inline`，未来可扩展 `Git`、`ConfigMap`、`OCI`。 |
| `spec.source.inline.schemaVersion` | 发布态 source schema 版本，不使用 runtime `builder.engineVersion` 命名。 |
| `spec.source.inline.frontend` | 前端菜单、页面、locales 等渲染输入。 |
| `spec.source.inline.extensionResources` | install-time Kubernetes 资源声明，包括 `jsBundle` 和 `roleTemplates`。 |
| `spec.publishPolicy.mode` | publish 策略。当前建议 `Manual`，未来可扩展 `Auto`、`Disabled`。 |
| `spec.publishPolicy.defaultTargetRef` | 默认 publish target 配置引用，通常指向包含 registry/ksbuilder 配置的 Secret 或 ConfigMap。 |

`spec.source.inline.frontend` 与 `FrontendIntegration.spec` 可以共享菜单、页面、locales 的 schema，但不应完整复用 FI spec：

- 不包含 `enabled`，因为发布态没有当前集群启停语义。
- 不包含 runtime build 状态相关字段。
- 使用 `schemaVersion` 表达 source schema，不使用 `builder.engineVersion` 表达 runner 行为。

## 4. Status 设计

`status.phase` 只表示 source 校验和 package artifact 生成流程。publish 状态单独放在 `status.publish`，避免 publish 失败影响 artifact 是否 Ready 的判断。

```yaml
status:
  phase: Ready
  observedGeneration: 3
  observedSourceHash: sha256:aabbcc...

  artifact:
    storage:
      kind: ConfigMap
      ref:
        namespace: extension-frontend-forge
        name: fe-inspecttask-a1b2c3d4
        uid: 11111111-2222-3333-4444-555555555555
      key: package.tgz

    digest: sha256:ddee1122...
    sizeBytes: 23142
    mediaType: application/gzip
    filename: inspecttask-0.1.0.tgz
    generatedAt: "2026-04-20T10:00:00Z"
    sourceHash: sha256:aabbcc...

  download:
    ready: true
    filename: inspecttask-0.1.0.tgz
    mediaType: application/gzip

  packageJob:
    namespace: extension-frontend-forge
    name: fe-inspecttask-package-a1b2c3d4
    uid: 22222222-3333-4444-5555-666666666666
    phase: Succeeded
    startedAt: "2026-04-20T09:59:20Z"
    finishedAt: "2026-04-20T10:00:00Z"
    message: ""

  publish:
    phase: NotRequested
    requestId: ""
    artifactDigest: ""
    jobRef:
      namespace: ""
      name: ""
      uid: ""
    startedAt: ""
    finishedAt: ""
    lastError: ""

  conditions:
    - type: SourceValid
      status: "True"
      reason: Validated
      message: ""
      observedGeneration: 3
      lastTransitionTime: "2026-04-20T09:59:00Z"

    - type: ArtifactReady
      status: "True"
      reason: Generated
      message: ""
      observedGeneration: 3
      lastTransitionTime: "2026-04-20T10:00:00Z"

    - type: DownloadReady
      status: "True"
      reason: Available
      message: ""
      observedGeneration: 3
      lastTransitionTime: "2026-04-20T10:00:00Z"

    - type: PublishSucceeded
      status: "False"
      reason: NotRequested
      message: ""
      observedGeneration: 3
      lastTransitionTime: "2026-04-20T10:00:00Z"
```

### 4.1 Phase 状态机

```text
           spec 创建 / source 变化
                    |
                    v
              +-----------+
              |  Pending  |
              +-----+-----+
          校验失败  |  校验通过 / 创建 package job
              |     v
              |  +-----------+
              |  | Packaging |
              |  +-----+-----+
              |  job失败 | job成功 + artifact ready
              v         v
          +--------+  +-------+
          | Failed |  | Ready |
          +--------+  +-------+
              |
              +-- spec 修复或 source 变化后回到 Pending
```

`status.phase` 枚举语义：

| phase | 含义 |
| --- | --- |
| `Pending` | source 变化后等待处理，或正在做前置校验，尚未进入实际打包。 |
| `Packaging` | package job 已创建并正在执行，或正在等待 artifact 落盘。 |
| `Ready` | artifact 已就绪，可下载。 |
| `Failed` | source 校验失败，或 package job 失败。 |

### 4.2 Package Job 状态

`status.packageJob.phase` 建议使用以下枚举：

| phase | 含义 |
| --- | --- |
| `Pending` | Job 已创建但 Pod 尚未开始实际执行。 |
| `Running` | Job 有 active Pod。 |
| `Succeeded` | Job 成功完成，且 controller 已观察到结果。 |
| `Failed` | Job 失败，或超过 deadline/backoff。 |

### 4.3 Publish 状态

`status.publish.phase` 与 `status.phase` 分离：

| phase | 含义 |
| --- | --- |
| `NotRequested` | 从未请求 publish，或当前 artifact 尚未触发 publish。 |
| `Pending` | publish 请求已接受，Job 尚未运行。 |
| `Running` | publish Job 正在执行 `ksbuilder publish`。 |
| `Succeeded` | 最近一次 publish 成功。 |
| `Failed` | 最近一次 publish 失败。 |

publish 状态必须记录 `artifactDigest`，用于说明这次 publish 针对哪一个 package artifact。source 变化重新打包后，旧 publish 结果不应被误认为新 artifact 的发布结果。

## 5. Conditions 设计

建议 conditions：

| type | 含义 | `False` 常见 reason |
| --- | --- | --- |
| `SourceValid` | source 已通过 schema/semantic 校验。 | `InvalidSchema` / `UnsupportedSchemaVersion` |
| `ArtifactReady` | artifact 已成功生成。 | `PackageFailed` / `JobNotFound` / `ArtifactMissing` |
| `DownloadReady` | artifact 已可被下载 API 提供。 | `ArtifactNotReady` / `StorageUnavailable` / `DigestMismatch` |
| `PublishSucceeded` | 最近一次 publish 是否成功。 | `NotRequested` / `PublishFailed` / `ArtifactExpired` |

conditions 更新原则：

- `observedGeneration` 必须与当前 controller 实际处理的 generation 对齐。
- source 变化后，应先把 `ArtifactReady=False`、`DownloadReady=False`，直到新 artifact 准备好。
- publish 失败不应把 `ArtifactReady` 改成 `False`，除非失败原因证明 artifact 本身不可用或 digest 不匹配。

## 6. 关键判定规则

### 6.1 Artifact 是否过期

```text
if status.artifact.sourceHash != status.observedSourceHash:
    artifact 已过期，需要重新打包
```

更严格的 Ready 判断还应确认：

- `status.phase == Ready`
- `ArtifactReady=True`
- `DownloadReady=True`
- `status.artifact.sourceHash == status.observedSourceHash`
- `status.artifact.digest` 非空
- `status.artifact.storage.ref.name` 和 `status.artifact.storage.key` 非空

### 6.2 Publish 请求是否命中过期产物

```text
if request.artifactDigest != status.artifact.digest:
    返回 409 Conflict
```

该规则避免用户在 UI 上看到旧 artifact 后，source 已变化并生成新 artifact 时，仍把旧 artifact 发布出去。

### 6.3 Source Hash 计算

`observedSourceHash` 应基于归一化后的发布态 source 计算，建议包含：

- `spec.package.version`
- `spec.package.charts`
- `spec.source`

不建议包含：

- `spec.publishPolicy`，因为 publish target 改变不应触发重新打包。
- metadata 中与产物无关的 labels/annotations。

### 6.4 Artifact Digest 计算

`artifact.digest` 应基于最终 `package.tgz` 字节内容计算，而不是基于 source hash 直接复用。这样可以区分“同一 source 生成产物内容是否稳定”和“实际下载文件是否被篡改”。

## 7. ConfigMap / Job / HTTP API 关系

### 7.1 Artifact ConfigMap

当前阶段建议把 artifact 存入 ConfigMap：

```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  namespace: extension-frontend-forge
  name: fe-inspecttask-a1b2c3d4
  labels:
    frontend-forge.io/managed-by: frontend-extension-controller
    frontend-forge.io/frontend-extension-name: inspecttask
    frontend-forge.io/source-hash: aabbcc...
  annotations:
    frontend-forge.io/artifact-digest: sha256:ddee1122...
    frontend-forge.io/source-hash: sha256:aabbcc...
binaryData:
  package.tgz: <base64>
data:
  artifact.json: |
    {
      "filename": "inspecttask-0.1.0.tgz",
      "digest": "sha256:ddee1122...",
      "mediaType": "application/gzip"
    }
```

建议内容：

- `binaryData["package.tgz"]`：最终可下载和可 publish 的 package。
- `data["artifact.json"]`：artifact 元信息，包括 digest、size、filename、sourceHash。
- 可选 `data["files.json"]`：调试用途的文件清单。不要让 UI 依赖它作为稳定 API。

package 内部建议包含：

- extension charts config。
- install-time `JSBundle` YAML 声明。
- install-time `RoleTemplate` YAML 声明。
- rendered frontend manifest。
- package metadata。

### 7.2 Package Job

package Job 的职责：

- 读取指定 `FrontendExtension`。
- 校验 `spec.source.inline.schemaVersion`。
- 将 `spec.source.inline.frontend` 转成 `FrontendRenderInput`。
- 调用 shared render core 生成 frontend manifest。
- 生成 extension charts config、`JSBundle` 声明、`RoleTemplate` 声明。
- 打包为 `package.tgz`。
- 写入 artifact ConfigMap。

package Job 不应创建 `JSBundle`，也不应调用 install 逻辑。

### 7.3 Publish Job

publish Job 的职责：

- 读取 `FrontendExtension.status.artifact` 指向的 package。
- 校验请求中的 `artifactDigest` 与当前 status digest 一致。
- 解包或挂载 package。
- 使用 `publishPolicy.defaultTargetRef` 或请求中的 targetRef 获取 publish 配置。
- 调用 `ksbuilder publish`。
- 将结果反馈给 controller，由 controller 更新 `status.publish`。

publish Job 必须是独立 Job，不允许由 controller、API server 或 package worker 进程内直接执行 `ksbuilder publish`。隔离要求：

- controller/API/packager 不直接依赖 ksbuilder crate、二进制路径或本地工作目录结构。
- `ksbuilder publish` 只存在于 `frontend-extension-publisher` Job 的 container image 中。
- publish Job 使用独立临时工作目录解包 artifact，禁止复用 controller/packager 的源码目录或 cargo workspace。
- ksbuilder 版本通过 publisher image tag 或 Job env 明确固定；升级 ksbuilder 只需要重建/切换 publisher image。
- publish Job 的 ServiceAccount、Secret/ConfigMap targetRef、网络权限和资源限制单独配置，不继承 controller 的权限。
- ksbuilder 输出、缓存和临时文件只写入 Job 容器文件系统或专用 emptyDir，不写回本地项目目录。

这样 ksbuilder 的依赖、CLI 行为、缓存格式或版本变化不会影响 frontend-forge 本地项目、controller 进程和 package 产物生成链路。

publish Job 不应安装 extension，也不应在当前集群创建 install-time 资源。

### 7.4 HTTP Download API

下载 API 的职责：

- 读取 `FrontendExtension`。
- 检查 `status.phase`、`ArtifactReady`、`DownloadReady`、sourceHash 和 digest。
- 读取 `status.artifact.storage` 指向的 ConfigMap。
- 校验 `package.tgz` digest。
- 返回 package 文件。

前端不应该直接读 ConfigMap，原因：

- ConfigMap 是内部存储实现，不是稳定产品 API。
- 直接读 ConfigMap 会扩大前端所需 Kubernetes RBAC。
- ConfigMap 暴露了内部 namespace、key、label、annotation、hash 命名规则。
- HTTP API 更容易做鉴权、审计、缓存、错误码、digest 校验和向后兼容。
- 未来 artifact 存储迁移到 Secret、PVC、OCI registry 或对象存储时，HTTP API 可以保持不变。

## 8. 异步流程设计

完整流程：

1. 用户通过 kubectl、GitOps 或 HTTP API 创建 `FrontendExtension`。
2. extension-controller watch 到对象，计算归一化 source hash。
3. controller 校验 source schema 和基本语义。
4. 校验失败时，更新 `status.phase=Failed`，设置 `SourceValid=False`。
5. 校验成功且 artifact 不存在或已过期时，创建 package Job。
6. controller 更新 `status.phase=Packaging` 和 `status.packageJob`。
7. package worker 读取 `FrontendExtension`，生成 frontend manifest。
8. package worker 生成 extension charts config、install-time `JSBundle`、`RoleTemplate` 等资源声明。
9. package worker 生成 `package.tgz`，计算 digest 和 size。
10. package worker 写入 artifact ConfigMap。
11. controller 观察 package Job 成功，并读取/校验 artifact ConfigMap。
12. controller 更新 `status.artifact`、`status.download`、conditions，设置 `status.phase=Ready`。
13. 前端通过 HTTP API 查询列表和详情。
14. 前端通过 HTTP download API 下载 package。
15. 用户可选触发 publish API。
16. extension-controller 创建 publish Job。
17. publish Job 调用 `ksbuilder publish`。
18. controller 根据 publish Job 结果更新 `status.publish` 和 `PublishSucceeded` condition。

失败处理：

- source 校验失败：不创建 package Job，`phase=Failed`。
- package Job 失败：`phase=Failed`，`ArtifactReady=False`，保留上一份未过期 artifact 的策略需要后续单独定义；v1alpha1 建议以当前 source 为准。
- artifact ConfigMap 缺失：`ArtifactReady=False`，`DownloadReady=False`。
- download digest 校验失败：HTTP 返回 `500` 或 `409`，并让 controller 后续把 `DownloadReady=False`。
- publish digest 不匹配：HTTP 返回 `409 Conflict`，不创建 publish Job。

## 9. Controller 与模块设计建议

继续放在 `frontend-forge-controller` 仓库内，但拆分 crate 和 binary。不要把发布态能力揉进现有 runtime controller。

推荐目录结构：

```text
crates/
  api/
    # FrontendIntegration + FrontendExtension CRD Rust types
  common/
    # hash、naming、labels、ResourceRef、Condition 等公共工具
  manifest/
    # shared render core，逐步从 FrontendIntegration 入参迁移到 RenderInput 入参
  extension-package-core/
    # package 文件模型、chart config、JSBundle 声明、RoleTemplate 声明渲染
  frontend-forge-controller/
    # 现有 FrontendIntegration runtime controller，保持职责不扩张
  frontend-forge-runner/
    # 现有 runtime build runner
  frontend-extension-controller/
    # FrontendExtension reconciler，负责 package/publish 状态收敛
  extension-packager/
    # package Job binary
  extension-publisher/
    # ksbuilder publish Job binary
  frontend-forge-extension-api/
    # axum HTTP API：list/get/download/publish

config/
  crd/bases/
    frontend-forge.kubesphere.io_frontendextensions.yaml
  rbac/
    extension-controller-rbac.yaml
    extension-packager-rbac.yaml
    extension-publisher-rbac.yaml
    extension-api-rbac.yaml
  manager/
    extension-controller-deployment.yaml
    extension-api-deployment.yaml
```

binary 建议：

- `frontend-forge-controller`：只保留现有 `FrontendIntegration` runtime reconciler 和可选 FI admission webhook。
- `frontend-forge-runner`：只保留现有 runtime build runner。
- `frontend-extension-controller`：新增，watch `FrontendExtension`、Job、artifact ConfigMap。
- `frontend-extension-packager`：新增，作为 package Job 执行。
- `frontend-extension-publisher`：新增，作为独立 publish Job 执行，是唯一允许调用 `ksbuilder publish` 的 binary/container。
- `frontend-extension-api`：新增，对前端提供 HTTP API。

packager 和 publisher 可以暂时使用同一个镜像承载多个 binary，但推荐尽快拆成不同 image 或至少不同 image tag。无论镜像如何组织，`ksbuilder` 都不应进入 controller/API 镜像的运行依赖；运行时 Deployment 和 Job command 必须清晰区分。

## 10. HTTP API 设计建议

HTTP API 是产品 API，不应泄漏 artifact ConfigMap 的内部结构。

建议路径：

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| `POST` | `/apis/frontend-forge.kubesphere.io/v1alpha1/frontendextensions` | 创建 `FrontendExtension`。适合 UI 表单创建，也可仅支持 Kubernetes API 创建。 |
| `GET` | `/apis/frontend-forge.kubesphere.io/v1alpha1/frontendextensions` | 列表，返回 metadata、package、phase、download ready、publish phase。 |
| `GET` | `/apis/frontend-forge.kubesphere.io/v1alpha1/frontendextensions/{name}` | 查询详情。 |
| `GET` | `/apis/frontend-forge.kubesphere.io/v1alpha1/frontendextensions/{name}/download` | 下载当前 Ready artifact。 |
| `POST` | `/apis/frontend-forge.kubesphere.io/v1alpha1/frontendextensions/{name}/publish` | 触发 publish Job。 |
| `GET` | `/apis/frontend-forge.kubesphere.io/v1alpha1/frontendextensions/{name}/publish` | 查询最近一次 publish 状态。 |

列表响应示例：

```json
{
  "items": [
    {
      "name": "inspecttask",
      "generation": 3,
      "package": {
        "version": "0.1.0",
        "displayName": {
          "zh": "巡检任务",
          "en": "Inspect Task"
        }
      },
      "phase": "Ready",
      "artifactDigest": "sha256:ddee1122...",
      "download": {
        "ready": true,
        "filename": "inspecttask-0.1.0.tgz"
      },
      "publish": {
        "phase": "NotRequested"
      }
    }
  ]
}
```

publish 请求示例：

```json
{
  "requestId": "20260420-100000",
  "artifactDigest": "sha256:ddee1122...",
  "targetRef": {
    "namespace": "extension-frontend-forge",
    "name": "ksbuilder-publish-config"
  }
}
```

publish API 行为：

- `artifactDigest` 必填。
- 当 `artifactDigest != status.artifact.digest` 时返回 `409 Conflict`。
- 当 artifact 未 Ready 时返回 `409 Conflict`。
- 当已有同一 `requestId` 的 publish Job 时返回当前状态，保证幂等。
- targetRef 缺省时使用 `spec.publishPolicy.defaultTargetRef`。

下载 API 行为：

- artifact 未 Ready 时返回 `409 Conflict` 或 `404 Not Found`，建议用 `409` 表示对象存在但当前不可下载。
- ConfigMap 缺失或 key 缺失时返回 `500`，同时由 controller 后续收敛 `DownloadReady=False`。
- digest 校验失败时拒绝返回内容。
- 响应设置 `Content-Type: application/gzip` 和 `Content-Disposition: attachment; filename="inspecttask-0.1.0.tgz"`。

## 11. 与 FrontendIntegration 的边界

`FrontendIntegration` 继续承担：

- 当前集群 runtime 生效入口。
- runtime build Job 调度。
- runtime bundle ConfigMap 写入。
- 当前集群 `JSBundle` 创建、状态同步、启停。
- `enabled` 语义。

`FrontendExtension` 负责：

- 发布态 source 定义。
- package artifact 生成。
- artifact download。
- 可选 publish job。
- package/publish 状态展示。

两者共享：

- frontend 菜单 schema。
- page schema。
- locales schema。
- frontend manifest render core。
- 语义校验规则中与 source 本身有关的部分。

两者不共享：

- controller reconcile 状态机。
- Job 类型和环境变量。
- ConfigMap 内容结构。
- `JSBundle` 创建职责。
- status phase 语义。

不能把 `FrontendExtension` 做成 `FrontendIntegration` 升级版，原因：

- FI 是 runtime intent，FE 是 package source。
- FI 的 apply 会触发当前集群收敛，FE 的 apply 只生成发布产物。
- FI 的成功条件是 `JSBundle` ready，FE 的成功条件是 artifact ready 和 download ready。
- FI 的 `enabled` 是 runtime 开关，FE 不应有当前集群启停语义。
- publish 需要凭据、target、digest 防过期和审计，属于不同控制面。
- `ksbuilder publish` 的依赖和行为变化必须被隔离在独立 publish Job 中，不能影响 runtime controller、本地 workspace 或 package 生成流程。

## 12. 命名与职责边界

### FrontendIntegration

适合表达“将前端能力集成到当前集群 runtime 中”。它和 `JSBundle` 的关系紧密，强调安装后的运行态。

### FrontendExtension

适合表达“一个可发布前端扩展的源定义”。该对象是 v2 发布态能力的主业务对象，原因：

- 名称与 extension package、extension charts、`ksbuilder publish` 的产品语义一致。
- 不暗示当前集群立即 runtime 集成。
- 可以自然承载 package metadata、source、extensionResources、publishPolicy。
- 可以作为 API 列表、详情、下载和 publish 的统一入口。

### ExtensionPackage

适合表达生成后的不可变产物。当前阶段不建议立即新增 `ExtensionPackage` CRD，因为 `FrontendExtension.status.artifact + ConfigMap` 已能满足单版本 artifact 下载和 publish。

未来如果需要多版本保留、签名、回滚、审计、跨集群同步或对象存储索引，可以引入：

```text
FrontendExtension -> ExtensionPackage -> ConfigMap/ObjectStorage/OCI
```

在当前阶段，`ExtensionPackage` 更适合作为内部 package artifact 模型，而不是用户直接操作的主 CRD。

## 13. 实施顺序建议

建议按以下顺序落地：

1. 在 `crates/api` 新增 `FrontendExtension` 类型和 CRD 生成。
2. 在 `crates/manifest` 抽象 `FrontendRenderInput`，让 FI 和 FE 都通过 adapter 复用渲染核心。
3. 新增 `crates/extension-package-core`，定义 package 文件模型和资源声明渲染。
4. 新增 `extension-packager` binary，先支持 `Inline` source 和 ConfigMap artifact。
5. 新增 `extension-controller` binary，完成 `Pending -> Packaging -> Ready/Failed` 状态机。
6. 新增 `extension-api` binary，提供列表、详情、下载 API。
7. 新增 publish API 和 `extension-publisher` binary，支持手动 `ksbuilder publish`。

每一步都应避免改变现有 `FrontendIntegration` runtime 行为。v2 发布态能力可以作为独立 Deployment 安装，确保回归风险可控。
