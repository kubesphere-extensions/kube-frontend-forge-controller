# Manifest Renderer

Code owner: `crates/manifest`

Source of truth: `crates/manifest/src/lib.rs`,
`crates/manifest/src/v1.rs`

## Status

| Capability | Status | Code path |
| --- | --- | --- |
| FI manifest render | Implemented | `render_extension_manifest` |
| FE manifest render | Implemented | `render_frontend_extension_manifest` |
| FI semantic validation | Implemented | `validate_frontend_integration` |
| FE semantic validation | Implemented | `validate_frontend_extension` |
| Resolved FE page list for package RoleTemplate generation | Implemented | `resolve_frontend_extension_pages` |
| Renderer versions beyond v1 | Planned / TODO | Not implemented |

## Render Input Mapping

| Field | FI source | FE source |
| --- | --- | --- |
| `name` | FI `metadata.name` | `spec.package.name` or FE `metadata.name` |
| `displayName` | `spec.displayName` | `source.inline.frontend.displayName`, then localized package display name |
| `description` | `metadata.annotations["kubesphere.io/description"]` | localized package description |
| `schema_version` | `spec.builder.engineVersion` | `source.inline.schemaVersion` |
| `route_namespace` | `frontendintegrations` | `frontendextensions` |
| `locales` | `spec.locales` | `source.inline.frontend.locales` |
| `menus` | `spec.menus` | `source.inline.frontend.menus` |
| `pages` | `spec.pages` | `source.inline.frontend.pages` |

Supported renderer aliases: `v1`, `v1alpha1`, `1`, `1.0`; missing or empty
version resolves to `v1`.

## Output Shape

Status: Implemented

```typescript
export type ExtensionManifest = {
  version: "1.0";
  name: string;
  displayName: string;
  description?: string;
  routes: { path: string; pageId: string }[];
  menus: {
    parent: string;
    name: string;
    title: string;
    icon: string;
    order: 999;
  }[];
  locales: { lang: string; messages: Record<string, string> }[];
  pages: {
    id: string;
    entryComponent: string;
    componentsTree: Record<string, unknown>;
  }[];
  build: {
    target: "kubesphere-extension";
    moduleName: string;
    systemjs: true;
  };
};
```

`manifest_content_and_hash` canonicalizes JSON object keys before hashing.

## Menu And Route Rules

Status: Implemented

| Input | Route suffix | Menu `name` | Menu `parent` | Route path |
| --- | --- | --- | --- | --- |
| Top-level `type=page` | `<key>` | `<route_namespace>/<name>/<key>` | `<placement>` | `<placement prefix>/<route_namespace>/<name>/<key>` |
| Top-level `type=organization` | none | `<route_namespace>/<name>/<key>` | `<placement>` | none |
| Child page under organization | `<parent-key>/<child-key>` | `<route_namespace>/<name>/<parent-key>/<child-key>` | `<placement>.<org menu name>` | `<placement prefix>/<route_namespace>/<name>/<parent-key>/<child-key>` |

Placement prefixes:

| Placement | Prefix |
| --- | --- |
| `cluster` | `/clusters/:cluster` |
| `workspace` | `/workspaces/:workspace` |
| `global` | empty string |

Menu icon behavior:

- `icon` is copied when present.
- Missing icon renders as `GridDuotone`.
- `order` is always `999`.

## Page ID Rules

Status: Implemented

```text
<name>-<placement>-<route-suffix-with-slashes-replaced-by-underscores>
```

Examples:

| Source | Page ID |
| --- | --- |
| FI `demo-fi`, cluster `overview` | `demo-fi-cluster-overview` |
| FI `demo-fi`, workspace `ops/inspecttasks` | `demo-fi-workspace-ops_inspecttasks` |
| FE package `inspecttask`, cluster `inspecttasks` | `inspecttask-cluster-inspecttasks` |

## Validation Rules

Status: Implemented

