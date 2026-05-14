# FrontendExtension Package And Publish

Code owners: `crates/frontend-extension-controller`,
`crates/extension-packager`, `crates/extension-publisher`,
`crates/extension-package-core`, `crates/frontend-forge-extension-api`

Source of truth: the code owners above plus `crates/api/src/fe.rs`

## Status

| Capability | Status | Code path |
| --- | --- | --- |
| `FrontendExtension` CRD | Implemented | `crates/api/src/fe.rs` |
| Source validation through shared manifest renderer | Implemented | `frontend_forge_manifest::validate_frontend_extension` |
| Package controller | Implemented | `crates/frontend-extension-controller` |
| Package Job binary | Implemented | `crates/extension-packager` |
| Package artifact in ConfigMap | Implemented | `crates/extension-packager`, controller status sync |
| HTTP list/get/create/download/publish API | Implemented | `crates/frontend-forge-extension-api` |
| Publish Job binary | Implemented | `crates/extension-publisher` |
| Install controller for generated package | Planned / TODO | Not implemented |
| Artifact storage outside ConfigMap | Planned / TODO | Not implemented |

## Controller Configuration

Status: Implemented

| Env | Helm value | Binary default | Behavior |
| --- | --- | --- | --- |
| `WORK_NAMESPACE` | release namespace | `extension-frontend-forge` | Package/publish Job namespace. |
| `PACKAGER_IMAGE` | `extensionController.packagerImage` or `extensionPackager.image.*` | `kubesphere/frontend-forge-extension-packager:latest` | Package Job image. |
| `PACKAGER_SERVICE_ACCOUNT` | `extensionController.packagerServiceAccountName` or packager SA | unset | Package Job service account. |
| `PUBLISHER_IMAGE` | `extensionController.publisherImage` or `extensionPublisher.image.*` | `kubesphere/frontend-forge-extension-publisher:latest` | Publish Job image. |
| `PUBLISHER_SERVICE_ACCOUNT` | `extensionController.publisherServiceAccountName` or publisher SA | unset | Publish Job service account. |
| `ARTIFACT_CONFIGMAP_NAMESPACE` | `extensionController.artifactConfigmapNamespace` | `WORK_NAMESPACE` | Artifact ConfigMap namespace. |
| `BUILD_SERVICE_BASE_URL` | `extensionController.buildServiceBaseUrl` | `http://frontend-forge.extension-frontend-forge.svc` | Package Job build-service endpoint. |
| `BUILD_SERVICE_TIMEOUT_SECONDS` | `extensionController.buildServiceTimeoutSeconds` | `240` | Package Job HTTP timeout. |
| `JSBUNDLE_CONFIG_KEY` | `extensionController.jsbundleConfigKey` | `index.js` | Preferred frontend bundle artifact. |
| `RECONCILE_REQUEUE_SECONDS` | `extensionController.reconcileRequeueSeconds` | `5` | Job polling delay. |
| `JOB_ACTIVE_DEADLINE_SECONDS` | `extensionController.jobActiveDeadlineSeconds` | `300` | Package/publish/unpublish Job active deadline. |
| `JOB_TTL_SECONDS_AFTER_FINISHED` | `extensionController.jobTtlSecondsAfterFinished` | `3600` | Package/publish Job TTL. |
| `ARTIFACT_RETAIN_OLD_COUNT` | `extensionController.artifactRetainOldCount` | `1` | Stale artifact ConfigMap retention count. |
| `PACKAGE_MAX_ATTEMPTS` | `extensionController.packageMaxAttempts` | `3` | Max package attempts per artifact key. |

## Source Identity

Status: Implemented

`frontend_extension_source_hash` hashes normalized package/source identity.

Included:

- Effective package name: `spec.package.name` or FE `metadata.name`.
- Package metadata fields in `FrontendExtensionPackageSpec`.
- Effective dependencies. Missing `spec.package.dependencies` normalizes to:
  - `<package-name>-helper` with tag `agent`
  - `frontend` with tag `extension`
- `source.type`.
- `source.inline.schemaVersion`.
- `source.inline.frontend`.

Excluded:

- `spec.publishPolicy`.
- `source.inline.extensionResources`.
- Publish request annotations.
- Rebuild token annotation.

Artifact key:

```text
sha256({ keyVersion: "v1", sourceHash, rebuildToken })
```

`rebuildToken` comes from annotation
`frontend-forge.kubesphere.io/rebuild-token`; missing or blank token is `""`.

## Package Reconcile

Status: Implemented

