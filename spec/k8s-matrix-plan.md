# 远程 K8s 多版本测试计划（3 步版）

## 背景

原计划假设“KS 自动安装 frontend-forge”，在新建测试集群上不稳定成立。现统一改为 3 步：

1. Step1（脚本）：安装 K8s + KS
2. Step2（人工）：用户手动安装 frontend-forge
3. Step3（脚本）：执行 FrontendIntegration 生命周期自动化测试

## 当前脚本拆分

- Step1：`scripts/k8s-matrix-step1-install-ks.sh <version>`
- Step3：`scripts/k8s-matrix-step3-fi-test.sh <version>`

兼容入口：

- `scripts/k8s-matrix-remote.sh` 仅保留迁移提示，不再执行旧一体化流程

## 关键变更

- kind 创建时显式暴露端口：`host:${KIND_EXPOSE_30880_HOST_PORT:-30880} -> node:30880`
- Step3 不再自动安装 frontend-forge，只做 Ready 校验与生命周期测试
- 单版本单次执行，便于人工 Step2 介入

## 不变约束

- 远程主机：`root@<remote-host>`
- 远程 kind：`/root/go/bin/kind`
- 跳过 webhook 测试
- 不接管默认 `kind-kind` 集群
