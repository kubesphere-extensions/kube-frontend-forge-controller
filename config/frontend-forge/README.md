# Frontend Forge

Frontend Forge provides controllers and jobs for KubeSphere frontend extension
packaging, publishing, API serving, and legacy `FrontendIntegration` migration.

This directory is the KubeSphere extension wrapper. The installable Helm chart
lives in `charts/frontend-forge` so `ksbuilder` can package it. The legacy
`config/charts/frontend-forge` path is a compatibility symlink to the same chart.

## Package or Publish

```bash
ksbuilder package config/frontend-forge
ksbuilder publish config/frontend-forge
```

## Configure

The extension root values file wraps the reused chart by dependency name:

```yaml
frontend-forge:
  extensionController:
    enabled: true
  extensionApi:
    enabled: true
```

For direct Helm installs, continue to use `config/charts/frontend-forge`.
