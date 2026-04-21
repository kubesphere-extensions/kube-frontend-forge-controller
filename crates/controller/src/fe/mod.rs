use super::{
    ContextData, Error, FrontendExtensionSourceHashSnafu, GetArtifactConfigMapSnafu,
    GetPublishJobSnafu, ListPackageJobsForHashSnafu, ObservedJobPhase,
    PatchFrontendExtensionStatusSnafu, SerializeFrontendExtensionStatusPatchSnafu, base_owner_ref,
    create_or_get_job, extract_job_message, observed_job_phase,
};
use chrono::Utc;
use frontend_forge_api::{
    ArtifactStorageKind, ArtifactStorageStatus, ExtensionArtifactStatus, ExtensionCondition,
    ExtensionDownloadStatus, FrontendExtension, FrontendExtensionPhase, FrontendExtensionStatus,
    NamespacedResourceRef, PackageJobPhase, PackageJobStatus, PublishPhase, PublishStatus,
};
use frontend_forge_common::{
    ANNO_ARTIFACT_DIGEST, ANNO_OBSERVED_GENERATION, ANNO_PUBLISH_ARTIFACT_DIGEST,
    ANNO_PUBLISH_REQUEST_ID, ANNO_PUBLISH_TARGET_KIND, ANNO_PUBLISH_TARGET_NAME,
    ANNO_PUBLISH_TARGET_NAMESPACE, LABEL_BUILD_KIND, LABEL_FE_NAME, LABEL_MANAGED_BY,
    LABEL_PACKAGE_KIND, LABEL_PUBLISH_KIND, LABEL_PUBLISH_REQUEST_HASH, LABEL_SOURCE_HASH,
    MANAGED_BY_VALUE, PACKAGE_KIND_VALUE, PUBLISH_KIND_VALUE, artifact_configmap_name,
    hash_label_value, package_job_name, publish_job_name, sha256_hex,
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
            let publish =
                sync_publish(&fe, &job_api, &work_ns, &ctx.config, &metadata, &cm).await?;
            let mut status =
                ready_fe_status(&fe, &source_hash, &cm, metadata, existing_package_job(&fe));
            apply_publish_sync(&mut status, &publish);
            patch_fe_status(&fe_api, &fe, status).await?;
            return Ok(requeue_if_publish_running(
                publish,
                ctx.config.reconcile_requeue_seconds,
            ));
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
                        let publish =
                            sync_publish(&fe, &job_api, &work_ns, &ctx.config, &metadata, &cm)
                                .await?;
                        let mut status = ready_fe_status(
                            &fe,
                            &source_hash,
                            &cm,
                            metadata,
                            Some(package_job_status(&job)),
                        );
                        apply_publish_sync(&mut status, &publish);
                        patch_fe_status(&fe_api, &fe, status).await?;
                        return Ok(requeue_if_publish_running(
                            publish,
                            ctx.config.reconcile_requeue_seconds,
                        ));
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

#[derive(Clone, Debug)]
struct PublishRequest {
    request_id: String,
    artifact_digest: String,
    target_ref: NamespacedResourceRef,
    target_kind: String,
}

#[derive(Clone, Debug)]
struct PublishSync {
    status: Option<PublishStatus>,
    should_requeue: bool,
}

async fn sync_publish(
    fe: &FrontendExtension,
    job_api: &Api<Job>,
    namespace: &str,
    config: &super::ControllerConfig,
    artifact: &PackageArtifactMetadata,
    artifact_cm: &ConfigMap,
) -> Result<PublishSync, Error> {
    let Some(request_id) = publish_request_id(fe) else {
        return Ok(PublishSync {
            status: current_publish_for_artifact(fe, &artifact.digest),
            should_requeue: false,
        });
    };

    let request = match publish_request(fe, request_id, &artifact.digest) {
        Ok(request) => request,
        Err(status) => {
            return Ok(PublishSync {
                status: Some(status),
                should_requeue: false,
            });
        }
    };
    let job_name = publish_job_name(&fe.name_any(), &request.request_id);
    let job = match job_api
        .get_opt(&job_name)
        .await
        .with_context(|_| GetPublishJobSnafu {
            namespace: namespace.to_string(),
            name: job_name.clone(),
        })? {
        Some(job) => job,
        None => {
            if publish_already_finished(fe, &request) {
                return Ok(PublishSync {
                    status: fe.status.as_ref().and_then(|status| status.publish.clone()),
                    should_requeue: false,
                });
            }
            let desired_job =
                make_publish_job(fe, config, &job_name, &request, artifact, artifact_cm);
            create_or_get_job(job_api, namespace, desired_job, &job_name).await?
        }
    };

    let status = publish_status_from_job(&request, &job);
    let should_requeue = matches!(status.phase, PublishPhase::Pending | PublishPhase::Running);

    Ok(PublishSync {
        status: Some(status),
        should_requeue,
    })
}

fn publish_request_id(fe: &FrontendExtension) -> Option<&str> {
    fe.metadata
        .annotations
        .as_ref()
        .and_then(|annos| annos.get(ANNO_PUBLISH_REQUEST_ID))
        .map(String::as_str)
        .filter(|request_id| !request_id.is_empty())
}

fn publish_request(
    fe: &FrontendExtension,
    request_id: &str,
    current_artifact_digest: &str,
) -> Result<PublishRequest, PublishStatus> {
    let annos = fe.metadata.annotations.as_ref();
    let requested_digest = annos
        .and_then(|annos| annos.get(ANNO_PUBLISH_ARTIFACT_DIGEST))
        .filter(|digest| !digest.is_empty())
        .cloned()
        .ok_or_else(|| {
            failed_publish_status(
                request_id,
                None,
                "publish artifact digest annotation is required",
            )
        })?;

    if requested_digest != current_artifact_digest {
        return Err(failed_publish_status(
            request_id,
            Some(requested_digest),
            "publish artifact digest does not match current ready artifact",
        ));
    }

    let target_ref = publish_target_ref(fe).ok_or_else(|| {
        failed_publish_status(
            request_id,
            Some(current_artifact_digest.to_string()),
            "publish targetRef is required",
        )
    })?;
    let target_kind = annos
        .and_then(|annos| annos.get(ANNO_PUBLISH_TARGET_KIND))
        .filter(|kind| !kind.is_empty())
        .cloned()
        .unwrap_or_else(|| "ConfigMap".to_string());

    Ok(PublishRequest {
        request_id: request_id.to_string(),
        artifact_digest: current_artifact_digest.to_string(),
        target_ref,
        target_kind,
    })
}

fn publish_target_ref(fe: &FrontendExtension) -> Option<NamespacedResourceRef> {
    let annos = fe.metadata.annotations.as_ref();
    let target_name = annos
        .and_then(|annos| annos.get(ANNO_PUBLISH_TARGET_NAME))
        .filter(|name| !name.is_empty());
    if let Some(name) = target_name {
        return Some(NamespacedResourceRef {
            namespace: annos
                .and_then(|annos| annos.get(ANNO_PUBLISH_TARGET_NAMESPACE))
                .filter(|namespace| !namespace.is_empty())
                .cloned()
                .unwrap_or_else(|| "extension-frontend-forge".to_string()),
            name: name.clone(),
            uid: None,
        });
    }

    fe.spec
        .publish_policy
        .as_ref()
        .and_then(|policy| policy.default_target_ref.clone())
}

fn failed_publish_status(
    request_id: &str,
    artifact_digest: Option<String>,
    message: &str,
) -> PublishStatus {
    PublishStatus {
        phase: PublishPhase::Failed,
        request_id: Some(request_id.to_string()),
        artifact_digest,
        last_error: Some(message.to_string()),
        ..Default::default()
    }
}

fn current_publish_for_artifact(
    fe: &FrontendExtension,
    artifact_digest: &str,
) -> Option<PublishStatus> {
    let publish = fe.status.as_ref().and_then(|status| status.publish.clone());
    match publish {
        Some(status) if status.artifact_digest.as_deref() == Some(artifact_digest) => Some(status),
        _ => Some(PublishStatus::default()),
    }
}

fn publish_already_finished(fe: &FrontendExtension, request: &PublishRequest) -> bool {
    fe.status
        .as_ref()
        .and_then(|status| status.publish.as_ref())
        .map(|publish| {
            publish.request_id.as_deref() == Some(request.request_id.as_str())
                && publish.artifact_digest.as_deref() == Some(request.artifact_digest.as_str())
                && matches!(
                    publish.phase,
                    PublishPhase::Succeeded | PublishPhase::Failed
                )
        })
        .unwrap_or(false)
}

fn make_publish_job(
    fe: &FrontendExtension,
    config: &super::ControllerConfig,
    job_name: &str,
    request: &PublishRequest,
    artifact: &PackageArtifactMetadata,
    artifact_cm: &ConfigMap,
) -> Job {
    let fe_name = fe.name_any();
    let request_hash = format!("sha256:{}", sha256_hex(request.request_id.as_bytes()));
    let labels = BTreeMap::from([
        (LABEL_MANAGED_BY.to_string(), MANAGED_BY_VALUE.to_string()),
        (LABEL_FE_NAME.to_string(), fe_name.clone()),
        (
            LABEL_PUBLISH_KIND.to_string(),
            PUBLISH_KIND_VALUE.to_string(),
        ),
        (
            LABEL_PUBLISH_REQUEST_HASH.to_string(),
            hash_label_value(&request_hash),
        ),
    ]);

    let mut annotations = BTreeMap::from([
        (
            ANNO_PUBLISH_REQUEST_ID.to_string(),
            request.request_id.clone(),
        ),
        (
            ANNO_PUBLISH_ARTIFACT_DIGEST.to_string(),
            request.artifact_digest.clone(),
        ),
        (
            ANNO_ARTIFACT_DIGEST.to_string(),
            request.artifact_digest.clone(),
        ),
    ]);
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
            name: "PUBLISH_REQUEST_ID".to_string(),
            value: Some(request.request_id.clone()),
            ..Default::default()
        },
        EnvVar {
            name: "ARTIFACT_DIGEST".to_string(),
            value: Some(request.artifact_digest.clone()),
            ..Default::default()
        },
        EnvVar {
            name: "ARTIFACT_CONFIGMAP_NAMESPACE".to_string(),
            value: Some(artifact_cm.namespace().unwrap_or_default()),
            ..Default::default()
        },
        EnvVar {
            name: "ARTIFACT_CONFIGMAP_NAME".to_string(),
            value: Some(artifact_cm.name_any()),
            ..Default::default()
        },
        EnvVar {
            name: "ARTIFACT_CONFIGMAP_KEY".to_string(),
            value: Some(PACKAGE_KEY.to_string()),
            ..Default::default()
        },
        EnvVar {
            name: "ARTIFACT_FILENAME".to_string(),
            value: Some(artifact.filename.clone()),
            ..Default::default()
        },
        EnvVar {
            name: "PUBLISH_TARGET_KIND".to_string(),
            value: Some(request.target_kind.clone()),
            ..Default::default()
        },
        EnvVar {
            name: "PUBLISH_TARGET_NAMESPACE".to_string(),
            value: Some(request.target_ref.namespace.clone()),
            ..Default::default()
        },
        EnvVar {
            name: "PUBLISH_TARGET_NAME".to_string(),
            value: Some(request.target_ref.name.clone()),
            ..Default::default()
        },
    ];

    let container = Container {
        name: "publisher".to_string(),
        image: Some(config.publisher_image.clone()),
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
                        "frontend-forge-extension-publisher".to_string(),
                    )])),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    restart_policy: Some("Never".to_string()),
                    service_account_name: config.publisher_service_account.clone(),
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

