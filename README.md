# Frontend Forge

Frontend Forge provides Kubernetes controllers and jobs for KubeSphere frontend
extensions. It covers the `FrontendExtension` package/download/publish/unpublish flow and
the `FrontendIntegration` runtime flow that builds a `JSBundle` for the current
cluster.

The default Helm installation is centered on `FrontendExtension`: package an
extension artifact, expose it through the extension API, and optionally publish
it through a publisher Job. The older FI runtime controller is still implemented,
but it is disabled by default and existing FI objects are migrated to FE by the
default Helm hook.

## What It Does

| Flow | Status | Default Helm behavior |
| --- | --- | --- |
| `FrontendExtension` package/download/publish/unpublish | Implemented | Enabled |
| `FrontendIntegration` runtime build to `JSBundle` | Implemented | Disabled with `controller.enabled=false` |
| FI to FE `migrator` | Implemented | Enabled with `migration.fiToFe.enabled=true` |
| FI admission webhook | Implemented | Disabled with `webhook.enabled=false` |
| Local/e2e build-service stub | Implemented for local/e2e | Disabled with `buildService.enabled=false` |

Core objects:

| Kind | Scope | Purpose |
| --- | --- | --- |
| `FrontendExtension` / `FE` | Cluster | Package source, artifact status, download and publish state. |
| `FrontendIntegration` / `FI` | Cluster | Runtime source for building a current-cluster `JSBundle`. |
| `JSBundle` | Cluster | Runtime frontend bundle consumed by the KubeSphere extension runtime. |

The extension API supports FE list, get, create, download, publish, unpublish, and delete
operations. Route-level behavior is documented in
[`spec/frontend-extension-design.md`](spec/frontend-extension-design.md).

## Default Helm Behavior

Default values come from [`config/charts/frontend-forge/values.yaml`](config/charts/frontend-forge/values.yaml):

| Value | Default | Effect |
| --- | --- | --- |
| `extensionController.enabled` | `true` | Installs the FE package/publish controller. |
| `extensionApi.enabled` | `true` | Installs the FE HTTP API Deployment and Service. |
| `migration.fiToFe.enabled` | `true` | Runs the FI-to-FE migration hook after install/upgrade. |
| `controller.enabled` | `false` | Does not install the FI runtime controller. |
| `webhook.enabled` | `false` | Does not install the FI validating webhook. |
| `crds.installJsBundle` | `false` | Does not install the external `JSBundle` CRD. |
| `buildService.enabled` | `false` | Does not install the local/e2e build-service stub. |

FE packaging and FI runtime builds both call `BUILD_SERVICE_BASE_URL`. With chart
defaults, that URL is derived even when `buildService.enabled=false`; provide a
service at that address, set an external URL, or enable the local/e2e stub.

## Quick Install

Default install:

```bash
helm upgrade --install frontend-forge config/charts/frontend-forge \
  --namespace extension-frontend-forge \
  --create-namespace
```

Install the local/e2e `JSBundle` CRD when the cluster does not provide it:

```bash
helm upgrade --install frontend-forge config/charts/frontend-forge \
  --namespace extension-frontend-forge \
  --create-namespace \
  --set crds.installJsBundle=true
```

Enable FI runtime for local/e2e testing:

```bash
helm upgrade --install frontend-forge config/charts/frontend-forge \
  --namespace extension-frontend-forge \
  --create-namespace \
  --set controller.enabled=true \
  --set crds.installJsBundle=true \
  --set buildService.enabled=true
```

## Basic Usage

Create a `FrontendExtension` sample:

```bash
kubectl apply -f config/samples/frontendextension-inspecttask.yaml
kubectl get frontendextensions.frontend-forge.kubesphere.io inspecttask
```

Create a `FrontendIntegration` sample when FI runtime is enabled:

```bash
kubectl apply -f config/samples/frontend-forge_v1alpha1_frontendintegration.yaml
kubectl get frontendintegrations.frontend-forge.kubesphere.io demo-fi
```

Other useful samples:

| File | Purpose |
| --- | --- |
| `config/samples/fi-crdtable.yaml` | FI `crdTable` page sample. |
| `config/samples/fi-nested-menu-demo.yaml` | FI two-level menu sample. |
| `config/samples/fi-lifecycle-smoke.yaml` | FI lifecycle smoke test sample. |

## Repository Layout

| Path | Responsibility |
| --- | --- |
| `crates/api` | Rust CRD types and CRD generation entrypoints. |
| `crates/frontend-extension-controller` | FE package/publish/unpublish reconciliation. |
| `crates/frontend-forge-controller` | FI runtime controller, FI webhook, FI-to-FE migrator. |
| `crates/frontend-forge-extension-api` | FE list/get/create/download/publish/unpublish/delete HTTP API. |
| `config/charts/frontend-forge` | Helm chart, CRDs, Deployments, RBAC, hooks. |
| `config/samples` | Example FI/FE manifests. |
| `spec` | Implementation notes tied to Rust types, controllers, routes, and chart defaults. |
| `skills/frontend-forge-fi-operations` | Repo-local Codex skill for FI operations. |

More detailed crate and Job behavior is documented under `spec/`.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo xtask gen-crd
```

Build the main binaries:

```bash
cargo build --release -p frontend-forge-controller
cargo build --release -p frontend-forge-runner
cargo build --release -p frontend-extension-controller
cargo build --release -p frontend-forge-extension-api
cargo build --release -p frontend-forge-extension-packager
cargo build --release -p frontend-forge-extension-publisher
```

Install git hooks:

```bash
lefthook install
```

Register the repo-local skill for Codex:

```bash
mkdir -p "${CODEX_HOME:-$HOME/.codex}/skills"
ln -s "$(pwd)/skills/frontend-forge-fi-operations" \
  "${CODEX_HOME:-$HOME/.codex}/skills/frontend-forge-fi-operations"
```

## Further Reading

| Topic | Document |
| --- | --- |
| CRD fields and status | [`spec/crds.md`](spec/crds.md) |
| Manifest renderer | [`spec/Manifest.md`](spec/Manifest.md) |
| FI runtime controller, runner, webhook | [`spec/fi-runtime.md`](spec/fi-runtime.md) |
| FE package/publish/API behavior | [`spec/frontend-extension-design.md`](spec/frontend-extension-design.md) |
| Helm values and template conditions | [`spec/helm-chart.md`](spec/helm-chart.md) |
| Kubernetes resources | [`spec/k8s-resources.md`](spec/k8s-resources.md) |
| FI-to-FE migration | [`spec/fi-to-fe-migration.md`](spec/fi-to-fe-migration.md) |
| Manual image build and install | [`spec/manual-build-and-helm-install.md`](spec/manual-build-and-helm-install.md) |
| Kubernetes version matrix | [`spec/k8s-matrix-plan.md`](spec/k8s-matrix-plan.md) |

Documentation details in `spec/` are checked against Rust API types, controller
behavior, Helm defaults, and HTTP routes.

## TODO / Open Questions

- Production build-service deployment ownership is not defined in this
  repository; current docs cover the chart stub and external URL configuration.
- Concrete `ksbuilder publish` credential keys depend on the selected
  `ksbuilder` and registry setup. The publisher defines how target data is
  passed to the process.
