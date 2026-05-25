# Kubernetes Resources

Code owner: `config/charts/frontend-forge`

Source of truth: Helm templates under `config/charts/frontend-forge`

## Status

| Resource group | Status | Default render state |
| --- | --- | --- |
| FI/FE CRDs | Implemented | Always installed from `crds/`. |
| FE package/publish/unpublish resources | Implemented | Rendered by default. |
| FE HTTP API resources | Implemented | Rendered by default. |
| Default publish target ConfigMap | Implemented | Rendered by default. |
| FI runtime resources | Implemented | Not rendered by default. |
| FI-to-FE migrator | Implemented | Rendered by default as Helm hook. |
| FI webhook resources | Implemented | Not rendered by default. |
| Local/e2e `JSBundle` CRD | Implemented | Not rendered by default. |
| Local/e2e build-service stub | Implemented | Not rendered by default. |

## Chart Paths

Status: Implemented

```text
config/charts/frontend-forge/
  Chart.yaml
  values.yaml
  values-e2e.yaml
  crds/
    frontend-forge.kubesphere.io_frontendintegrations.yaml
    frontend-forge.kubesphere.io_frontendextensions.yaml
  templates/
    build-service.yaml
    fi-to-fe-migration-job.yaml
    frontend-extension-controller-deployment.yaml
    frontend-forge-controller-deployment.yaml
    frontend-forge-extension-api-apiservice.yaml
    frontend-forge-extension-api-deployment.yaml
    jsbundle-crd.yaml
    publish-target-config.yaml
    rbac-extension.yaml
    rbac-runtime.yaml
    serviceaccounts.yaml
    webhook.yaml
    webhook-certgen.yaml
```

## Chart-Managed Resources

Status: Implemented

| Resource | Render condition | Template |
| --- | --- | --- |
| `CustomResourceDefinition/frontendintegrations.frontend-forge.kubesphere.io` | Always from `crds/` | `crds/frontend-forge.kubesphere.io_frontendintegrations.yaml` |
| `CustomResourceDefinition/frontendextensions.frontend-forge.kubesphere.io` | Always from `crds/` | `crds/frontend-forge.kubesphere.io_frontendextensions.yaml` |
| `CustomResourceDefinition/jsbundles.extensions.kubesphere.io` | `crds.installJsBundle=true` | `templates/jsbundle-crd.yaml` |
| `Deployment/<fullname>-controller` | `controller.enabled=true` | `templates/frontend-forge-controller-deployment.yaml` |
| Runtime controller ServiceAccount | `controller.enabled=true` and SA create enabled | `templates/serviceaccounts.yaml` |
| Runtime runner ServiceAccount | `controller.enabled=true` and SA create enabled | `templates/serviceaccounts.yaml` |
| Runtime ClusterRole / ClusterRoleBinding | `rbac.create=true` and `controller.enabled=true` | `templates/rbac-runtime.yaml` |
| Runner ConfigMap writer Role / RoleBinding | `rbac.create=true` and `controller.enabled=true` | `templates/rbac-runtime.yaml` |
| `Deployment/<fullname>-extension-controller` | `extensionController.enabled=true` | `templates/frontend-extension-controller-deployment.yaml` |
| Extension controller ServiceAccount | `extensionController.enabled=true` and SA create enabled | `templates/serviceaccounts.yaml` |
| Extension packager ServiceAccount | `extensionController.enabled=true` and SA create enabled | `templates/serviceaccounts.yaml` |
| Extension publisher ServiceAccount | `extensionController.enabled=true` and SA create enabled | `templates/serviceaccounts.yaml` |
| Extension controller/packager/publisher RBAC | `rbac.create=true` and `extensionController.enabled=true` | `templates/rbac-extension.yaml` |
| `Deployment/<fullname>-extension-api` | `extensionApi.enabled=true` | `templates/frontend-forge-extension-api-deployment.yaml` |
| `Service/<fullname>-extension-api` | `extensionApi.enabled=true` | `templates/frontend-forge-extension-api-deployment.yaml` |
| Extension API ServiceAccount | `extensionApi.enabled=true` and SA create enabled | `templates/serviceaccounts.yaml` |
| Extension API RBAC | `rbac.create=true` and `extensionApi.enabled=true` | `templates/rbac-extension.yaml` |
| KubeSphere `APIService` | APIService render guard passes | `templates/frontend-forge-extension-api-apiservice.yaml` |
| `ConfigMap/ksbuilder-publish-config` | `publishTargetConfig.enabled=true` | `templates/publish-target-config.yaml` |
| Migrator ServiceAccount | `migration.fiToFe.enabled=true` and SA create enabled | `templates/serviceaccounts.yaml` |
| Migrator ClusterRole / ClusterRoleBinding | `migration.fiToFe.enabled=true` | `templates/fi-to-fe-migration-job.yaml` |
| `Job/<fullname>-fi-to-fe-migrator` | `migration.fiToFe.enabled=true` | `templates/fi-to-fe-migration-job.yaml` |
| Webhook Service | `controller.enabled=true` and `webhook.enabled=true` | `templates/webhook.yaml` |
| `ValidatingWebhookConfiguration` | `controller.enabled=true` and `webhook.enabled=true` | `templates/webhook.yaml` |
| Webhook certgen RBAC / Jobs | `controller.enabled=true` and `webhook.enabled=true` | `templates/webhook-certgen.yaml` |
| Build-service Deployment / Service | `buildService.enabled=true` | `templates/build-service.yaml` |

