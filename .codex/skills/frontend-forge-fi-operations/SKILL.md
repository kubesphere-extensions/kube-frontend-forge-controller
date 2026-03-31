---
name: frontend-forge-fi-operations
description: 处理 FrontendIntegration（FI）的创建、修改、启用/禁用、删除，以及状态检查和排障。
---

# Frontend Forge FI Operations

## 何时使用

- 创建或更新 `FrontendIntegration`
- 启用、禁用或删除 FI
- 查看 FI 状态和最近一次构建
- 追踪关联资源：Job、`JSBundle`、ConfigMap
- 排查这些问题：
  - 没有触发 Job
  - 没有生成 `JSBundle`
  - 状态不正确或卡住

## Quick Entry

- 创建或更新 FI -> 看「创建或更新 FI」
- 禁用 FI -> 看「禁用 FI」
- 启用 FI -> 看「启用 FI」
- 删除 FI -> 看「删除 FI」
- 排查问题：
  - 没有 Job -> 看「情况 1」
  - 有 Job 无 Bundle -> 看「情况 2」
  - Bundle 状态异常 -> 看「情况 3」
  - 状态卡住 -> 看「情况 4」

## 前置条件

- 当前集群可通过 `kubectl` 访问
- 以下 CRD 已安装：
  - `frontendintegrations.frontend-forge.kubesphere.io`
  - `jsbundles.extensions.kubesphere.io`
- `frontend-forge` 和 `frontend-forge-controller` 已运行
- 默认 namespace 可能是：
  - `extension-frontend-forge`

## 先读文件

- `references/lifecycle.md`
- `references/inspection.md`
- `config/samples/fi-lifecycle-smoke.yaml`
- `crates/common/src/lib.rs`
- `crates/api/src/lib.rs`

## 资源模型

- `FrontendIntegration`
  - cluster-scoped
  - short name: `fi`
- `JSBundle`
  - cluster-scoped
  - 默认通常是：`fi-<fi-name>`
- ConfigMap
  - 默认 namespace：`extension-frontend-forge`
  - 默认通常是：`<bundle-name>-config`
- Job
  - 默认 namespace：`extension-frontend-forge`
  - 构建时触发

## 默认命名

- bundle: `fi-<fi-name>`
- configmap: `<bundle-name>-config`

这些是当前默认值，不要无条件硬编码。运行时配置变更后，名称和 namespace 可能不同。

## 常用命令

### 查看 FI

```bash
kubectl get fi <name> -o yaml
kubectl get fi <name> -o jsonpath='{.status}'
```

### 创建或更新

```bash
kubectl apply -f <file.yaml>
```

### 禁用

```bash
kubectl patch fi <name> --type=merge -p '{"spec":{"enabled":false}}'
```

### 启用

```bash
kubectl patch fi <name> --type=merge -p '{"spec":{"enabled":true}}'
```

### 删除

```bash
kubectl delete fi <name>
```

### 查看关联资源

```bash
kubectl get jsbundle <bundle-name> -o yaml
kubectl -n extension-frontend-forge get cm <bundle-name>-config -o yaml
kubectl -n extension-frontend-forge get jobs
```

## 推荐工作流

### 1. 创建或更新 FI

1. 应用清单：

   ```bash
   kubectl apply -f <file.yaml>
   ```

2. 查看 FI 状态：
   - `.status.phase`
   - `.status.message`
   - `.status.observed_spec_hash`
   - `.status.last_build`

3. 验证：
   - 已创建 Job
   - `JSBundle` 已存在
   - `JSBundle.status.state = Available`

### 2. 禁用 FI

1. patch：

   ```bash
   kubectl patch fi <name> --type=merge -p '{"spec":{"enabled":false}}'
   ```

2. 预期：
   - `status.message = Disabled`
   - `status.last_build = null`
   - `JSBundle.status.state = Disabled`
   - label `frontend-forge.io/enabled = false`

### 3. 启用 FI

1. patch：

   ```bash
   kubectl patch fi <name> --type=merge -p '{"spec":{"enabled":true}}'
   ```

2. 预期：
   - `JSBundle.status.state = Available`
   - label `frontend-forge.io/enabled = true`
   - `bundle_ref.name` 正确
   - 可能复用已有 bundle，也可能重新触发构建，取决于当前实现和现场状态

### 4. 删除 FI

```bash
kubectl delete fi <name>
```

预期：

- FI 被删除
- `JSBundle` 被删除，或其残留状态可以解释
- ConfigMap 被清理，或其残留状态可以解释

### 5. 查看构建产物

从 `JSBundle.metadata.annotations` 查看：

- `frontend-forge.io/manifest-content`
- `frontend-forge.io/source-spec`
- `frontend-forge.io/source-spec-hash`
- `frontend-forge.io/build-job`

它们可用于：

- 追溯输入 manifest
- 核对来源 spec
- 确认构建来自哪个 Job

## 排障

### 情况 1：没有触发 Job

检查：

1. `kubectl get fi <name> -o yaml`
   - `phase`
   - `message`
2. `spec.enabled` 是否为 `true`
3. controller 是否运行：

   ```bash
   kubectl -n extension-frontend-forge get deploy
   ```

4. controller 日志

### 情况 2：有 Job 但没有 JSBundle

检查：

1. Job 状态：

   ```bash
   kubectl -n extension-frontend-forge get jobs
   ```

2. Job 日志
3. runner 错误
4. RBAC / 权限
5. `observed_spec_hash` 或 spec stale 检查

### 情况 3：JSBundle 存在但状态不对

检查：

1. `JSBundle.status.state`
2. label：
   - `frontend-forge.io/enabled`
3. FI：
   - `bundle_ref`
4. 注解：
   - `source-spec`
   - `manifest-content`

### 情况 4：状态不正确或卡住

检查：

1. FI：
   - `observed_generation`
   - `observed_spec_hash`
2. controller 日志
3. reconcile 行为是否持续推进

## 验收重点

- FI 最终进入预期 phase：
  - `Succeeded`
  - `Building`
  - `Failed`
  - `Pending`
- `JSBundle` 状态与 FI 意图一致
- 注解里的源信息与实际输入一致
- build Job 与当前 spec 一致

## 坑点

- 不要给 FI 加 `-n`；它是 cluster-scoped 资源
- 不要盲信默认命名；若部署改了 `JSBUNDLE_CONFIGMAP_NAMESPACE` 等环境变量，先回查运行时配置
- 不要只看 Job；最终必须同时看：
  - `FI.status`
  - `JSBundle.status`
  - 注解里的源数据
- 如果 admission webhook 没启用，语义错误可能在运行期才暴露，而不是 `kubectl apply` 当场失败

## 升级排查

如果问题仍未定位，继续看：

```bash
kubectl -n extension-frontend-forge logs deploy/frontend-forge-controller
kubectl -n extension-frontend-forge logs deploy/frontend-forge
```

同时核对：

- deployment 环境变量
- CRD 版本
- 集群 RBAC
