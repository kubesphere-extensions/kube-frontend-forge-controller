# Helm Chart

Code owner: `config/charts/frontend-forge`

Source of truth: `config/charts/frontend-forge/values.yaml`,
`config/charts/frontend-forge/templates/*`,
`config/charts/frontend-forge/crds/*`

## Status

| Capability | Status | Template / path |
| --- | --- | --- |
| FI/FE CRDs in `crds/` | Implemented | `crds/*.yaml` |
| Optional local/e2e `JSBundle` CRD | Implemented | `templates/jsbundle-crd.yaml` |
| FI runtime controller resources | Implemented | Rendered only when `controller.enabled=true` |
| FE controller resources | Implemented | Rendered when `extensionController.enabled=true` |
| FE HTTP API resources | Implemented | Rendered when `extensionApi.enabled=true` |
| FI-to-FE migration hook | Implemented | Rendered when `migration.fiToFe.enabled=true` |
| FI webhook resources | Implemented | Rendered only when `controller.enabled=true` and `webhook.enabled=true` |
| Local/e2e build-service stub | Implemented | Rendered when `buildService.enabled=true` |

## Default Values

Status: Implemented

| Value | Default | Effect |
| --- | --- | --- |
| `controller.enabled` | `false` | FI runtime Deployment/RBAC/SA are not rendered. |
| `extensionController.enabled` | `true` | FE controller Deployment/RBAC/SA are rendered. |
| `extensionApi.enabled` | `true` | FE API Deployment/Service/RBAC/SA are rendered. |
| `extensionApi.apiService.enabled` | `true` | APIService is eligible to render. |
| `extensionApi.apiService.onlyIfApiResourceExists` | `true` | APIService renders only if the cluster has `extensions.kubesphere.io/v1alpha1/APIService`. |
| `migration.fiToFe.enabled` | `true` | Migration hook Job/RBAC/SA are rendered. |
| `publishTargetConfig.enabled` | `true` | Default `ksbuilder-publish-config` ConfigMap is rendered. |
| `crds.installJsBundle` | `false` | Optional local/e2e `JSBundle` CRD is not rendered. |
| `webhook.enabled` | `false` | Webhook resources are not rendered. |
| `buildService.enabled` | `false` | Build-service stub Deployment/Service are not rendered. |

Default install:

```bash
helm upgrade --install frontend-forge config/charts/frontend-forge \
  --namespace extension-frontend-forge \
  --create-namespace
```

Local/e2e install with runtime FI controller:

```bash
helm upgrade --install frontend-forge config/charts/frontend-forge \
  --namespace extension-frontend-forge \
  --create-namespace \
  --set controller.enabled=true \
  --set crds.installJsBundle=true \
  --set buildService.enabled=true
```

## Values Groups

Status: Implemented

| Group | Behavior |
| --- | --- |
| `global` | Global image registry and pull secrets. |
| `image` | FI runtime controller image and FI migrator image. |
| `controller` | FI runtime controller, runner Job config, webhook env injection. |
| `runner` | FI runner image and service account. |
| `extensionController` | FE controller Deployment config and package/publish/unpublish Job defaults. |
| `extensionPackager` | Package Job image and service account defaults. |
| `extensionPublisher` | Publish Job image and service account defaults. |
| `extensionApi` | FE API Deployment, Service, and optional KubeSphere APIService. |
| `migration.fiToFe` | FI-to-FE migrator hook and publish target defaults. |
| `publishTargetConfig` | Chart-created default ConfigMap read by publisher Jobs. |
| `webhook` | FI validating webhook, certgen jobs, TLS Secret names. |
| `buildService` | Local/e2e build-service stub. |
| `crds` | Optional external CRDs rendered from templates. |

## Render Conditions

Status: Implemented

| Condition | Rendered resources |
| --- | --- |
| Always | FI/FE CRDs under `crds/`. |
| `crds.installJsBundle=true` | `JSBundle` CRD template. |
| `controller.enabled=true` | FI controller Deployment, runtime RBAC, controller/runner service accounts. |
| `extensionController.enabled=true` | FE controller Deployment, FE controller RBAC, packager/publisher service accounts and RBAC. |
| `extensionApi.enabled=true` | FE API Deployment, Service, service account, RBAC. |
| `extensionApi.enabled=true` and APIService condition passes | KubeSphere `APIService`. |
| `publishTargetConfig.enabled=true` | Default publish target ConfigMap. |
| `migration.fiToFe.enabled=true` | Migrator hook Job, service account, ClusterRole, ClusterRoleBinding. |
| `controller.enabled=true` and `webhook.enabled=true` | Webhook Service, `ValidatingWebhookConfiguration`, certgen RBAC/Jobs. |
| `buildService.enabled=true` | Build-service Deployment and Service. |