`<fullname>` is derived by the Helm helper `frontend-forge.fullname`.

## Runtime Resource Use

Status: Implemented

| API | Kind | Scope | Used by | Behavior |
| --- | --- | --- | --- | --- |
| `frontend-forge.kubesphere.io/v1alpha1` | `FrontendIntegration` | Cluster | FI controller, runner, webhook, migrator | Runtime source; migrator reads and deletes after FE Ready. |
| `frontend-forge.kubesphere.io/v1alpha1` | `FrontendExtension` | Cluster | FE controller, API, packager, migrator | Package/publish/unpublish source and status object. |
| `extensions.kubesphere.io/v1alpha1` | `JSBundle` | Cluster | FI controller, runner | Runtime bundle CR. |
| `batch/v1` | `Job` | Namespaced | FI controller, FE controller | Build, package, publish, unpublish jobs. |
| `v1` | `ConfigMap` | Namespaced | FI runner, FE packager, FE API, publisher | Bundle content, package artifact, publish target. |
| `v1` | `Secret` | Namespaced | Webhook certgen, publisher | Webhook TLS, publish target. |
| `admissionregistration.k8s.io/v1` | `ValidatingWebhookConfiguration` | Cluster | API server, certgen | FI admission validation. |
| `extensions.kubesphere.io/v1alpha1` | `APIService` | Cluster | KubeSphere API aggregation | Optional FE API registration. |

## Namespaces

Status: Implemented

| Resource | Namespace source |
| --- | --- |
| Runtime controller Deployment | Helm release namespace. |
| Runtime runner Jobs | `controller.workNamespace`, default release namespace. |
| Runner bundle ConfigMaps | `controller.jsbundleConfigmapNamespace`, default FI work namespace. |
| FE controller Deployment | Helm release namespace. |
| FE package/publish/unpublish Jobs | `WORK_NAMESPACE`, default release namespace. |
| FE artifact ConfigMaps | `extensionController.artifactConfigmapNamespace`, default release namespace. |
| FE API Deployment/Service | Helm release namespace. |
| Default publish target ConfigMap | `publishTargetConfig.namespace`, default release namespace. |
| Migrator Job | Helm release namespace. |
| Migrated FE publish target namespace | `migration.fiToFe.publishTarget.namespace`, default release namespace. |

## Labels And Ownership

Status: Implemented

| Label / owner | Behavior |
| --- | --- |
| Helm labels | All chart resources use chart helper labels. |
| Component label | Templates add `app.kubernetes.io/component` per component. |
| FI runner Jobs | Owner reference points to FI when available. |
| FI bundle ConfigMaps | Owner reference points to FI when available. |
| FI `JSBundle` | Runner writes owner reference; controller patches owner reference if missing. |
| FE package Jobs | Owner reference points to FE. |
| FE publish/unpublish Jobs | Owner reference points to FE. |
| FE artifact ConfigMaps | Owner reference points to FE. |
| Migrated FE | Labeled `frontend-forge.kubesphere.io/managed-by=frontend-forge-fi-migrator`. |

## Constraints

Status: Implemented

- Chart is the install path in this repository; no `config/manager` or Kustomize install path is maintained.
- FI/FE CRDs in `crds/` are generated by `cargo xtask gen-crd`.
- `JSBundle` CRD is treated as external unless `crds.installJsBundle=true`.
- `ksbuilder publish` and `ksbuilder unpublish` run only in publisher Jobs.
- Migrator calls FE API to request publish; it does not create publish Jobs directly.
- Jobs, ConfigMaps, Secrets, and Deployments are namespaced even when FI/FE are cluster-scoped.

## TODO / Open Question

Status: Planned / TODO

- Production build-service deployment ownership is outside current chart defaults.
- APIService behavior depends on KubeSphere APIService CRD availability in the target cluster.
