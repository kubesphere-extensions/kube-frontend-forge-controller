# FI To FE Migration

Code owner: `crates/frontend-forge-controller/src/bin/fi_to_fe_migrator.rs`

Helm template: `config/charts/frontend-forge/templates/fi-to-fe-migration-job.yaml`

## Status

| Capability | Status | Default Helm state |
| --- | --- | --- |
| Helm hook migrator Job | Implemented | Enabled by `migration.fiToFe.enabled=true` |
| FI list/read/delete | Implemented | Cluster-scoped API |
| FE create/patch | Implemented | Cluster-scoped API |
| Wait for FE Ready | Implemented | Polls FE status |
| Publish enabled FI through FE API | Implemented | Direct HTTP call to FE API |
| Compensation after FI deleted and publish failed | Planned / TODO | Not implemented |

## Job Configuration

Status: Implemented

| Env | Helm value | Binary default | Behavior |
| --- | --- | --- | --- |
| `PACKAGE_VERSION` | `migration.fiToFe.packageVersion` | `0.1.0` | Migrated FE package version. |
| `SCHEMA_VERSION` | `migration.fiToFe.schemaVersion` | `v1` | Fallback FE inline schema version. |
| `READY_TIMEOUT_SECONDS` | `migration.fiToFe.readyTimeoutSeconds` | `600` | CRD readiness, FE Ready, FI deletion timeout. |
| `POLL_INTERVAL_SECONDS` | `migration.fiToFe.pollIntervalSeconds` | `5` | Poll interval. |
| `FE_API_BASE_URL` | `migration.fiToFe.feApiBaseUrl` | chart service URL | Base URL for publish request. |
| `FE_API_INSECURE_SKIP_TLS_VERIFY` | `migration.fiToFe.feApiInsecureSkipTlsVerify` | `false` | HTTP TLS verification option. |
| `FE_API_CA_CERT_PATH` | `migration.fiToFe.feApiCaCertPath` | empty | Optional custom CA path. Missing configured file fails startup. |
| `FE_API_GROUP` | `extensionApi.apiService.group` | `frontend-forge-api.kubesphere.io` | Publish API group. |
| `FE_API_VERSION` | `extensionApi.apiService.version` | `v1alpha1` | Publish API version. |
| `PUBLISH_TARGET_KIND` | `migration.fiToFe.publishTarget.kind` | `ConfigMap` | FE `publishPolicy.defaultTargetKind`. |
| `PUBLISH_TARGET_NAMESPACE` | `migration.fiToFe.publishTarget.namespace` or release namespace | required after chart render | FE `publishPolicy.defaultTargetRef.namespace`. |
| `PUBLISH_TARGET_NAME` | `migration.fiToFe.publishTarget.name` | `ksbuilder-publish-config` | FE `publishPolicy.defaultTargetRef.name`. |

Helm hook:

| Property | Value |
| --- | --- |
| Job name | `<release-name>-fi-to-fe-migrator` |
| Hook | `post-install,post-upgrade` |
| Hook weight | `10` |
| Default `backoffLimit` | `0` |
| Default delete policy | `before-hook-creation` |

## Prerequisites

Status: Implemented

Before scanning FI objects, migrator waits for:

| Resource | Check |
| --- | --- |
| `frontendintegrations.frontend-forge.kubesphere.io` CRD | Established. |
| `frontendextensions.frontend-forge.kubesphere.io` CRD | Established and v1alpha1 status subresource exists. |

FE API Service availability is not preflighted; publish request errors are
reported by the publish HTTP call.

## Per-FI Flow

Status: Implemented

```text
list all FrontendIntegration
for each FI:
  derive FE name
  create or patch migrator-owned FE
  wait for FE phase Ready and status.artifact.digest
  delete FI and wait until it is gone
  if original FI spec.enabled is missing or true:
    POST FE publish API
  else:
    skip publish
```

Failure handling:

- One FI failure is collected and does not stop the scan.
- Job exits non-zero if any FI failed.
- Failed hook Job is retained by default because `hook-failed` is not in
  `hookDeletePolicy`.

## FE Naming

Status: Implemented

| FI name | FE name |
| --- | --- |
| `demo` | `fi-demo` |
| `fi-demo` | `fi-fi-demo` |
| over 60 chars after prefix | `fi-<dns-label-prefix>-<12-char-sha256>` |

Rules:

- Prefix `fi-` is always added.
- Existing `fi-` prefix is not deduplicated.
- Long names use SHA-256 of the original FI name.
- Non DNS-label chars in the sliced prefix become `-`; empty prefix becomes `x`.