Controller watches cluster-scoped FE, Jobs in `WORK_NAMESPACE`, and artifact
ConfigMaps in `ARTIFACT_CONFIGMAP_NAMESPACE`.

| Step | Behavior |
| --- | --- |
| Deletion | FE with `deletionTimestamp` is ignored. |
| Hash | Computes source hash and artifact key. |
| Validate source | Uses shared FE manifest renderer. Invalid source writes `phase=Failed`; no package Job. |
| Artifact exists | If matching ConfigMap metadata exists, status becomes `Ready`; publish sync runs. |
| Latest package Job pending/running | Status becomes `Packaging`; controller requeues. |
| Latest package Job failed | Creates next attempt until `PACKAGE_MAX_ATTEMPTS`; then `Failed`. |
| Latest package Job succeeded but artifact missing/mismatched | Creates next attempt until max attempts; then `Failed`. |
| No usable artifact/job | Creates package Job attempt `a<N>`. |
| Artifact GC | Keeps current artifact, artifact referenced by status, and `ARTIFACT_RETAIN_OLD_COUNT` old owned ConfigMaps. |

Package Job name:

```text
fe-<fe-name>-package-<artifact-key-12>-a<attempt>
```

bounded to 63 chars while preserving the suffix.

## Package Job

Status: Implemented

Package Job environment:

| Env | Source |
| --- | --- |
| `FE_NAME` | FE name |
| `FE_UID` | FE UID |
| `SOURCE_HASH` | Controller-computed source hash |
| `ARTIFACT_KEY` | Controller-computed artifact key |
| `REBUILD_TOKEN` | Effective rebuild token |
| `ARTIFACT_CONFIGMAP_NAMESPACE` | Controller config |
| `ARTIFACT_CONFIGMAP_NAME` | Controller-computed artifact ConfigMap name |
| `BUILD_SERVICE_BASE_URL` | Controller config |
| `BUILD_SERVICE_TIMEOUT_SECONDS` | Controller config |
| `JSBUNDLE_CONFIG_KEY` | Controller config |

Package Job flow:

| Step | Behavior |
| --- | --- |
| Read FE | Reads cluster-scoped FE by `FE_NAME`. |
| Stale check | Recomputes source hash and fails if it differs from `SOURCE_HASH`. |
| Render manifest | Calls `render_frontend_extension_manifest`. |
| Build frontend | Calls build-service with canonical manifest JSON. |
| Select bundle | Uses `JSBUNDLE_CONFIG_KEY` to select bundle artifact. |
| Build package | Calls `build_extension_package`. |
| Upsert ConfigMap | Creates or replaces artifact ConfigMap. |

Artifact ConfigMap:

| Field | Value |
| --- | --- |
| Name | `fe-<package-name>-<artifact-key-12>`, bounded to 63 chars. |
| Namespace | `ARTIFACT_CONFIGMAP_NAMESPACE`. |
| Owner reference | FE owner reference. |
| `binaryData["package.tgz"]` | Generated extension package archive. |
| `data["artifact.json"]` | Artifact metadata JSON. |
| `data["files.json"]` | Package file metadata JSON. |

Artifact annotations:

| Annotation | Value |
| --- | --- |
| `frontend-forge.kubesphere.io/source-hash` | Source hash. |
| `frontend-forge.kubesphere.io/artifact-key` | Artifact key. |
| `frontend-forge.io/artifact-digest` | Package digest. |
| `frontend-forge.io/artifact-filename` | Package filename. |

## Package Contents

Status: Implemented

Generated files include:

| Path | Source |
| --- | --- |
| `extension.yaml` | FE package metadata. |
| `permissions.yaml` | Embedded package template. |
| `values.yaml` | `spec.package.charts.values` plus helper `roleTemplate.enabled=true` default. |
| `README.md`, `README_zh.md` | Generated placeholders using package name. |
| `static/favicon.svg` | Embedded package template. |
| `charts/frontend/Chart.yaml` | Generated frontend chart metadata. |
| `charts/frontend/values.yaml` | Embedded package template. |
| `charts/frontend/scripts/index.js` | Build-service selected bundle content. |
| `charts/frontend/templates/*` | Embedded frontend chart templates. |
| `charts/<package>-helper/Chart.yaml` | Generated helper chart metadata. |
| `charts/<package>-helper/values.yaml` | Embedded helper values template. |
| `charts/<package>-helper/templates/roleTemplate.yaml` | Generated RoleTemplate template. |

`extension.yaml` uses API version `kubesphere.io/v1alpha1`.

