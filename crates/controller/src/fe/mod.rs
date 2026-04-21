use super::{
    ContextData, Error, FrontendExtensionSourceHashSnafu, GetArtifactConfigMapSnafu,
    ListPackageJobsForHashSnafu, ObservedJobPhase, PatchFrontendExtensionStatusSnafu,
    SerializeFrontendExtensionStatusPatchSnafu, base_owner_ref, create_or_get_job,
    extract_job_message, observed_job_phase,
};
use chrono::Utc;
use frontend_forge_api::{
    ArtifactStorageKind, ArtifactStorageStatus, ExtensionArtifactStatus, ExtensionCondition,
    ExtensionDownloadStatus, FrontendExtension, FrontendExtensionPhase, FrontendExtensionStatus,
    NamespacedResourceRef, PackageJobPhase, PackageJobStatus,
};
use frontend_forge_common::{
    ANNO_OBSERVED_GENERATION, LABEL_BUILD_KIND, LABEL_FE_NAME, LABEL_MANAGED_BY,
    LABEL_PACKAGE_KIND, LABEL_SOURCE_HASH, MANAGED_BY_VALUE, PACKAGE_KIND_VALUE,
    artifact_configmap_name, hash_label_value, package_job_name, sha256_hex,
};
use frontend_forge_extension_package::{
    ARTIFACT_METADATA_KEY, PACKAGE_KEY, PackageArtifactMetadata, frontend_extension_package_name,
    frontend_extension_source_hash,
};
use frontend_forge_manifest::validate_frontend_extension;
use futures::StreamExt;
use k8s_openapi::api::batch::v1::{Job, JobSpec};
use k8s_openapi::api::core::v1::{ConfigMap, Container, EnvVar, PodSpec, PodTemplateSpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, Time};
use kube::api::{ListParams, Patch, PatchParams};
use kube::{Api, Resource, ResourceExt};
use kube_runtime::controller::{Action, Controller};
use kube_runtime::watcher;
use serde_json::json;
use snafu::ResultExt;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};

pub(crate) async fn run(ctx: Arc<ContextData>) -> Result<(), Error> {
    let client = ctx.client.clone();
    let fe_api = Api::<FrontendExtension>::all(client.clone());
    let job_api = Api::<Job>::namespaced(client.clone(), &ctx.config.work_namespace);
    let artifact_api =
        Api::<ConfigMap>::namespaced(client.clone(), &ctx.config.artifact_configmap_namespace);

    Controller::new(fe_api, watcher::Config::default())
        .owns(job_api, watcher::Config::default())
        .owns(artifact_api, watcher::Config::default())
        .shutdown_on_signal()
        .run(reconcile, error_policy, ctx)
        .for_each(|result| async move {
            match result {
                Ok((obj_ref, action)) => info!(?obj_ref, ?action, "reconciled"),
                Err(err) => error!(error = %err, "frontend extension reconcile stream error"),
            }
        })
        .await;

    Ok(())
}

fn error_policy(_fe: Arc<FrontendExtension>, err: &Error, _ctx: Arc<ContextData>) -> Action {
    warn!(error = %err, "frontend extension reconcile failed; requeueing");
    Action::requeue(Duration::from_secs(10))
}

