# CRD Types

Code owner: `crates/api`

Source of truth: `crates/api/src/fi.rs`, `crates/api/src/fe.rs`,
`crates/api/src/lib.rs`

## Resource Summary

| Resource | Status | Scope | Group / Version | Short name | CRD path |
| --- | --- | --- | --- | --- | --- |
| `FrontendIntegration` | Implemented | Cluster | `frontend-forge.kubesphere.io/v1alpha1` | `fi` | `config/charts/frontend-forge/crds/frontend-forge.kubesphere.io_frontendintegrations.yaml` |
| `FrontendExtension` | Implemented | Cluster | `frontend-forge.kubesphere.io/v1alpha1` | `fe` | `config/charts/frontend-forge/crds/frontend-forge.kubesphere.io_frontendextensions.yaml` |
| `JSBundle` | Partially implemented in this repo | Cluster | `extensions.kubesphere.io/v1alpha1` | none | Optional chart template `templates/jsbundle-crd.yaml` |

FI and FE CRDs are generated from Rust structs and labeled
`kubesphere.io/resource-served: "true"`. `JSBundle` is an external KubeSphere
resource; this repo only carries Rust types and an optional local/e2e CRD.

## FrontendIntegration

Status: Implemented

`FrontendIntegration` is the FI runtime source object. The runtime controller is
implemented but Helm disables it by default with `controller.enabled=false`.

### Spec Fields

| Field | Type | Required by Rust type | Default / compatibility | Behavior |
| --- | --- | --- | --- | --- |
| `displayName` | string | no | omitted | Manifest `displayName`; renderer falls back to `metadata.name`. |
| `locales` | map `<lang, map<string,string>>` | no | `{}` | Rendered to manifest `locales`. |
| `enabled` | boolean | no | `true` | Runtime switch. Excluded from `spec_hash`. |
| `menus` | `PrimaryMenuSpec[]` | yes | none | Two-level menu source. |
| `pages` | `PageSpec[]` | yes | none | Page config list bound by `key`. |
| `builder.engineVersion` | string | no | renderer treats missing/empty as `v1` | Supported values: `v1`, `v1alpha1`, `1`, `1.0`. |

### Menu Fields

| Field | Type | Applies to | Behavior |
| --- | --- | --- | --- |
| `displayName` | string | primary and secondary | Menu title and page title source. |
| `key` | string | primary and secondary | Kebab-case route fragment; validated by renderer. |
| `icon` | string | primary and secondary | Optional; renderer falls back to `GridDuotone`. |
| `placement` | `global` / `workspace` / `cluster` | primary | Route prefix and menu parent. |
| `type` | `page` / `organization` | primary | `page` binds directly; `organization` groups secondary pages. |
| `children` | `SecondaryMenuSpec[]` | primary `organization` | Required for `organization`, forbidden for `page`. |

Implemented validation in `crates/manifest`:

- Top-level menu keys are unique among top-level menus.
- Page menu bindings are unique by `(placement, key)`.
- `type=page` cannot define `children`.
- `type=organization` must define at least one child.
- `type=organization` cannot bind to a page config with the same key.
- Only two menu levels are implemented.

### Page Fields

| Field | Type | Behavior |
| --- | --- | --- |
| `key` | string | Binds one page config to one page menu key. |
| `type` | `iframe` / `crdTable` | Selects the required page config field. |
| `iframe.src` | string | Iframe URL. `url` is accepted as a serde alias. |
| `crdTable.names.kind` | string | Optional; used in CRD config and create initial value when present. |
| `crdTable.names.plural` | string | Required CRD plural. |
| `crdTable.group` | string | Required API group. |
| `crdTable.version` | string | Required API version. |
| `crdTable.authKey` | string | Optional permission action key. |
| `crdTable.scope` | `Namespaced` / `Cluster` | CRD object scope for page state and generated permissions. |
| `crdTable.columns` | `ColumnSpec[]` | Required non-empty list for `crdTable`. |

Column fields:

| Field | Type | Behavior |
| --- | --- | --- |
| `key` | string | Column key. |
| `title` | string | Column title. |
| `enableSorting` | boolean | Optional, forwarded to manifest column config. |
| `enableHiding` | boolean | Optional, forwarded to manifest column config. |
| `render.type` | `text` / `time` / `link` | Renderer type. |
| `render.path` | string | Source path. |
| `render.format` | string | Optional payload entry. |
| `render.pattern` | string | Optional payload entry. |
| `render.link` | string | Optional payload entry. |
| `render.payload` | object | Optional payload base object. |

### Status Fields

| Field | Type | Behavior |
| --- | --- | --- |
| `phase` | `Pending` / `Building` / `Succeeded` / `Failed` | Runtime build phase. Disabled FI is patched to `Pending` with message `Disabled`. |
| `observed_spec_hash` | string | Hash of `spec.without_enabled()`. |
| `observed_manifest_hash` | string | Deprecated compatibility field retained by controller and runner. |
| `observed_generation` | integer | Source generation observed by controller/runner. |
| `last_build.job_ref` | `ResourceRef` | Last selected build Job. |
| `last_build.started_at` | timestamp | Build start timestamp tracked by controller. |
| `bundle_ref` | `ResourceRef` | Desired or observed `JSBundle` reference. |
| `last_error` | `LastBuildError` | Runner or Job failure details. |
| `message` | string | Human-readable controller state. |
| `conditions` | `SimpleCondition[]` | Type exists; FI controller currently writes an empty list. |

## FrontendExtension

Status: Implemented

`FrontendExtension` is the package/download/publish source object. Helm installs
the FE controller and FE HTTP API by default.

### Spec Fields

| Field | Type | Required by Rust type | Behavior |
| --- | --- | --- | --- |
| `package` | `FrontendExtensionPackageSpec` | yes | Extension package metadata and chart values. |
| `source.type` | `Inline` | yes | Only `Inline` is implemented. |
| `source.inline.schemaVersion` | string | yes | Renderer schema version; supported values match FI v1 renderer aliases. |
| `source.inline.frontend` | `FrontendExtensionFrontendSpec` | yes | Menu/page/locales source shared with FI schema. |
| `source.inline.extensionResources.jsBundle.name` | string | no | Type exists for compatibility; current package renderer does not use it. |
| `publishPolicy` | `PublishPolicySpec` | no | Manual publish target defaults. |

### Package Fields

| Field | Type | Required by Rust type | Behavior |
| --- | --- | --- | --- |
| `name` | string | no | Package name; defaults to FE `metadata.name`. |
| `version` | string | yes | Package version and archive filename suffix. |
| `displayName` | map `<lang,string>` | yes | Package display name; renderer uses `en`, then `zh`, then first value for manifest fallback. |
| `description` | map `<lang,string>` | yes | Package description and manifest description fallback. |
| `category` | string | no | Written to generated `extension.yaml` when present. |
| `keywords` | string[] | no | Written to package metadata. |
| `sources` | string[] | no | Written to package metadata and frontend chart. |
| `kubeVersion` | string | no | Written to package metadata. |
| `ksVersion` | string | no | Written to package metadata. |
| `maintainers` | list | no | Written to package metadata. |
| `home` | string | no | Written to package metadata and frontend chart. |
| `provider` | map `<lang, provider>` | no | Written to package metadata. |
| `icon` | string | no | Written to package metadata. |
| `staticFileDirectory` | string | no | Written to package metadata. |
| `dependencies` | list | no | Defaults to `<package>-helper` with tag `agent` and `frontend` with tag `extension`. |
| `installationMode` | string | no | Written to package metadata. |
| `images` | string[] | no | Written to package metadata. |
| `charts.values` | object | no | Root `values.yaml`; helper `roleTemplate.enabled` defaults to `true`. |

### Publish Policy

Status: Implemented

| Field | Type | Behavior |
| --- | --- | --- |
| `mode` | `Manual` | Only `Manual` is implemented. |
| `defaultTargetKind` | `ConfigMap` / `Secret` | Defaults to `ConfigMap` in controller/API behavior when no annotation override exists. |
| `defaultTargetRef.namespace` | string | Target namespace. |
| `defaultTargetRef.name` | string | Target object name. |
| `defaultTargetRef.uid` | string | Optional, not required by publish controller. |