## Managed FE Guard

Status: Implemented

Migrator-created FE metadata:

| Metadata | Value |
| --- | --- |
| Label `frontend-forge.io/managed-by` | `frontend-forge-fi-migrator` |
| Annotation `frontend-forge.io/source-fi-name` | Source FI name |
| Annotation `frontend-forge.io/source-fi-uid` | Source FI UID when present |

Existing FE behavior:

| Existing FE state | Behavior |
| --- | --- |
| Missing | Create desired FE. |
| Managed by migrator and source FI name matches | Patch labels, annotations, and spec. |
| Not migrator-owned | Record failure for this FI. |
| Migrator-owned but source FI name differs | Record failure for this FI. |

## Field Mapping

Status: Implemented

| FI source | FE target |
| --- | --- |
| Derived FE name | `metadata.name` |
| Derived FE name | `spec.package.name` |
| `PACKAGE_VERSION` | `spec.package.version` |
| `spec.displayName` trimmed, fallback FI name | `spec.package.displayName.en` |
| `metadata.annotations["kubesphere.io/description"]` trimmed, fallback display name | `spec.package.description.en` |
| `metadata.annotations["kubesphere.io/creator"]` trimmed, fallback `Fi Migration Bot` | `spec.package.provider.en.name` and `spec.package.provider.zh.name` |
| constant `dev-tools` | `spec.package.category` |
| constant `./static/favicon.svg` | `spec.package.icon` |
| empty list | `spec.package.keywords`, `sources`, `maintainers`, `images` |
| null | `kubeVersion`, `ksVersion`, `home`, `staticFileDirectory`, `dependencies`, `installationMode`, `charts` |
| constant `Inline` | `spec.source.type` |
| `spec.builder.engineVersion` trimmed, fallback `SCHEMA_VERSION` | `spec.source.inline.schemaVersion` |
| `spec.displayName` | `spec.source.inline.frontend.displayName` |
| `spec.locales` | `spec.source.inline.frontend.locales` |
| `spec.menus` | `spec.source.inline.frontend.menus` |
| `spec.pages` | `spec.source.inline.frontend.pages` |
| none | `spec.source.inline.extensionResources` is absent |
| constant `Manual` | `spec.publishPolicy.mode` |
| `PUBLISH_TARGET_KIND` | `spec.publishPolicy.defaultTargetKind` |
| `PUBLISH_TARGET_NAMESPACE` / `PUBLISH_TARGET_NAME` | `spec.publishPolicy.defaultTargetRef` |

`spec.enabled` is not copied into FE. It is only used to decide whether publish
is requested after FI deletion.

## Publish Request

Status: Implemented

Migrator publish URL:

```text
POST <FE_API_BASE_URL>/apis/<FE_API_GROUP>/<FE_API_VERSION>/frontendextensions/<fe-name>/publish
```

Default path:

```text
/apis/frontend-forge-api.kubesphere.io/v1alpha1/frontendextensions/<fe-name>/publish
```

Request body:

```json
{
  "requestId": "fi-migration-<fe-name>-<digest-prefix-12>",
  "expectedArtifactDigest": "<current FE artifact digest>"
}
```

Accepted status codes:

- `200 OK`
- `201 Created`
- `202 Accepted`

Other status codes fail this FI migration item.

## Finalizer And Retry Constraints

Status: Implemented

- Migrator deletes the source FI after FE package is Ready.
- It waits until FI no longer exists before publishing.
- If FI deletion is blocked by a finalizer, migration for that FI times out.
- The default Job `backoffLimit=0` avoids hiding partial failure with Job-level retries.

Status: Planned / TODO

- If FI deletion succeeds and publish fails, rerunning the Job will not see the
  deleted FI and will not automatically retry publish.

## Local Verification

Status: Implemented

Run the migrator binary against the current kubeconfig:

```bash
PACKAGE_VERSION=0.1.0 \
SCHEMA_VERSION=v1 \
PUBLISH_TARGET_KIND=ConfigMap \
PUBLISH_TARGET_NAMESPACE=extension-frontend-forge \
PUBLISH_TARGET_NAME=ksbuilder-publish-config \
FE_API_BASE_URL=http://127.0.0.1:18080 \
cargo run -p frontend-forge-controller --bin fi-to-fe-migrator
```

Use a local or port-forwarded FE API at `FE_API_BASE_URL`.

## TODO / Open Question

Status: Planned / TODO

- Publish compensation after source FI deletion is not implemented.
- The publish target contents depend on external `ksbuilder publish` requirements.
