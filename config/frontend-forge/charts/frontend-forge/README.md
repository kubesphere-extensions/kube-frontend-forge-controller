# frontend-forge Helm Chart

This chart installs the Frontend Forge runtime controller, FrontendExtension
package/publish controller, extension API, CRDs, RBAC, and optional webhook
resources.

```bash
helm upgrade --install frontend-forge config/charts/frontend-forge \
  --namespace extension-frontend-forge \
  --create-namespace
```

Render locally:

```bash
helm template frontend-forge config/charts/frontend-forge \
  --namespace extension-frontend-forge
```

For local/e2e clusters that do not provide the KubeSphere `JSBundle` CRD:

```bash
helm upgrade --install frontend-forge config/charts/frontend-forge \
  --namespace extension-frontend-forge \
  --create-namespace \
  --set crds.installJsBundle=true
```

When the KubeSphere `extensions.kubesphere.io/v1alpha1/APIService` resource is
available, the chart registers the FrontendExtension API through APIService:

```yaml
extensionApi:
  apiService:
    enabled: true
    group: frontend-forge-api.kubesphere.io
    version: v1alpha1
```

The API backend serves publisher endpoints under
`/apis/frontend-forge-api.kubesphere.io/v1alpha1/...`. The FI to FE migrator
calls the chart service `frontend-forge-extension-api` directly instead of going
through ks-apiserver. The group intentionally differs from the FrontendExtension
CRD group `frontend-forge.kubesphere.io`, because KubeSphere serves CRDs with
`kubesphere.io/resource-served: 'true'` through `/kapis/<crd-group>/<version>/...`.

## Publish target

The chart creates `ConfigMap/ksbuilder-publish-config` in the release namespace
by default. `FrontendExtension.spec.publishPolicy.defaultTargetRef` can point at
this ConfigMap so publisher Jobs have a default `ksbuilder publish` target.
If the referenced target ConfigMap or Secret does not exist, the publisher treats
it as empty target data and still runs `ksbuilder`.

```yaml
publishTargetConfig:
  enabled: true
  name: ksbuilder-publish-config
  namespace: ""
  data:
    args: ""
```

`args` is appended to `ksbuilder publish <package-dir>`. Keys named
`env.<NAME>` are passed to the publisher process as environment variables.

## FI to FE migration

The chart installs a Helm hook Job named
`<release-name>-fi-to-fe-migrator` when `migration.fiToFe.enabled=true`.
It is intended for upgrades that disable the legacy FI runtime controller and
move existing `FrontendIntegration` resources to `FrontendExtension`.

Default behavior:

- `controller.enabled=false`.
- `migration.fiToFe.enabled=true`.
- `migration.fiToFe.backoffLimit=0`.
- `migration.fiToFe.hookDeletePolicy=before-hook-creation`; failed Jobs are not
  deleted automatically.
- The migrator checks FI/FE CRD readiness before processing resources.
- FI/FE are cluster-scoped resources, so the migrator uses cluster-scoped
  Kubernetes API operations.
- The target FE name is always `fi-<fi.metadata.name>`. Existing `fi-` prefixes
  are preserved, for example `fi-foo -> fi-fi-foo`.
- Overlong FE names use a stable `fi-<slice>-<hash>` form.
- A target FE that already exists but is not marked as migrator-owned is skipped
  and reported as a failure.
- Enabled FI resources are published by patching FE publish intent annotations
  after the source FI has been deleted. The FE controller creates the publish Job
  after the matching FE artifact is Ready.

Package defaults added during migration:

```yaml
spec:
  package:
    version: "0.1.0"
    icon: ./static/favicon.svg
    category: dev-tools
    provider:
      en:
        name: <FI kubesphere.io/creator or "Fi Migration Bot">
      zh:
        name: <FI kubesphere.io/creator or "Fi Migration Bot">
```

Main values:

```yaml
migration:
  fiToFe:
    enabled: true
    packageVersion: "0.1.0"
    schemaVersion: v1
    readyTimeoutSeconds: 600
    pollIntervalSeconds: 5
    backoffLimit: 0
    hookDeletePolicy: before-hook-creation
    publishTarget:
      kind: ConfigMap
      namespace: ""
      name: ksbuilder-publish-config
```

The migrator writes publish intent annotations directly to migrated FE resources;
the FE controller publishes enabled FI migrations after the matching FE artifact is
ready.

Full migration semantics are documented in
[`spec/fi-to-fe-migration.md`](../../../spec/fi-to-fe-migration.md).