## RoleTemplate Generation

Status: Implemented

RoleTemplate input comes from resolved FE frontend pages.

| Page type | Scope rule | View permission | Manage permission |
| --- | --- | --- | --- |
| `crdTable`, placement `cluster` | cluster | CRD verbs `get,list,watch` | CRD verbs `*` |
| `crdTable`, placement `workspace` | namespace | CRD verbs `get,list,watch` | CRD verbs `*` |
| `crdTable`, placement `global`, CRD scope `Cluster` | cluster | CRD verbs `get,list,watch` | CRD verbs `*` |
| `crdTable`, placement `global`, CRD scope `Namespaced` | namespace | CRD verbs `get,list,watch` | CRD verbs `*` |
| `iframe`, placement `cluster` | cluster | action key only, no Kubernetes rule | none |
| `iframe`, placement `workspace` | namespace | action key only, no Kubernetes rule | none |
| `iframe`, placement `global` | none | no RoleTemplate contribution | none |

Action key:

- `crdTable.authKey` when set.
- Otherwise the resolved page action key.

Generated Category names:

| Scope | Category |
| --- | --- |
| cluster | `cluster-fe-management` |
| namespace | `namespace-fe-management` |

Manage RoleTemplate depends on corresponding view RoleTemplate.

## FE Status

Status: Implemented

| Phase | Trigger |
| --- | --- |
| `Pending` | Default enum value before controller writes status. |
| `Packaging` | Package Job created/running. |
| `Ready` | Matching artifact ConfigMap exists and metadata matches source hash/artifact key. |
| `Failed` | Source invalid, package attempts exceeded, or artifact remained missing/mismatched after max attempts. |

Conditions written by controller:

| Type | True reason | False reason examples |
| --- | --- | --- |
| `SourceValid` | `Validated` | `InvalidSource` |
| `ArtifactReady` | `Generated` | `Packaging`, `PackageAttemptsExceeded`, `InvalidSource` |
| `DownloadReady` | `Available` | `ArtifactNotReady` |
| `PublishSucceeded` | `Succeeded` | `NotRequested`, `PublishFailed` |

Status labels written by controller for frontend list filtering:

| Label | Values | Meaning |
| --- | --- | --- |
| `frontend-forge.kubesphere.io/package-state` | `packaging`, `ready`, `failed` | Package creation state. `Pending` status is exposed as `packaging`. |
| `frontend-forge.kubesphere.io/publish-state` | `not-published`, `publishing`, `published`, `failed` | Publish state. `published` requires `status.publish.phase=Succeeded` and `active=true`; inactive succeeded publish is exposed as `not-published`. |

## Extension HTTP API

Status: Implemented

Binary: `crates/frontend-forge-extension-api`

Default bind address: `0.0.0.0:8080`.

Routes are registered for:

- `/apis/frontend-forge.kubesphere.io/v1alpha1/frontendextensions`
- `/apis/<EXTENSION_API_GROUP>/<EXTENSION_API_VERSION>/frontendextensions`
- `/kapis/<EXTENSION_API_GROUP>/<EXTENSION_API_VERSION>/frontendextensions`

Default extension API group/version:

| Env | Default |
| --- | --- |
| `EXTENSION_API_GROUP` | `frontend-forge-api.kubesphere.io` |
| `EXTENSION_API_VERSION` | `v1alpha1` |

API operations:

| Method | Path suffix | Behavior |
| --- | --- | --- |
| `GET` | `/frontendextensions` | Lists FE summaries. Supports `?labelSelector=frontend-forge.kubesphere.io/package-state=ready,frontend-forge.kubesphere.io/publish-state=not-published`. |
| `POST` | `/frontendextensions` | Creates a cluster-scoped `FrontendExtension` through Kubernetes API. |
| `GET` | `/frontendextensions/{name}` | Returns full FE object. |
| `GET` | `/frontendextensions/{name}/download` | Returns current ready `package.tgz`. |
| `GET` | `/frontendextensions/{name}/publish` | Returns `status.publish` or default `NotRequested`. |
| `POST` | `/frontendextensions/{name}/publish` | Patches publish request annotations and returns `202 Accepted`. |
| `GET` | `/frontendextensions/{name}/unpublish` | Returns `status.unpublish` or default `NotRequested`. |
| `POST` | `/frontendextensions/{name}/unpublish` | Patches unpublish request annotations and returns `202 Accepted`. |
| `POST` | `/frontendextensions/{name}/delete` | Deletes FE directly, or triggers unpublish first when requested and currently published. |