## Namespace And URLs

Status: Implemented

| Value / helper | Default derivation |
| --- | --- |
| `controller.workNamespace` | Release namespace when empty. |
| `controller.jsbundleConfigmapNamespace` | FI work namespace when empty. |
| `extensionController.artifactConfigmapNamespace` | Release namespace when empty. |
| `controller.buildServiceBaseUrl` | `http://<build-service-name>.<release-namespace>.svc` when empty. |
| `extensionController.buildServiceBaseUrl` | `http://<build-service-name>.<release-namespace>.svc` when empty. |
| `migration.fiToFe.feApiBaseUrl` | `http://<extension-api-service>.<release-namespace>.svc:<extensionApi.service.port>` when empty. |
| `migration.fiToFe.publishTarget.namespace` | Release namespace when empty. |
| `publishTargetConfig.namespace` | Release namespace when empty. |

`buildService.enabled=false` does not clear the derived build-service URL. FE
package Jobs and FI runner Jobs still require a reachable service or explicit
external URL when those flows run.

## Publish Target ConfigMap

Status: Implemented

Template: `templates/publish-target-config.yaml`

Default values:

```yaml
publishTargetConfig:
  enabled: true
  name: ksbuilder-publish-config
  namespace: ""
  annotations: {}
  data:
    args: ""
  binaryData: {}
```

The ConfigMap is intended for
`FrontendExtension.spec.publishPolicy.defaultTargetRef`. Publisher Jobs read it
before running `ksbuilder publish`; `args` is appended to the command and
`env.<NAME>` entries become process environment variables.

## Extension APIService

Status: Implemented

Template: `templates/frontend-forge-extension-api-apiservice.yaml`

| Value | Default |
| --- | --- |
| `extensionApi.apiService.name` | `v1alpha1.frontend-forge-api.kubesphere.io` |
| `extensionApi.apiService.group` | `frontend-forge-api.kubesphere.io` |
| `extensionApi.apiService.version` | `v1alpha1` |
| `extensionApi.apiService.url` | FE API Service URL |
| `extensionApi.apiService.caBundle` | empty |
| `extensionApi.apiService.insecureSkipTLSVerify` | `null` |

Render guard:

```text
extensionApi.enabled
AND extensionApi.apiService.enabled
AND (
  NOT extensionApi.apiService.onlyIfApiResourceExists
  OR cluster supports extensions.kubesphere.io/v1alpha1/APIService
)
```

## Webhook

Status: Implemented

Webhook values:

| Value | Default |
| --- | --- |
| `webhook.enabled` | `false` |
| `webhook.bindAddr` | `0.0.0.0:9443` |
| `webhook.certPath` | `/tls/tls.crt` |
| `webhook.keyPath` | `/tls/tls.key` |
| `webhook.service.port` | `443` |
| `webhook.certgen.image.repository` | `kubespheredev/kube-webhook-certgen` |
| `webhook.certgen.image.tag` | `v1.1.1` |
| `webhook.certgen.ttlSecondsAfterFinished` | `300` |

certgen hook behavior:

| Job | Hook | Weight | Delete policy |
| --- | --- | --- | --- |
| `*-webhook-certgen-create` | `post-install,post-upgrade` | `-5` | `before-hook-creation,hook-succeeded` |
| `*-webhook-certgen-patch` | `post-install,post-upgrade` | `5` | `before-hook-creation,hook-succeeded` |

## FI-To-FE Migration

Status: Implemented

Default values:

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

Hook:

| Property | Value |
| --- | --- |
| Job name | `<release-name>-fi-to-fe-migrator` |
| Hook | `post-install,post-upgrade` |
| Hook weight | `10` |
| Backoff limit | `0` by default |
| Failed job deletion | Not deleted by default because `hook-failed` is absent. |

The migrator calls:

```text
POST <feApiBaseUrl>/apis/<api-group>/<api-version>/frontendextensions/<fe-name>/publish
```

with defaults:

```text
api-group=frontend-forge-api.kubesphere.io
api-version=v1alpha1
```

Migration field behavior is documented in `spec/fi-to-fe-migration.md`.

## e2e Values

Status: Implemented

`config/charts/frontend-forge/values-e2e.yaml` sets:

| Value | e2e value |
| --- | --- |
| `crds.installJsBundle` | `true` |
| `extensionController.enabled` | `false` |
| `extensionApi.enabled` | `false` |
| `buildService.enabled` | `true` |

The e2e script can layer additional `--set` values for the runtime controller
and images.

## TODO / Open Question

Status: Planned / TODO

- Production build-service ownership is not defined in the chart defaults.
- APIService availability depends on a KubeSphere CRD outside this repository.