async fn reconcile(fe: Arc<FrontendExtension>, ctx: Arc<ContextData>) -> Result<Action, Error> {
    let fe_name = fe.name_any();
    let client = ctx.client.clone();
    let work_ns = ctx.config.work_namespace.clone();
    let artifact_ns = ctx.config.artifact_configmap_namespace.clone();

    let fe_api = Api::<FrontendExtension>::all(client.clone());
    let job_api = Api::<Job>::namespaced(client.clone(), &work_ns);
    let artifact_api = Api::<ConfigMap>::namespaced(client.clone(), &artifact_ns);

    if fe.meta().deletion_timestamp.is_some() {
        return Ok(Action::await_change());
    }

    let source_hash =
        frontend_extension_source_hash(&fe).context(FrontendExtensionSourceHashSnafu)?;
    let package_name = frontend_extension_package_name(&fe);
    let artifact_name = artifact_configmap_name(&package_name, &source_hash);

    info!(
        fe = %fe_name,
        source_hash,
        phase = ?fe.status.as_ref().map(|s| &s.phase),
        "frontend extension reconcile started"
    );

    if let Err(err) = validate_frontend_extension(&fe) {
        patch_fe_status(
            &fe_api,
            &fe,
            failed_fe_status(&fe, &source_hash, None, "InvalidSource", err.to_string()),
        )
        .await?;
        return Ok(Action::await_change());
    }

    if let Some(cm) =
        get_artifact_configmap_opt(&artifact_api, &artifact_ns, &artifact_name).await?
    {
        if let Some(metadata) = artifact_metadata_from_configmap(&cm, &source_hash) {
            let status =
                ready_fe_status(&fe, &source_hash, &cm, metadata, existing_package_job(&fe));
            patch_fe_status(&fe_api, &fe, status).await?;
            return Ok(Action::await_change());
        }
    }

    let current_job = find_package_job_for_hash(&job_api, &work_ns, &fe_name, &source_hash).await?;

    if let Some(job) = current_job {
        match observed_job_phase(job.status.as_ref()) {
            ObservedJobPhase::Pending | ObservedJobPhase::Running => {
                patch_fe_status(
                    &fe_api,
                    &fe,
                    packaging_fe_status(&fe, &source_hash, &job, "Package job in progress"),
                )
                .await?;
                return Ok(Action::requeue(Duration::from_secs(
                    ctx.config.reconcile_requeue_seconds,
                )));
            }
            ObservedJobPhase::Failed => {
                let message =
                    extract_job_message(&job).unwrap_or_else(|| "Package job failed".to_string());
                patch_fe_status(
                    &fe_api,
                    &fe,
                    failed_fe_status(&fe, &source_hash, Some(&job), "PackageFailed", message),
                )
                .await?;
                return Ok(Action::await_change());
            }
            ObservedJobPhase::Succeeded => {
                if let Some(cm) =
                    get_artifact_configmap_opt(&artifact_api, &artifact_ns, &artifact_name).await?
                {
                    if let Some(metadata) = artifact_metadata_from_configmap(&cm, &source_hash) {
                        patch_fe_status(
                            &fe_api,
                            &fe,
                            ready_fe_status(
                                &fe,
                                &source_hash,
                                &cm,
                                metadata,
                                Some(package_job_status(&job)),
                            ),
                        )
                        .await?;
                        return Ok(Action::await_change());
                    }
                }

                patch_fe_status(
                    &fe_api,
                    &fe,
                    packaging_fe_status(
                        &fe,
                        &source_hash,
                        &job,
                        "Package job succeeded; waiting for artifact ConfigMap",
                    ),
                )
                .await?;
                return Ok(Action::requeue(Duration::from_secs(
                    ctx.config.reconcile_requeue_seconds,
                )));
            }
        }
    }

    let job_name = package_job_name(&fe_name, &source_hash);
    let desired_job = make_package_job(&fe, &ctx.config, &job_name, &source_hash, &artifact_name);
    let job = create_or_get_job(&job_api, &work_ns, desired_job, &job_name).await?;
    patch_fe_status(
        &fe_api,
        &fe,
        packaging_fe_status(&fe, &source_hash, &job, "Package job created"),
    )
    .await?;
    Ok(Action::requeue(Duration::from_secs(
        ctx.config.reconcile_requeue_seconds,
    )))
}

async fn find_package_job_for_hash(
    job_api: &Api<Job>,
    namespace: &str,
    fe_name: &str,
    source_hash: &str,
) -> Result<Option<Job>, Error> {
    let selector = format!(
        "{}={},{}={}",
        LABEL_FE_NAME,
        fe_name,
        LABEL_SOURCE_HASH,
        hash_label_value(source_hash)
    );
    let jobs = job_api
        .list(&ListParams::default().labels(&selector))
        .await
        .with_context(|_| ListPackageJobsForHashSnafu {
            namespace: namespace.to_string(),
            fe_name: fe_name.to_string(),
            source_hash: source_hash.to_string(),
        })?;
    let mut items = jobs.items;
    items.sort_by_key(|j| j.metadata.creation_timestamp.clone());
    Ok(items.pop())
}

