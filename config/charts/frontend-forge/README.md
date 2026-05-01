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