List summary fields:

| Field | Source |
| --- | --- |
| `name` | FE name |
| `generation` | FE metadata generation |
| `package.version` | `spec.package.version` |
| `package.displayName` | `spec.package.displayName` |
| `phase` | `status.phase` or default |
| `artifactDigest` | `status.artifact.digest` |
| `download` | `status.download.ready`, `filename` |
| `publish` | `status.publish` or default |

Publish request body:

```json
{
  "requestId": "manual-1",
  "expectedArtifactDigest": "sha256:..."
}
```

Unpublish request body:

```json
{
  "requestId": "manual-1"
}
```

Delete request body:

```json
{
  "unpublish": true
}
```

Publish API behavior:

| Case | Status code | Behavior |
| --- | --- | --- |
| FE missing | `404` | JSON error response. |
| Artifact not ready | `409` | JSON error response. |
| `expectedArtifactDigest` mismatch | `409` | No annotations patched. |
| Missing publish target ref | `409` | No annotations patched. |
| Target kind not `ConfigMap` or `Secret` | `409` | No annotations patched. |
| Existing same `requestId` and digest in status | `202` | Returns current publish status. |
| Accepted new request | `202` | Patches publish annotations. |
| Kubernetes API error | `500` | JSON error response. |

Unpublish and delete API behavior:

| Case | Status code | Behavior |
| --- | --- | --- |
| FE missing | `404` | JSON error response. |
| Unpublish target ref missing | `409` | No annotations patched. |
| Unpublish target kind invalid | `409` | No annotations patched. |
| Accepted unpublish request | `202` | Patches unpublish annotations. |
| `POST /delete` with `unpublish=false` | `200` | Deletes FE CR directly. |
| `POST /delete` when not currently published | `200` | Deletes FE CR directly. |
| `POST /delete` when `status.publish.phase=Succeeded` and `status.publish.active=true` | `202` | Patches unpublish annotations and `delete-after-unpublish-request-id`. |
| Kubernetes API error | `500` | JSON error response. |

Download API behavior:

| Case | Status code | Behavior |
| --- | --- | --- |
| FE missing | `404` | JSON error response. |
| FE not `Ready` | `409` | JSON error response. |
| Download not ready | `409` | JSON error response. |
| Artifact source hash mismatch | `409` | JSON error response. |
| Artifact ConfigMap missing key | `500` | JSON error response. |
| Digest mismatch | `500` | JSON error response. |
| Success | `200` | `Content-Type` from artifact status, `Content-Disposition: attachment`. |

## Publish Reconcile

Status: Implemented

Publish trigger source is FE annotations, normally patched by extension API or
FI-to-FE migrator through the API.

| Step | Behavior |
| --- | --- |
| No request id | Keeps current publish if it matches current artifact; otherwise default `NotRequested`. |
| Requested digest missing | Writes `publish.phase=Failed`. |
| Requested digest mismatches current artifact | Writes `publish.phase=Failed`. |
| Target ref missing | Writes `publish.phase=Failed`. |
| Target kind invalid | Writes `publish.phase=Failed`. |
| Job exists | Maps Job phase to publish status. |
| Same request already terminal for current artifact key | Keeps existing terminal status. |
| New request | Creates publisher Job. |
| Publish Job succeeded | Writes `publish.phase=Succeeded` and `publish.active=true`. |

Unpublish trigger source is FE annotations, normally patched by extension API.
Unpublish success writes `status.unpublish.phase=Succeeded` and changes
`status.publish.active` to `false`.

Direct `kubectl delete fe <name>` does not automatically run unpublish. Use
`POST /frontendextensions/{name}/delete` with `{"unpublish":true}` when the
extension should be unpublished before the FE CR is removed.

Unpublish reconcile:

| Step | Behavior |
| --- | --- |
| No request id | Keeps current `status.unpublish`. |
| Target ref missing | Writes `unpublish.phase=Failed`. |
| Target kind invalid | Writes `unpublish.phase=Failed`. |
| Job exists | Maps Job phase to unpublish status. |
| Same request already terminal | Keeps existing terminal status. |
| New request | Creates unpublish Job. |
| Request succeeded and `delete-after-unpublish-request-id` matches | Deletes FE CR. |

Publisher Job name:

```text
fe-<fe-name>-publish-<request-id-hash-short>
```

Unpublish Job name:

```text
fe-<fe-name>-unpublish-<request-id-hash-short>
```

Publisher Job environment:

| Env | Source |
| --- | --- |
| `FE_NAME` | FE name |
| `PUBLISH_REQUEST_ID` | Publish request id |
| `ARTIFACT_DIGEST` | Requested artifact digest |
| `ARTIFACT_CONFIGMAP_NAMESPACE` | Artifact ConfigMap namespace |
| `ARTIFACT_CONFIGMAP_NAME` | Artifact ConfigMap name |
| `ARTIFACT_CONFIGMAP_KEY` | `package.tgz` |
| `ARTIFACT_FILENAME` | Artifact filename |
| `PUBLISH_TARGET_KIND` | `ConfigMap` or `Secret` |
| `PUBLISH_TARGET_NAMESPACE` | Target namespace |
| `PUBLISH_TARGET_NAME` | Target name |

Unpublish Job environment:

| Env | Source |
| --- | --- |
| `FE_NAME` | FE name |
| `PUBLISH_ACTION` | `unpublish` |
| `UNPUBLISH_REQUEST_ID` | Unpublish request id |
| `UNPUBLISH_EXTENSION_NAME` | Effective package name, `spec.package.name` or FE name |
| `PUBLISH_TARGET_KIND` | `ConfigMap` or `Secret` |
| `PUBLISH_TARGET_NAMESPACE` | Target namespace |
| `PUBLISH_TARGET_NAME` | Target name |

## Publisher Binary

Status: Implemented

Binary: `crates/extension-publisher`

Flow:

| Step | Behavior |
| --- | --- |
| Load artifact | Reads `binaryData[ARTIFACT_CONFIGMAP_KEY]` from artifact ConfigMap. |
| Verify digest | Requires `sha256:<hex>` to match `ARTIFACT_DIGEST`. |
| Prepare workdir | Uses `PUBLISH_WORKDIR` or `/tmp/frontend-extension-publish-<request-hash>`. |
| Write archive | Writes artifact package using `ARTIFACT_FILENAME`. |
| Unpack archive | Extracts tarball into `<workdir>/package` using safe child paths. |
| Load target | Reads target ConfigMap `data` + `binaryData`, or Secret `data`. |
| Write target data | Writes non-special keys under `.frontend-forge-publish-target`. |
| Env target data | Keys `env.<NAME>` become process env `<NAME>`. |
| Args target data | Key `args` is split by whitespace and appended to publish args. |
| Kubeconfig | Writes in-cluster kubeconfig when no explicit kubeconfig is configured. |
| Execute publish | Runs `ksbuilder publish <workdir>/package ...`. |
| Execute unpublish | With `PUBLISH_ACTION=unpublish`, runs `ksbuilder unpublish <extension-name> ...`. |

Publisher environment:

| Env | Default |
| --- | --- |
| `PUBLISH_WORKDIR` | `/tmp/frontend-extension-publish-<request-hash>` |
| `PUBLISH_ACTION` | `publish` |
| `ARTIFACT_CONFIGMAP_NAMESPACE` | `extension-frontend-forge` |
| `ARTIFACT_CONFIGMAP_KEY` | `package.tgz` |
| `ARTIFACT_FILENAME` | `package.tgz` |
| `PUBLISH_TARGET_KIND` | Optional; target loading defaults to `ConfigMap` when target name is set. |
| `PUBLISH_TARGET_NAMESPACE` | Optional; defaults to `extension-frontend-forge` when target name is set. |
| `PUBLISH_TARGET_NAME` | Optional. |
| `UNPUBLISH_REQUEST_ID` | Required when `PUBLISH_ACTION=unpublish`. |
| `UNPUBLISH_EXTENSION_NAME` | Required when `PUBLISH_ACTION=unpublish`. |
| `KSBUILDER_BIN` | `ksbuilder` |
| `KSBUILDER_PUBLISH_ARGS` | empty |

## Boundaries

Status: Implemented

- Applying FE does not create a runtime `JSBundle` in the current cluster.
- Package Job does not run `ksbuilder publish`.
- Publisher Job does not create package artifacts.
- HTTP API does not read package bytes from FE spec; it reads the artifact ConfigMap referenced by status.
- ConfigMap is the only artifact storage kind implemented.
- `source.inline.extensionResources` is present in the CRD type but is not part of source hash.

## TODO / Open Question

Status: Planned / TODO

- Install controller for generated package is not implemented.
- Artifact storage backends other than ConfigMap are not implemented.
- Concrete publish target keys depend on the chosen `ksbuilder` and registry configuration; this repo only defines how target data is passed to `ksbuilder publish`.
