# FrontendIntegration 生命周期操作索引

核心来源：

- `config/samples/fi-lifecycle-smoke.yaml`
- `scripts/k8s-matrix-step3-fi-test.sh`
- `README.md`

## 基本事实

- `FrontendIntegration` 是 cluster-scoped 资源
- 短名：`fi`
- 建议从样例起步：
  - `config/samples/fi-lifecycle-smoke.yaml`

## 新建

```bash
kubectl apply -f config/samples/fi-lifecycle-smoke.yaml
kubectl get fi fi-lifecycle-smoke -o yaml
```

常看字段：

- `.status.phase`
- `.status.message`
- `.status.observed_spec_hash`
- `.status.observed_generation`
- `.status.last_build`
- `.status.bundle_ref`

## 修改

推荐直接改 YAML 后重新 `apply`，或者 patch 具体字段。例如修改 iframe 地址：

```bash
kubectl patch fi fi-lifecycle-smoke --type merge -p \
  '{"spec":{"pages":[{"key":"lifecycle-smoke","type":"iframe","iframe":{"src":"http://example.test/v2"}}]}}'
```

修改后重点看：

- `.status.observed_generation` 是否推进
- `.status.observed_spec_hash` 是否变化
- `.status.phase` 是否回到 `Succeeded`

## 禁用

```bash
kubectl patch fi fi-lifecycle-smoke --type merge -p '{"spec":{"enabled":false}}'
kubectl get fi fi-lifecycle-smoke -o yaml
```

禁用后重点看：

- `.status.phase`
- `.status.message`
- `.status.last_build`

## 启用

```bash
kubectl patch fi fi-lifecycle-smoke --type merge -p '{"spec":{"enabled":true}}'
kubectl get fi fi-lifecycle-smoke -o yaml
```

启用后重点看：

- `.status.phase`
- `.status.bundle_ref.name`
- 关联 `JSBundle.status.state`

## 删除

```bash
kubectl delete fi fi-lifecycle-smoke
```

删除后再核对：

- `kubectl get fi fi-lifecycle-smoke`
- `kubectl get jsbundle fi-fi-lifecycle-smoke`
- `kubectl get configmap -n extension-frontend-forge fi-fi-lifecycle-smoke-config`