Publish request annotations override spec target fields:

| Annotation | Behavior |
| --- | --- |
| `frontend-forge.io/publish-request-id` | Triggers publish reconciliation. |
| `frontend-forge.io/publish-artifact-digest` | Required requested artifact digest. |
| `frontend-forge.io/publish-target-kind` | Optional target kind override. |
| `frontend-forge.io/publish-target-namespace` | Optional target namespace override. |
| `frontend-forge.io/publish-target-name` | Optional target name override. |

### Status Fields

| Field | Type | Behavior |
| --- | --- | --- |
| `phase` | `Pending` / `Packaging` / `Ready` / `Failed` | Package source and artifact phase. |
| `observedGeneration` | integer | Observed FE generation. |
| `observedSourceHash` | string | Normalized package/source hash. |
| `observedRebuildToken` | string | Current `frontend-forge.kubesphere.io/rebuild-token` annotation value. |
| `artifact.storage.kind` | `ConfigMap` | Only ConfigMap artifact storage is implemented. |
| `artifact.storage.ref` | `NamespacedResourceRef` | Artifact ConfigMap reference. |
| `artifact.storage.key` | string | Binary package key, currently `package.tgz`. |
| `artifact.digest` | string | Package tarball digest, `sha256:<hex>`. |
| `artifact.sizeBytes` | integer | Package byte size. |
| `artifact.mediaType` | string | `application/gzip`. |
| `artifact.filename` | string | `<package-name>-<version>.tgz`. |
| `artifact.generatedAt` | timestamp | Package generation time. |
| `artifact.sourceHash` | string | Source hash used for this artifact. |
| `artifact.artifactKey` | string | Hash of source hash plus rebuild token. |
| `download.ready` | boolean | HTTP download readiness. |
| `download.filename` | string | Download filename. |
| `download.mediaType` | string | Download media type. |
| `packageJob` | `PackageJobStatus` | Current or last package Job status. |
| `publish` | `PublishStatus` | Current publish request status for current artifact key. |
| `conditions` | `ExtensionCondition[]` | Controller writes `SourceValid`, `ArtifactReady`, `DownloadReady`, `PublishSucceeded`. |

`PublishStatus` fields:

| Field | Type | Behavior |
| --- | --- | --- |
| `phase` | `NotRequested` / `Pending` / `Running` / `Succeeded` / `Failed` | Publish Job phase or default. |
| `requestId` | string | Publish request id. |
| `artifactDigest` | string | Requested artifact digest. |
| `jobRef` | `NamespacedResourceRef` | Publisher Job reference. |
| `startedAt` | timestamp | Job start time. |
| `finishedAt` | timestamp | Job completion time. |
| `lastError` | string | Job failure message. |

## JSBundle

Status: Partially implemented

`JSBundle` Rust types are used by the FI runtime controller and runner. The CRD
is normally provided by KubeSphere. The chart can install a local/e2e CRD with
`crds.installJsBundle=true`.

| Field | Type | Behavior |
| --- | --- | --- |
| `spec.raw` | string | Inline bundle content; not written by FI runner. |
| `spec.rawFrom.configMapKeyRef` | namespaced key ref | FI runner writes this reference. |
| `spec.rawFrom.secretKeyRef` | namespaced key ref | Type exists; not written by FI runner. |
| `spec.rawFrom.url` | string | Type exists; not written by FI runner. |
| `status.state` | string | Runner/controller set `Available`; controller sets `Disabled` when FI is disabled. |
| `status.link` | string | Runner writes `/dist/<jsbundle>/<key>`. |
| `status.conditions` | array | Type exists; runner writes an empty list. |

## TODO / Open Question

Status: Planned / TODO

- The external production owner of the `JSBundle` CRD is outside this repository.
- Rust types do not enforce semantic validation such as key format or page binding; those rules live in `crates/manifest`.