| Error | Trigger |
| --- | --- |
| `DuplicateTopLevelMenuKey` | Duplicate primary `menus[].key`. |
| `DuplicatePageKey` | Duplicate `pages[].key` or duplicate page menu binding for `(placement,key)`. |
| `MissingPageForMenuKey` | Page menu key has no matching `pages[].key`. |
| `OrphanPageConfig` | Page config is not bound by any page menu. |
| `InvalidMenuShape` | `page` menu has children; `organization` has no children; organization binds page config. |
| `InvalidMenuKey` | Menu key is empty, starts/ends with `-`, or contains chars outside `[a-z0-9-]`. |
| `InvalidPageShape` | Page key invalid; page type config missing; page defines config for the wrong type. |
| `MissingCrdColumns` | `crdTable.columns` is empty. |
| `UnsupportedEngineVersion` | FI `builder.engineVersion` is not a v1 alias. |
| `UnsupportedSchemaVersion` | FE `source.inline.schemaVersion` is not a v1 alias. |

Page key format implemented by code:

```text
non-empty; no leading/trailing "-"; only ASCII lowercase letters, digits, "-"
```

## Iframe Page Output

Status: Implemented

| Output field | Value |
| --- | --- |
| `id` / `entryComponent` | Page ID. |
| `componentsTree.meta.id` / `name` | Page ID. |
| `componentsTree.meta.title` | Bound menu display name. |
| `componentsTree.meta.path` | `/<pageId>`. |
| `root.type` | `Iframe`. |
| `root.props.FRAME_URL` | `iframe.src`. |

## CRD Table Page Output

Status: Implemented

| Output field | Value |
| --- | --- |
| `root.type` | `CrdTable`. |
| `root.props.TABLE_KEY` | Page ID. |
| `root.props.TITLE` | Bound menu display name. |
| `root.props.AUTH_KEY` | `crdTable.authKey` or empty string. |
| `dataSources[0].type` | `crd-columns`. |
| `dataSources[1].type` | `workspace-crd-page-state` for `workspace`; otherwise `crd-page-state`. |
| `dataSources[1].config.CRD_CONFIG.apiVersion` | `crdTable.version`. |
| `dataSources[1].config.CRD_CONFIG.group` | `crdTable.group`. |
| `dataSources[1].config.CRD_CONFIG.plural` | `crdTable.names.plural`. |
| `dataSources[1].config.CRD_CONFIG.kind` | Present only when `crdTable.names.kind` is present. |
| `dataSources[1].config.SCOPE` | `namespace` / `cluster`, omitted for `workspace` placement. |
| `CREATE_INITIAL_VALUE.kind` | Present only when `crdTable.names.kind` is present. |
| `CREATE_INITIAL_VALUE.metadata.namespace` | Present only when `crdTable.scope=Namespaced`. |

Column transform:

| Input | Output |
| --- | --- |
| `key` | `key` |
| `title` | `title` |
| `render.type` | `render.type` as `text`, `time`, or `link` |
| `render.path` | `render.path` |
| `render.payload` | Base `render.payload`, default `{}` |
| `render.format` / `pattern` / `link` | Added into `render.payload` when present |
| `enableSorting` / `enableHiding` | Added only when present |

## Minimal FI Example

Status: Implemented

```yaml
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendIntegration
metadata:
  name: demo-fi
spec:
  displayName: Demo FI
  menus:
    - displayName: Demo
      key: demo
      placement: global
      type: page
  pages:
    - key: demo
      type: iframe
      iframe:
        src: http://example.test/frontend
  builder:
    engineVersion: v1
```

Rendered route:

```json
{
  "path": "/frontendintegrations/demo-fi/demo",
  "pageId": "demo-fi-global-demo"
}
```

## Minimal FE Example

Status: Implemented

```yaml
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendExtension
metadata:
  name: inspecttask
spec:
  package:
    name: inspecttask
    version: 0.1.0
    displayName:
      en: Inspect Task
    description:
      en: InspectTask extension package
  source:
    type: Inline
    inline:
      schemaVersion: v1
      frontend:
        menus:
          - displayName: Inspect Tasks
            key: inspecttasks
            placement: cluster
            type: page
        pages:
          - key: inspecttasks
            type: iframe
            iframe:
              src: http://example.test
```

Rendered route:

```json
{
  "path": "/clusters/:cluster/frontendextensions/inspecttask/inspecttasks",
  "pageId": "inspecttask-cluster-inspecttasks"
}
```

## TODO / Open Question

Status: Planned / TODO

- Renderer versions beyond v1 are not implemented.
- Renderer validation is semantic; Rust CRD structs still allow some invalid shapes until the shared validator runs.
