# FI Runtime Controller And Runner

Status: Implemented. Disabled by default with `controller.enabled=false`.

`FrontendIntegration` is the older runtime `JSBundle` flow. The default Helm
installation uses `FrontendExtension` package/publish and enables FI-to-FE
migration.

Code owners: `crates/frontend-forge-controller`,
`crates/frontend-forge-runner`

Source of truth: `crates/frontend-forge-controller/src/fi/mod.rs`,
`crates/frontend-forge-controller/src/webhook.rs`,
`crates/frontend-forge-runner/src/main.rs`

## Status

| Capability | Status | Default Helm state |
| --- | --- | --- |
| FI runtime controller | Implemented | Disabled by `controller.enabled=false` |
| FI runner Job | Implemented | Created only by enabled FI controller |
| FI admission webhook | Implemented | Disabled by `webhook.enabled=false` |
| Build-service retry in runner | Implemented | Runtime behavior |
| In-repo production build-service | Planned / TODO | Not defined |

## Controller Inputs

Status: Implemented

Environment variables read by `ControllerConfig::from_env`:

| Env | Helm value | Default in binary | Behavior |
| --- | --- | --- | --- |
| `WORK_NAMESPACE` | `controller.workNamespace` | `extension-frontend-forge` | Namespace for runner Jobs. |
| `RUNNER_IMAGE` | `runner.image.*` | `kubesphere/frontend-forge-runner:latest` | Runner Job image. |
| `RUNNER_SERVICE_ACCOUNT` | `controller.runnerServiceAccountName` | unset | Runner Job service account. |
| `BUILD_SERVICE_BASE_URL` | `controller.buildServiceBaseUrl` | `http://frontend-forge.extension-frontend-forge.svc` | Runner build-service endpoint. |
| `JSBUNDLE_CONFIGMAP_NAMESPACE` | `controller.jsbundleConfigmapNamespace` | `extension-frontend-forge` | Bundle ConfigMap namespace. |
| `JSBUNDLE_CONFIG_KEY` | `controller.jsbundleConfigKey` | `index.js` | Preferred bundle artifact key. |
| `BUILD_SERVICE_TIMEOUT_SECONDS` | `controller.buildServiceTimeoutSeconds` | `600` in binary, `240` in chart | HTTP timeout used by runner. |
| `STALE_CHECK_GRACE_SECONDS` | `controller.staleCheckGraceSeconds` | `30` | Runner status observation wait. |
| `RECONCILE_REQUEUE_SECONDS` | `controller.reconcileRequeueSeconds` | `5` | Build/publish polling delay. |
| `JOB_ACTIVE_DEADLINE_SECONDS` | `controller.jobActiveDeadlineSeconds` | `300` | Runner Job active deadline. |
| `JOB_TTL_SECONDS_AFTER_FINISHED` | `controller.jobTtlSecondsAfterFinished` | `3600` | Runner Job TTL. |

Helm values override the binary defaults for chart deployments.

## Reconcile Behavior

Status: Implemented

Controller watches cluster-scoped `FrontendIntegration` and owns runner Jobs in
`WORK_NAMESPACE`.

| Step | Behavior |
| --- | --- |
| Deletion | If FI has `deletionTimestamp`, controller waits for changes. |
| Label sync | Patches FI label `frontend-forge.io/enabled` to `true` or `false`. |
| Spec hash | Computes `sha256` over `fi.spec.without_enabled()`. |
| Bundle name | Uses `fi-<fi-name>` bounded to 63 chars. |
| Disabled FI | Patches existing matching `JSBundle` to label enabled `false` and status `Disabled`; FI status becomes `Pending` with message `Disabled`. |
| Build needed | Creates or reuses one runner Job for `(fi_name, spec_hash)`. |
| Build running | Patches FI status `phase=Building`, `observed_spec_hash`, `last_build`, `bundle_ref`. |
| Job failed | Patches FI status `phase=Failed`; preserves runner-written `last_error` for same spec hash when present. |
| Job succeeded, bundle present | If bundle label `frontend-forge.io/spec-hash` matches, sets `JSBundle` enabled `true`, status `Available`, then patches FI `Succeeded`. |
| Job succeeded, bundle missing/mismatched | Keeps FI `Building` and requeues. |
| No current Job, matching bundle present | Syncs JSBundle enabled state and patches FI `Succeeded`. |

Job reuse rules:

- Pending or running Job for the same hash is reused.
- Succeeded Job is reused only when the matching bundle is ready and FI is not currently failed.
- Failed Job is not reused.
- If multiple Jobs match a hash, controller uses the latest by creation timestamp.

## Runner Job

Status: Implemented

Job name:

```text
fi-<fi-name>-build-<spec-hash-short>
```

Runner environment:

| Env | Source |
| --- | --- |
| `FI_NAME` | FI name |
| `SPEC_HASH` | Controller-computed spec hash |
| `JSBUNDLE_NAME` | `fi-<fi-name>` bounded to 63 chars |
| `BUILD_SERVICE_BASE_URL` | Controller config |
| `JSBUNDLE_CONFIGMAP_NAMESPACE` | Controller config |
| `JSBUNDLE_CONFIG_KEY` | Controller config |
| `BUILD_SERVICE_TIMEOUT_SECONDS` | Controller config |
| `STALE_CHECK_GRACE_SECONDS` | Controller config |

Runner flow:

