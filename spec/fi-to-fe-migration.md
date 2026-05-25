# FI To FE Migration

Code owner: `crates/frontend-forge-controller/src/bin/fi_to_fe_migrator.rs`

Helm template: `config/charts/frontend-forge/templates/fi-to-fe-migration-job.yaml`

## Status

| Capability | Status | Default Helm state |
| --- | --- | --- |
| Helm hook migrator Job | Implemented | Enabled by `migration.fiToFe.enabled=true` |
| FI list/read/delete | Implemented | Cluster-scoped API |
| FE create/patch | Implemented | Cluster-scoped API |
| Publish enabled FI by FE intent | Implemented | Patch FE annotations |
| Compensation after FI deleted and publish intent failed | Planned / TODO | Not implemented |

## Job Configuration

Status: Implemented

| Env | Helm value | Binary default | Behavior |
| --- | --- | --- | --- |
| `PACKAGE_VERSION` | `migration.fiToFe.packageVersion` | `0.1.0` | Migrated FE package version. |
| `SCHEMA_VERSION` | `migration.fiToFe.schemaVersion` | `v1` | Fallback FE inline schema version. |
| `READY_TIMEOUT_SECONDS` | `migration.fiToFe.readyTimeoutSeconds` | `600` | CRD readiness and FI deletion timeout. |
| `POLL_INTERVAL_SECONDS` | `migration.fiToFe.pollIntervalSeconds` | `5` | Poll interval. |
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

FE API Service availability is not required. The migrator patches FE resources
directly through the Kubernetes API.

## Per-FI Flow

Status: Implemented

```text
list all FrontendIntegration
for each FI:
  derive FE name
  create or patch migrator-owned FE
  delete FI and wait until it is gone
  if original FI spec.enabled is missing or true:
    patch FE publish intent for returned generation/sourceHash
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
| Label `frontend-forge.kubesphere.io/managed-by` | `frontend-forge-fi-migrator` |
| Annotation `frontend-forge.kubesphere.io/source-fi-name` | Source FI name |
| Annotation `frontend-forge.kubesphere.io/source-fi-uid` | Source FI UID when present |

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
| `spec.menus` | `spec.source.inline.frontend.menus`; each FI menu `placement` becomes a single-item FE `placements`; page menus receive `pageKey` from the FI menu or child key |
| `spec.pages` | `spec.source.inline.frontend.pages`; generated FE pages keep page config and key, and collect the placements of every FI menu that binds the page |
| none | `spec.source.inline.extensionResources` is absent |
| constant `Manual` | `spec.publishPolicy.mode` |
| `PUBLISH_TARGET_KIND` | `spec.publishPolicy.defaultTargetKind` |
| `PUBLISH_TARGET_NAMESPACE` / `PUBLISH_TARGET_NAME` | `spec.publishPolicy.defaultTargetRef` |

`spec.enabled` is not copied into FE. It is only used to decide whether publish
is requested after FI deletion.

## Publish Request

Status: Implemented

The migrator no longer calls FE API publish endpoints. For enabled source FIs it
patches publish intent annotations directly onto the migrated FE after the FE
create/patch response is returned by apiserver:

```yaml
frontend-forge.kubesphere.io/publish-request-id: fi-migration-<fe-name>-<source-hash-prefix-12>
frontend-forge.kubesphere.io/publish-request-generation: "<metadata.generation>"
frontend-forge.kubesphere.io/publish-request-source-hash: <current FE source hash>
frontend-forge.kubesphere.io/publish-artifact-digest: null
frontend-forge.kubesphere.io/publish-target-kind: ConfigMap
frontend-forge.kubesphere.io/publish-target-namespace: <target namespace>
frontend-forge.kubesphere.io/publish-target-name: <target name>
```

The FE controller converts this intent into a publish Job only after the matching
generation/source hash artifact is ready. If the FE changes before the artifact is
ready, the stale intent is failed and no old artifact is published.

## Finalizer And Retry Constraints

Status: Implemented

- Migrator deletes the source FI after the FE create/patch succeeds.
- It waits until FI no longer exists before writing publish intent.
- If FI deletion is blocked by a finalizer, migration for that FI times out.
- The default Job `backoffLimit=0` avoids hiding partial failure with Job-level retries.

Status: Planned / TODO

- If FI deletion succeeds and publish intent patch fails, rerunning the Job will
  not see the deleted FI and will not automatically retry publish.

## Local Verification

Status: Implemented

Run the migrator binary against the current kubeconfig:

```bash
PACKAGE_VERSION=0.1.0 \
SCHEMA_VERSION=v1 \
PUBLISH_TARGET_KIND=ConfigMap \
PUBLISH_TARGET_NAMESPACE=extension-frontend-forge \
PUBLISH_TARGET_NAME=ksbuilder-publish-config \
cargo run -p frontend-forge-controller --bin fi-to-fe-migrator
```

## TODO / Open Question

Status: Planned / TODO

- Publish compensation after source FI deletion is not implemented.
- The publish target contents depend on external `ksbuilder publish` requirements.