fn publish_status_from_job(request: &PublishRequest, job: &Job) -> PublishStatus {
    let phase = match observed_job_phase(job.status.as_ref()) {
        ObservedJobPhase::Pending => PublishPhase::Pending,
        ObservedJobPhase::Running => PublishPhase::Running,
        ObservedJobPhase::Succeeded => PublishPhase::Succeeded,
        ObservedJobPhase::Failed => PublishPhase::Failed,
    };
    let last_error = if matches!(phase, PublishPhase::Failed) {
        extract_job_message(job).or_else(|| Some("Publish job failed".to_string()))
    } else {
        None
    };

    PublishStatus {
        phase,
        request_id: Some(request.request_id.clone()),
        artifact_digest: Some(request.artifact_digest.clone()),
        job_ref: Some(namespaced_ref(job)),
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
        last_error,
    }
}

fn apply_publish_sync(status: &mut FrontendExtensionStatus, publish: &PublishSync) {
    status.publish = publish.status.clone();
    let generation = status.observed_generation;
    status
        .conditions
        .retain(|condition| condition.type_ != "PublishSucceeded");
    status.conditions.push(fe_publish_condition_from_status(
        status.publish.as_ref(),
        generation,
    ));
}

fn requeue_if_publish_running(publish: PublishSync, requeue_seconds: u64) -> Action {
    if publish.should_requeue {
        Action::requeue(Duration::from_secs(requeue_seconds))
    } else {
        Action::await_change()
    }
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
    let publish = retained_publish_for_source(fe, source_hash);
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
        publish: publish.clone(),
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
            fe_publish_condition_from_status(publish.as_ref(), generation),
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
    let publish = retained_publish_for_source(fe, source_hash);
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
        publish: publish.clone(),
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
            fe_publish_condition_from_status(publish.as_ref(), generation),
        ],
    }
}

fn retained_publish_for_source(fe: &FrontendExtension, source_hash: &str) -> Option<PublishStatus> {
    let status = fe.status.as_ref()?;
    if status.observed_source_hash.as_deref() == Some(source_hash) {
        status.publish.clone()
    } else {
        Some(PublishStatus::default())
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
    fe_publish_condition_from_status(
        fe.status
            .as_ref()
            .and_then(|status| status.publish.as_ref()),
        observed_generation,
    )
}

fn fe_publish_condition_from_status(
    publish: Option<&PublishStatus>,
    observed_generation: Option<i64>,
) -> ExtensionCondition {
    match publish {
        Some(publish) if matches!(publish.phase, PublishPhase::Succeeded) => fe_condition(
            "PublishSucceeded",
            "True",
            "Succeeded",
            "",
            observed_generation,
        ),
        Some(publish) if matches!(publish.phase, PublishPhase::Failed) => fe_condition(
            "PublishSucceeded",
            "False",
            "PublishFailed",
            publish.last_error.as_deref().unwrap_or(""),
            observed_generation,
        ),
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
