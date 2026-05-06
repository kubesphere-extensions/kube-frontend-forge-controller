# Kubernetes Version Matrix

Code owners: `scripts/k8s-matrix-step1-install-ks.sh`,
`scripts/k8s-matrix-step2-install-frontend-forge.sh`,
`scripts/k8s-matrix-step3-fi-test.sh`

## Status

| Step | Status | Script |
| --- | --- | --- |
| Create remote kind cluster and install KubeSphere | Implemented | `k8s-matrix-step1-install-ks.sh` |
| Apply frontend-forge `InstallPlan` | Implemented | `k8s-matrix-step2-install-frontend-forge.sh` |
| Run FI lifecycle smoke test | Implemented | `k8s-matrix-step3-fi-test.sh` |
| Webhook coverage | Planned / TODO | Step3 does not enable webhook. |
| FE package/publish matrix coverage | Planned / TODO | Matrix currently targets FI lifecycle. |

## Supported Versions

Status: Implemented

| Kubernetes version | kind node image |
| --- | --- |
| `1.23` | `kindest/node:v1.23.17` |
| `1.26` | `kindest/node:v1.26.15` |
| `1.28` | `kindest/node:v1.28.15` |
| `1.30` | `kindest/node:v1.30.13` |
| `1.32` | `kindest/node:v1.32.11` |
| `1.34` | `kindest/node:v1.34.3` |

Cluster name format:

```text
ff-k<version-without-dot>
```

Example: `1.32` -> `ff-k132`.

## Shared Environment

Status: Implemented

| Env | Default | Used by |
| --- | --- | --- |
| `REMOTE_SSH_TARGET` | required | All steps |
| `REMOTE_KIND_BIN` | `/root/go/bin/kind` | All steps |
| `REMOTE_KUBECONFIG_ROOT` | `/root/.kube/frontend-forge-matrix` | All steps |
| `REMOTE_WORK_ROOT` | `/root/.frontend-forge-matrix` | All steps |
| `ARTIFACT_ROOT` | `artifacts/k8s-matrix` | All steps |
| `POLL_INTERVAL_SECONDS` | `5` | Step2, Step3 |

Constraints:

- Single version per run.
- The scripts manage only the derived `ff-k*` cluster.
- The default `kind-kind` cluster is not managed.
- Remote host is represented in docs as `root@<remote-host>`.

## Step1: Install Kubernetes And KubeSphere

Status: Implemented

Command:

```bash
REMOTE_SSH_TARGET=root@<remote-host> \
./scripts/k8s-matrix-step1-install-ks.sh 1.32
```

Additional env:

| Env | Default | Behavior |
| --- | --- | --- |
| `KS_CHART` | `oci://hub.kubesphere.com.cn/kse/ks-core` | KubeSphere chart. |
| `KS_VERSION` | `1.2.4` | KubeSphere chart version. |
| `KS_HELM_TIMEOUT` | `30m` | Helm install timeout. |
| `KUBESPHERE_NAMESPACE` | `kubesphere-system` | KubeSphere namespace. |
| `KIND_WAIT` | `5m` | kind create wait. |
| `MIN_FREE_GB` | `20` | Remote disk guard. |
| `MIN_MEM_AVAILABLE_GB` | `8` | Remote memory guard. |
| `KIND_EXPOSE_30880_HOST_PORT` | `30880` | Host port mapped to node port `30880`. |

Behavior:

- Verifies remote Docker, kubectl, Helm, kind, disk, and memory.
- Deletes existing derived test cluster.
- Creates a kind cluster for the requested Kubernetes version.
- Writes remote kubeconfig under `REMOTE_KUBECONFIG_ROOT`.
- Installs KubeSphere.
- Patches `extensions-museum` to trigger sync.

## Step2: Apply InstallPlan

Status: Implemented

Command:

```bash
REMOTE_SSH_TARGET=root@<remote-host> \
./scripts/k8s-matrix-step2-install-frontend-forge.sh 1.32
```

Additional env:

| Env | Default |
| --- | --- |
| `INSTALLPLAN_NAME` | `frontend-forge` |
| `INSTALLPLAN_CREATOR` | `admin` |
| `FRONTEND_FORGE_VERSION` | `1.0.0-rc.1` |
| `INSTALLPLAN_WAIT_TIMEOUT_SECONDS` | `600` |

Applied object:

```yaml
apiVersion: kubesphere.io/v1alpha1
kind: InstallPlan
metadata:
  name: frontend-forge
  annotations:
    kubesphere.io/creator: admin
spec:
  enabled: true
  extension:
    name: frontend-forge
    version: 1.0.0-rc.1
```

Behavior:

- Verifies derived cluster and kubeconfig.
- Waits for `installplans.kubesphere.io` CRD.
- Writes InstallPlan YAML into remote work dir.
- Applies and records the InstallPlan.

## Step3: FI Lifecycle Test

Status: Implemented

Command:

```bash
REMOTE_SSH_TARGET=root@<remote-host> \
./scripts/k8s-matrix-step3-fi-test.sh 1.32
```

Additional env:

| Env | Default | Behavior |
| --- | --- | --- |
| `FRONTEND_FORGE_NAMESPACE` | `extension-frontend-forge` | Namespace checked for readiness. |
| `SAMPLE_FILE` | `config/samples/fi-lifecycle-smoke.yaml` | FI sample applied during lifecycle test. |
| `READINESS_TIMEOUT_SECONDS` | `1800` | frontend-forge readiness timeout. |
| `LIFECYCLE_TIMEOUT_SECONDS` | `600` | Create/update/disable/enable wait timeout. |
| `DELETE_TIMEOUT_SECONDS` | `300` | FI deletion wait timeout. |
| `CLEANUP_ON_SUCCESS` | `false` | Deletes test cluster after success when true. |

Readiness checks:

- Namespace `extension-frontend-forge` exists.
- `frontendintegrations.frontend-forge.kubesphere.io` CRD exists.
- `jsbundles.extensions.kubesphere.io` CRD exists.
- `deployment/frontend-forge` is ready.
- `deployment/frontend-forge-controller` is ready.

Lifecycle actions:

| Action | Expected behavior |
| --- | --- |
| Create FI | FI reaches `Succeeded`; JSBundle exists. |
| Modify FI | New observed hash and bundle update. |
| Disable FI | FI status message `Disabled`; JSBundle state `Disabled`. |
| Enable FI | FI returns to `Succeeded`; JSBundle state `Available`. |
| Delete FI | FI is removed; related checks are recorded. |

Fixed object names:

| Name | Value |
| --- | --- |
| FI | `fi-lifecycle-smoke` |
| JSBundle | `fi-fi-lifecycle-smoke` |
| ConfigMap | `fi-fi-lifecycle-smoke-config` |

## Artifacts

Status: Implemented

Default directory:

```text
artifacts/k8s-matrix/<version>/
```

Common files:

| Step | Files |
| --- | --- |
| Step1 | `step1-install.log`, `step1-summary.md` |
| Step2 | `step2-install.log`, `step2-summary.md` |
| Step3 | `step3-readiness.log`, `fi-*.yaml`, `fi-*.apply.log`, `fi-*.result.yaml`, `step3-summary.json` |
| Failure collection | `kubectl-get-all-wide.txt`, `kubectl-get-fi-jsbundle.yaml`, controller describe/log files, `kind-export/` |

## TODO / Open Question

Status: Planned / TODO

- Matrix workflow does not enable or validate FI admission webhook.
- Matrix workflow does not cover FE package/download/publish.