| Step | Behavior |
| --- | --- |
| Read FI | Reads cluster-scoped FI by `FI_NAME`. |
| Stale pre-check | Recomputes `spec.without_enabled()` hash; exits without writes if hash differs. |
| Render manifest | Calls `render_extension_manifest`. |
| Build | Sends canonical manifest JSON to build-service. |
| Retry | Retries request-level build-service failures for 60s with delay 1s to 5s. |
| Stale post-check | Waits until FI status observes this spec hash; exits without writes if a different hash is observed. |
| Select artifact | Uses `JSBUNDLE_CONFIG_KEY`, default `index.js`, to select build output. |
| Write ConfigMap | Server-side applies bundle ConfigMap in `JSBUNDLE_CONFIGMAP_NAMESPACE`. |
| Write JSBundle | Server-side applies cluster-scoped `JSBundle` with `rawFrom.configMapKeyRef`. |
| Patch JSBundle status | Patches status `state=Available`, `link=/dist/<jsbundle>/<key>`. |
| Failure status | On runner error, patches FI status `phase=Failed`, `last_error.source=runner`. |

Bundle ConfigMap name:

```text
<jsbundle-name>-config
```

bounded to 63 chars.

## Labels And Annotations

Status: Implemented

Runtime labels:

| Key | Written on | Behavior |
| --- | --- | --- |
| `frontend-forge.io/managed-by` | Jobs, ConfigMaps, JSBundle | Value `frontend-forge-builder-controller`. |
| `frontend-forge.io/fi-name` | Jobs, ConfigMaps, JSBundle | Source FI name. |
| `frontend-forge.io/spec-hash` | Jobs, ConfigMaps, JSBundle | Hash label value, truncated for label length. |
| `frontend-forge.io/manifest-hash` | ConfigMaps, JSBundle | Manifest hash label value. |
| `frontend-forge.io/build-kind` | Jobs | Value `frontend-forge`. |
| `frontend-forge.io/enabled` | FI, JSBundle | Runtime enabled state. |

Runtime annotations:

| Key | Written on | Behavior |
| --- | --- | --- |
| `frontend-forge.io/observed-generation` | Job | FI generation used for the Job. |
| `frontend-forge.io/build-job` | ConfigMap, JSBundle | Runner Job name from `HOSTNAME`. |
| `frontend-forge.io/manifest-hash` | ConfigMap, JSBundle | Full manifest hash. |
| `frontend-forge.io/manifest-content` | JSBundle | Canonical manifest JSON. |
| `frontend-forge.io/source-spec` | JSBundle | Canonical FI spec snapshot. |
| `frontend-forge.io/source-spec-hash` | JSBundle | Hash of FI spec snapshot. |
| `frontend-forge.io/source-generation` | JSBundle | FI generation. |

## FI Status

Status: Implemented

| Phase | Written by | Meaning |
| --- | --- | --- |
| `Pending` | Controller | FI disabled or default status. |
| `Building` | Controller | Runner Job created/running or waiting for matching bundle. |
| `Succeeded` | Controller | Matching `JSBundle` is present and marked available. |
| `Failed` | Controller or runner | Runner or Job failed for the observed spec hash. |

Compatibility behavior:

- `observed_manifest_hash` is retained for older status consumers.
- `observed_manifest_hash` can still be used as fallback when checking observed hash.
- `active_build` is accepted as serde alias for `last_build`.

## Admission Webhook

Status: Implemented

Webhook server is part of `frontend-forge-controller`; no separate validator
binary exists.

Environment variables:

| Env | Default |
| --- | --- |
| `WEBHOOK_ENABLED` | `false` |
| `WEBHOOK_BIND_ADDR` | `0.0.0.0:9443` |
| `WEBHOOK_CERT_PATH` | `/tls/tls.crt` |
| `WEBHOOK_KEY_PATH` | `/tls/tls.key` |

Routes:

| Method | Path | Behavior |
| --- | --- | --- |
| `GET` | `/healthz` | Returns `ok`. |
| `POST` | `/validate/frontendintegrations` | Processes Kubernetes `AdmissionReview<FrontendIntegration>`. |

Admission behavior:

- Validates only `CREATE` and `UPDATE`.
- Uses `validate_frontend_integration`.
- Returns renderer error text in denial response.
- Allows other operations by returning a normal admission response.
- Missing `request.object` returns failure reason `InvalidRequest`.

Helm webhook resources are rendered only when both `controller.enabled=true` and
`webhook.enabled=true`:

| Resource | Status | Source |
| --- | --- | --- |
| Webhook Service | Implemented | `templates/webhook.yaml` |
| `ValidatingWebhookConfiguration` | Implemented | `templates/webhook.yaml` |
| certgen create Job | Implemented | `templates/webhook-certgen.yaml` |
| certgen patch Job | Implemented | `templates/webhook-certgen.yaml` |

certgen image defaults to `kubespheredev/kube-webhook-certgen:v1.1.1`.

## Runtime Constraints

Status: Implemented

- The controller does not render manifests; runner and webhook call the shared renderer/validator.
- `enabled` does not affect build identity.
- Runner and controller treat `JSBundle` as cluster-scoped.
- Bundle payload is stored in a namespaced ConfigMap and referenced by `JSBundle.spec.rawFrom.configMapKeyRef`.
- Runner writes owner references to FI on ConfigMap and JSBundle when possible.
- Controller tolerates clusters where `JSBundle` has no status subresource by falling back to a normal patch.

## TODO / Open Question

Status: Planned / TODO

- Production build-service deployment ownership is not defined in this repository.
- FI runtime is still implemented, but Helm default migration path disables its controller. Consumers that still need FI runtime must enable it explicitly.
