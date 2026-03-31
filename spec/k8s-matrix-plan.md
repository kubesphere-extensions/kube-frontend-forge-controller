# K8s 多版本构建与测试流程

## 背景

原先的一体化方案假设“安装 KS 后会自动安装 frontend-forge”，在新建测试集群上不稳定成立。当前统一采用 3 步流程：

1. Step1：安装 K8s + KS（脚本）
2. Step2：用户手动安装 frontend-forge（人工）
3. Step3：执行 FrontendIntegration 生命周期自动化测试（脚本）

兼容入口：

- `scripts/k8s-matrix-remote.sh` 仅保留迁移提示，不再执行旧一体化流程

## 目标

在远程主机 `root@<remote-host>` 上按版本重建 kind 集群，并验证 `FrontendIntegration` 的完整生命周期：

- 创建
- 修改
- 禁用
- 启用
- 删除

## 固定约束

- 单版本单次执行，避免多套 KS 同时占用远程资源
- Step2 必须人工介入，Step3 不会自动安装 frontend-forge
- `frontend-forge` webhook 当前跳过
- 不接管默认 `kind-kind` 集群
- 远端示例统一使用 `root@<remote-host>` 占位

## 远程依赖

- 远程 kind 路径：`/root/go/bin/kind`
- 远程 kubeconfig 目录：`/root/.kube/frontend-forge-matrix`
- 远程工作目录：`/root/.frontend-forge-matrix`

## 版本映射

- `1.23` -> `kindest/node:v1.23.17`
- `1.26` -> `kindest/node:v1.26.15`
- `1.28` -> `kindest/node:v1.28.15`
- `1.30` -> `kindest/node:v1.30.13`
- `1.32` -> `kindest/node:v1.32.11`
- `1.34` -> `kindest/node:v1.34.3`

## Step1：安装 K8s + KS

入口：

```bash
REMOTE_SSH_TARGET=root@<remote-host> ./scripts/k8s-matrix-step1-install-ks.sh <version>
```

示例：

```bash
REMOTE_SSH_TARGET=root@<remote-host> ./scripts/k8s-matrix-step1-install-ks.sh 1.32
```

行为：

- 清理同名历史测试集群，仅处理当前版本对应 cluster
- 创建 kind 集群
- 安装 KS
- patch `extensions-museum` 触发同步

端口暴露：

- kind 创建时会把节点 `30880` 映射到远程主机端口
- 默认映射：`host:30880 -> control-plane:30880`
- 可通过环境变量覆盖：`KIND_EXPOSE_30880_HOST_PORT`

示例：

```bash
REMOTE_SSH_TARGET=root@<remote-host> KIND_EXPOSE_30880_HOST_PORT=38080 ./scripts/k8s-matrix-step1-install-ks.sh 1.32
```

## Step2：手动安装 frontend-forge

Step1 完成后，用户在当前测试集群内手动安装 frontend-forge。

说明：

- Step3 不会自动创建 `InstallPlan/frontend-forge`
- Step3 只等待并校验 frontend-forge 已安装且 Ready

## Step3：执行生命周期自动化测试

入口：

```bash
REMOTE_SSH_TARGET=root@<remote-host> ./scripts/k8s-matrix-step3-fi-test.sh <version>
```

示例：

```bash
REMOTE_SSH_TARGET=root@<remote-host> ./scripts/k8s-matrix-step3-fi-test.sh 1.32
```

行为：

- 检查目标 cluster 和 kubeconfig 是否存在
- 等待 frontend-forge Ready：
  - `extension-frontend-forge` namespace 存在
  - `frontendintegrations.frontend-forge.kubesphere.io` CRD 存在
  - `jsbundles.extensions.kubesphere.io` CRD 存在
  - `deployment/frontend-forge` Ready
  - `deployment/frontend-forge-controller` Ready
- 执行 FI 生命周期测试：
  - 创建
  - 修改
  - 禁用
  - 启用
  - 删除

可选参数：

- `CLEANUP_ON_SUCCESS=true` 时，Step3 成功后删除该测试集群

## 产物目录

默认产物目录：

```bash
artifacts/k8s-matrix/<version>/
```

主要文件：

- Step1：
  - `step1-install.log`
  - `step1-summary.md`
- Step3：
  - `step3-readiness.log`
  - `fi-*.yaml`
  - `fi-*.apply.log`
  - `fi-*.result.yaml`
  - `step3-summary.json`

失败时额外收集：

- `kubectl-get-all-wide.txt`
- `kubectl-get-fi-jsbundle.yaml`
- `describe-frontend-forge-controller.txt`
- `frontend-forge-controller.log`
- `frontend-forge.log`
- `kind-export/`