async fn get_artifact_configmap_opt(
    cm_api: &Api<ConfigMap>,
    namespace: &str,
    name: &str,
) -> Result<Option<ConfigMap>, Error> {
    cm_api
        .get_opt(name)
        .await
        .with_context(|_| GetArtifactConfigMapSnafu {
            namespace: namespace.to_string(),
            name: name.to_string(),
        })
}

fn artifact_metadata_from_configmap(
    cm: &ConfigMap,
    source_hash: &str,
) -> Option<PackageArtifactMetadata> {
    let metadata_content = cm
        .data
        .as_ref()
        .and_then(|data| data.get(ARTIFACT_METADATA_KEY))?;
    let metadata: PackageArtifactMetadata = serde_json::from_str(metadata_content).ok()?;
    if metadata.source_hash != source_hash {
        return None;
    }

    let bytes = cm
        .binary_data
        .as_ref()
        .and_then(|binary_data| binary_data.get(PACKAGE_KEY))?;
    if format!("sha256:{}", sha256_hex(&bytes.0)) != metadata.digest {
        return None;
    }

    Some(metadata)
}

fn make_package_job(
    fe: &FrontendExtension,
    config: &super::ControllerConfig,
    job_name: &str,
    source_hash: &str,
    artifact_configmap_name: &str,
) -> Job {
    let fe_name = fe.name_any();
    let mut labels = BTreeMap::from([
        (LABEL_MANAGED_BY.to_string(), MANAGED_BY_VALUE.to_string()),
        (LABEL_FE_NAME.to_string(), fe_name.clone()),
        (LABEL_SOURCE_HASH.to_string(), hash_label_value(source_hash)),
        (
            LABEL_PACKAGE_KIND.to_string(),
            PACKAGE_KIND_VALUE.to_string(),
        ),
    ]);
    labels.insert(
        LABEL_BUILD_KIND.to_string(),
        "frontend-extension-package".to_string(),
    );

    let mut annotations = BTreeMap::new();
    if let Some(generation) = fe.metadata.generation {
        annotations.insert(ANNO_OBSERVED_GENERATION.to_string(), generation.to_string());
    }

    let env = vec![
        EnvVar {
            name: "FE_NAME".to_string(),
            value: Some(fe_name.clone()),
            ..Default::default()
        },
        EnvVar {
            name: "SOURCE_HASH".to_string(),
            value: Some(source_hash.to_string()),
            ..Default::default()
        },
        EnvVar {
            name: "ARTIFACT_CONFIGMAP_NAMESPACE".to_string(),
            value: Some(config.artifact_configmap_namespace.clone()),
            ..Default::default()
        },
        EnvVar {
            name: "ARTIFACT_CONFIGMAP_NAME".to_string(),
            value: Some(artifact_configmap_name.to_string()),
            ..Default::default()
        },
    ];

    let container = Container {
        name: "packager".to_string(),
        image: Some(config.packager_image.clone()),
        env: Some(env),
        ..Default::default()
    };

    Job {
        metadata: ObjectMeta {
            name: Some(job_name.to_string()),
            namespace: Some(config.work_namespace.clone()),
            labels: Some(labels),
            annotations: Some(annotations),
            owner_references: base_owner_ref(fe).map(|o| vec![o]),
            ..Default::default()
        },
        spec: Some(JobSpec {
            active_deadline_seconds: Some(config.job_active_deadline_seconds),
            ttl_seconds_after_finished: config.job_ttl_seconds_after_finished,
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(BTreeMap::from([(
                        "app.kubernetes.io/name".to_string(),
                        "frontend-forge-extension-packager".to_string(),
                    )])),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    restart_policy: Some("Never".to_string()),
                    service_account_name: config.packager_service_account.clone(),
                    containers: vec![container],
                    ..Default::default()
                }),
            },
            backoff_limit: Some(0),
            ..Default::default()
        }),
        status: None,
    }
}

