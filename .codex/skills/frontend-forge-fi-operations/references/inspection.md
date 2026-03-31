# FI 运行时观察索引

核心来源：

- `crates/common/src/lib.rs`
- `crates/api/src/lib.rs`
- `crates/controller/src/main.rs`
- `crates/runner/src/main.rs`

## 默认命名规则

- 默认 bundle 名：
  - `fi-<fi-name>`
- 默认 Job 名前缀：
  - `fi-<fi-name>-build-<hash8>`
- 默认 ConfigMap 名：
  - `<bundle-name>-config`

这些是当前默认值。若部署层改了运行时环境变量，要先回查 controller 配置。

## 关联资源查询

查看 FI：

```bash
kubectl get fi <fi-name> -o yaml
```

查看 Job：

```bash
kubectl get jobs -n extension-frontend-forge -l frontend-forge.io/fi-name=<fi-name>
kubectl describe job -n extension-frontend-forge <job-name>
```

查看 JSBundle：

```bash
kubectl get jsbundle fi-<fi-name> -o yaml
```

查看 ConfigMap：

```bash
kubectl get configmap -n extension-frontend-forge fi-<fi-name>-config -o yaml
```

## 重点标签与注解

常用标签：

- `frontend-forge.io/fi-name`
- `frontend-forge.io/spec-hash`
- `frontend-forge.io/manifest-hash`
- `frontend-forge.io/enabled`

常用注解：

- `frontend-forge.io/build-job`
- `frontend-forge.io/manifest-hash`
- `frontend-forge.io/manifest-content`
- `frontend-forge.io/source-spec`
- `frontend-forge.io/source-spec-hash`
- `frontend-forge.io/source-generation`

## 直接查看 Manifest 和来源 spec

查看 Manifest：

```bash
kubectl get jsbundle fi-<fi-name> -o jsonpath='{.metadata.annotations.frontend-forge\.io/manifest-content}'
```

查看来源 spec：

```bash
kubectl get jsbundle fi-<fi-name> -o jsonpath='{.metadata.annotations.frontend-forge\.io/source-spec}'
```

查看 build Job：

```bash
kubectl get jsbundle fi-<fi-name> -o jsonpath='{.metadata.annotations.frontend-forge\.io/build-job}'
```

## 关键状态字段

FI status：

- `.status.phase`
- `.status.observed_spec_hash`
- `.status.observed_manifest_hash`
- `.status.observed_generation`
- `.status.last_build.job_ref.name`
- `.status.last_error.message`
- `.status.bundle_ref.name`
- `.status.message`

JSBundle status：

- `.status.state`
- `.status.link`

## 排障顺序

1. 先看 FI `status`
2. 再看 `bundle_ref.name` 和 `last_build.job_ref.name`
3. 再去查 Job 日志、`JSBundle` 注解和 ConfigMap 内容
4. 如果 FI 没出 Job，重点排查 webhook 或 controller 是否拒绝或跳过了该 spec
