# Frontend Forge

Frontend Forge 为 KubeSphere 前端扩展提供 package、publish、API 服务和旧
`FrontendIntegration` 迁移所需的控制器与 Job。

该目录是 KubeSphere extension wrapper。实际安装的 Helm chart 位于
`charts/frontend-forge`，这样 `ksbuilder` 可以直接打包；旧的
`config/charts/frontend-forge` 路径是指向同一 chart 的兼容 symlink。

## 打包或发布

```bash
ksbuilder package config/frontend-forge
ksbuilder publish config/frontend-forge
```

## 配置

extension 根目录的 values 需要按 dependency name 包一层：

```yaml
frontend-forge:
  migration:
    fiToFe:
      enabled: false
  extensionController:
    enabled: true
  extensionApi:
    enabled: true
```

wrapper 默认关闭 FI-to-FE migration hook，用于 KubeSphere extension fresh install。
直接 Helm 安装仍保留 chart 默认行为。

如果直接使用 Helm 安装，继续使用 `config/charts/frontend-forge`。