fn package_job_status(job: &Job) -> PackageJobStatus {
    let phase = match observed_job_phase(job.status.as_ref()) {
        ObservedJobPhase::Pending => PackageJobPhase::Pending,
        ObservedJobPhase::Running => PackageJobPhase::Running,
        ObservedJobPhase::Succeeded => PackageJobPhase::Succeeded,
        ObservedJobPhase::Failed => PackageJobPhase::Failed,
    };

    PackageJobStatus {
        namespace: job.namespace().unwrap_or_default(),
        name: job.name_any(),
        uid: job.meta().uid.clone(),
        phase,
        started_at: job
            .status
            .as_ref()
            .and_then(|status| status.start_time.as_ref())
            .and_then(k8s_time_to_chrono),
        finished_at: job
            .status
            .as_ref()
            .and_then(|status| status.completion_time.as_ref())
            .and_then(k8s_time_to_chrono),
        message: extract_job_message(job),
    }
}

fn k8s_time_to_chrono(time: &Time) -> Option<chrono::DateTime<Utc>> {
    let nanos = time.0.subsec_nanosecond();
    if nanos < 0 {
        return None;
    }
    chrono::DateTime::from_timestamp(time.0.as_second(), nanos as u32)
}

fn existing_package_job(fe: &FrontendExtension) -> Option<PackageJobStatus> {
    fe.status
        .as_ref()
        .and_then(|status| status.package_job.clone())
}

fn packaging_fe_status(
    fe: &FrontendExtension,
    source_hash: &str,
    job: &Job,
    message: &str,
) -> FrontendExtensionStatus {
    let generation = Some(fe.metadata.generation.unwrap_or_default());
    FrontendExtensionStatus {
        phase: FrontendExtensionPhase::Packaging,
        observed_generation: generation,
        observed_source_hash: Some(source_hash.to_string()),
        artifact: None,
        download: Some(ExtensionDownloadStatus {
            ready: false,
            filename: String::new(),
            media_type: String::new(),
        }),
        package_job: Some(package_job_status(job)),
        publish: fe.status.as_ref().and_then(|status| status.publish.clone()),
        conditions: vec![
            fe_condition("SourceValid", "True", "Validated", "", generation),
            fe_condition("ArtifactReady", "False", "Packaging", message, generation),
            fe_condition(
                "DownloadReady",
                "False",
                "ArtifactNotReady",
                message,
                generation,
            ),
            fe_publish_condition(fe, generation),
        ],
    }
}

fn ready_fe_status(
    fe: &FrontendExtension,
    source_hash: &str,
    cm: &ConfigMap,
    metadata: PackageArtifactMetadata,
    package_job: Option<PackageJobStatus>,
) -> FrontendExtensionStatus {
    let generation = Some(fe.metadata.generation.unwrap_or_default());
    FrontendExtensionStatus {
        phase: FrontendExtensionPhase::Ready,
        observed_generation: generation,
        observed_source_hash: Some(source_hash.to_string()),
        artifact: Some(ExtensionArtifactStatus {
            storage: ArtifactStorageStatus {
                kind: ArtifactStorageKind::ConfigMap,
                ref_: namespaced_ref(cm),
                key: PACKAGE_KEY.to_string(),
            },
            digest: metadata.digest.clone(),
            size_bytes: metadata.size_bytes as i64,
            media_type: metadata.media_type.clone(),
            filename: metadata.filename.clone(),
            generated_at: metadata.generated_at,
            source_hash: metadata.source_hash,
        }),
        download: Some(ExtensionDownloadStatus {
            ready: true,
            filename: metadata.filename,
            media_type: metadata.media_type,
        }),
        package_job,
        publish: fe.status.as_ref().and_then(|status| status.publish.clone()),
        conditions: vec![
            fe_condition("SourceValid", "True", "Validated", "", generation),
            fe_condition("ArtifactReady", "True", "Generated", "", generation),
            fe_condition("DownloadReady", "True", "Available", "", generation),
            fe_publish_condition(fe, generation),
        ],
    }
}

