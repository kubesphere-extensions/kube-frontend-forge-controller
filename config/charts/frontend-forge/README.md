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