fn failed_fe_status(
    fe: &FrontendExtension,
    source_hash: &str,
    job: Option<&Job>,
    reason: &str,
    message: String,
) -> FrontendExtensionStatus {
    let generation = Some(fe.metadata.generation.unwrap_or_default());
    FrontendExtensionStatus {
        phase: FrontendExtensionPhase::Failed,
        observed_generation: generation,
        observed_source_hash: Some(source_hash.to_string()),
        artifact: None,
        download: Some(ExtensionDownloadStatus {
            ready: false,
            filename: String::new(),
            media_type: String::new(),
        }),
        package_job: job.map(package_job_status),
        publish: fe.status.as_ref().and_then(|status| status.publish.clone()),
        conditions: vec![
            fe_condition(
                "SourceValid",
                if reason == "InvalidSource" {
                    "False"
                } else {
                    "True"
                },
                reason,
                &message,
                generation,
            ),
            fe_condition("ArtifactReady", "False", reason, &message, generation),
            fe_condition(
                "DownloadReady",
                "False",
                "ArtifactNotReady",
                &message,
                generation,
            ),
            fe_publish_condition(fe, generation),
        ],
    }
}

fn fe_condition(
    type_: &str,
    status: &str,
    reason: &str,
    message: &str,
    observed_generation: Option<i64>,
) -> ExtensionCondition {
    ExtensionCondition {
        type_: type_.to_string(),
        status: status.to_string(),
        reason: Some(reason.to_string()),
        message: if message.is_empty() {
            None
        } else {
            Some(message.to_string())
        },
        observed_generation,
        last_transition_time: None,
    }
}

fn fe_publish_condition(
    fe: &FrontendExtension,
    observed_generation: Option<i64>,
) -> ExtensionCondition {
    match fe
        .status
        .as_ref()
        .and_then(|status| status.publish.as_ref())
    {
        Some(publish) if matches!(publish.phase, frontend_forge_api::PublishPhase::Succeeded) => {
            fe_condition(
                "PublishSucceeded",
                "True",
                "Succeeded",
                "",
                observed_generation,
            )
        }
        Some(publish) if matches!(publish.phase, frontend_forge_api::PublishPhase::Failed) => {
            fe_condition(
                "PublishSucceeded",
                "False",
                "PublishFailed",
                publish.last_error.as_deref().unwrap_or(""),
                observed_generation,
            )
        }
        _ => fe_condition(
            "PublishSucceeded",
            "False",
            "NotRequested",
            "",
            observed_generation,
        ),
    }
}

fn namespaced_ref<K: ResourceExt>(obj: &K) -> NamespacedResourceRef {
    NamespacedResourceRef {
        namespace: obj.namespace().unwrap_or_default(),
        name: obj.name_any(),
        uid: obj.meta().uid.clone(),
    }
}

fn fe_status_needs_patch(fe: &FrontendExtension, desired_status: &FrontendExtensionStatus) -> bool {
    fe.status.as_ref() != Some(desired_status)
}

async fn patch_fe_status(
    fe_api: &Api<FrontendExtension>,
    fe: &FrontendExtension,
    status: FrontendExtensionStatus,
) -> Result<(), Error> {
    if !fe_status_needs_patch(fe, &status) {
        return Ok(());
    }

    let fe_name = fe.name_any();
    let patch = frontend_extension_status_patch(&status, &fe_name)?;

    fe_api
        .patch_status(&fe_name, &PatchParams::default(), &Patch::Merge(&patch))
        .await
        .with_context(|_| PatchFrontendExtensionStatusSnafu {
            name: fe_name.clone(),
        })?;

    Ok(())
}

fn frontend_extension_status_patch(
    status: &FrontendExtensionStatus,
    name: &str,
) -> Result<serde_json::Value, Error> {
    let mut status_value = serde_json::to_value(status).with_context(|_| {
        SerializeFrontendExtensionStatusPatchSnafu {
            name: name.to_string(),
        }
    })?;
    let status_object = status_value.as_object_mut().ok_or_else(|| {
        Error::InvalidFrontendExtensionStatusPatchShape {
            name: name.to_string(),
        }
    })?;

    if status.artifact.is_none() {
        status_object.insert("artifact".to_string(), serde_json::Value::Null);
    }
    if status.download.is_none() {
        status_object.insert("download".to_string(), serde_json::Value::Null);
    }
    if status.package_job.is_none() {
        status_object.insert("packageJob".to_string(), serde_json::Value::Null);
    }
    if status.publish.is_none() {
        status_object.insert("publish".to_string(), serde_json::Value::Null);
    }

    Ok(json!({
        "status": status_value,
    }))
}
