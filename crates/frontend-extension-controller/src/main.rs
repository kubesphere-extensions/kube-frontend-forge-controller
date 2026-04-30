use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    sync::Arc,
    time::Duration,
};

use chrono::Utc;
use frontend_forge_api::{
    ArtifactStorageKind, ArtifactStorageStatus, ExtensionArtifactStatus, ExtensionCondition,
    ExtensionDownloadStatus, FrontendExtension, FrontendExtensionPhase, FrontendExtensionStatus,
    NamespacedResourceRef, PackageJobPhase, PackageJobStatus, PublishPhase, PublishStatus,
    PublishTargetKind,
};
use frontend_forge_common::{
    ANNO_ARTIFACT_DIGEST, ANNO_ARTIFACT_KEY, ANNO_OBSERVED_GENERATION,
    ANNO_PUBLISH_ARTIFACT_DIGEST, ANNO_PUBLISH_REQUEST_ID, ANNO_PUBLISH_TARGET_KIND,
    ANNO_PUBLISH_TARGET_NAME, ANNO_PUBLISH_TARGET_NAMESPACE, ANNO_REBUILD_TOKEN, ANNO_SOURCE_HASH,
    LABEL_ARTIFACT_KEY_SHORT, LABEL_BUILD_KIND, LABEL_FE_NAME, LABEL_FE_UID, LABEL_MANAGED_BY,
    LABEL_PACKAGE_KIND, LABEL_PUBLISH_KIND, LABEL_PUBLISH_REQUEST_HASH, LABEL_SOURCE_HASH_SHORT,
    MANAGED_BY_VALUE, ObservedJobPhase, PACKAGE_KIND_VALUE, PUBLISH_KIND_VALUE,
    artifact_configmap_name, artifact_key, base_owner_ref, create_or_get_job, extract_job_message,
    hash_label_value, observed_job_phase, package_job_name, publish_job_name, sha256_hex,
};
use frontend_forge_extension_package_core::{
    ARTIFACT_METADATA_KEY, ExtensionPackageError, PACKAGE_KEY, PackageArtifactMetadata,
    frontend_extension_package_name, frontend_extension_source_hash,
};
use frontend_forge_manifest::validate_frontend_extension;
use futures::StreamExt;
use k8s_openapi::{
    api::{
        batch::v1::{Job, JobSpec},
        core::v1::{ConfigMap, Container, EnvVar, PodSpec, PodTemplateSpec},
    },
    apimachinery::pkg::apis::meta::v1::{ObjectMeta, Time},
};
use kube::{
    Api, Client, Resource, ResourceExt,
    api::{DeleteParams, ListParams, Patch, PatchParams},
};
use kube_runtime::{
    controller::{Action, Controller},
    watcher,
};
use serde_json::json;
use snafu::{ResultExt, Snafu};
use tracing::{error, info, warn};

#[derive(Debug, Snafu)]
enum Error {
    #[snafu(display("failed to hash FrontendExtension package source: {source}"))]
    FrontendExtensionSourceHash { source: ExtensionPackageError },
    #[snafu(display("failed to compute FrontendExtension artifact key: {source}"))]
    FrontendExtensionArtifactKey {
        source: frontend_forge_common::CommonError,
    },
    #[snafu(display("failed to initialize Kubernetes client: {source}"))]
    KubeClientInit {
        #[snafu(source(from(kube::Error, Box::new)))]
        source: Box<kube::Error>,
    },
    #[snafu(display("failed to patch FrontendExtension status {name}: {source}"))]
    PatchFrontendExtensionStatus {
        name: String,
        #[snafu(source(from(kube::Error, Box::new)))]
        source: Box<kube::Error>,
    },
    #[snafu(display("failed to serialize FrontendExtension status patch for {name}: {source}"))]
    SerializeFrontendExtensionStatusPatch {
        name: String,
        source: serde_json::Error,
    },
    #[snafu(display("serialized FrontendExtension status patch for {name} was not a JSON object"))]
    InvalidFrontendExtensionStatusPatchShape { name: String },
    #[snafu(display(
        "failed to list package Jobs in {namespace} for FrontendExtension {fe_name} and \
         artifactKey {artifact_key}: {source}"
    ))]
    ListPackageJobsForArtifactKey {
        namespace: String,
        fe_name: String,
        artifact_key: String,
        #[snafu(source(from(kube::Error, Box::new)))]
        source: Box<kube::Error>,
    },
    #[snafu(display("failed to get artifact ConfigMap {namespace}/{name}: {source}"))]
    GetArtifactConfigMap {
        namespace: String,
        name: String,
        #[snafu(source(from(kube::Error, Box::new)))]
        source: Box<kube::Error>,
    },
    #[snafu(display("failed to list artifact ConfigMaps in {namespace} for GC: {source}"))]
    ListArtifactConfigMapsForGc {
        namespace: String,
        #[snafu(source(from(kube::Error, Box::new)))]
        source: Box<kube::Error>,
    },
    #[snafu(display("failed to delete artifact ConfigMap {namespace}/{name}: {source}"))]
    DeleteArtifactConfigMap {
        namespace: String,
        name: String,
        #[snafu(source(from(kube::Error, Box::new)))]
        source: Box<kube::Error>,
    },
    #[snafu(transparent)]
    Job {
        source: frontend_forge_common::JobError,
    },
    #[snafu(display("failed to get publish Job {namespace}/{name}: {source}"))]
    GetPublishJob {
        namespace: String,
        name: String,
        #[snafu(source(from(kube::Error, Box::new)))]
        source: Box<kube::Error>,
    },
}

#[derive(Clone, Debug)]
struct ControllerConfig {
    work_namespace: String,
    packager_image: String,
    packager_service_account: Option<String>,
    publisher_image: String,
    publisher_service_account: Option<String>,
    artifact_configmap_namespace: String,
    build_service_base_url: String,
    build_service_timeout_seconds: u64,
    jsbundle_config_key: String,
    reconcile_requeue_seconds: u64,
    job_active_deadline_seconds: i64,
    job_ttl_seconds_after_finished: Option<i32>,
    artifact_retain_old_count: usize,
    package_max_attempts: u32,
}

impl ControllerConfig {
    fn from_env() -> Self {
        let work_namespace =
            env::var("WORK_NAMESPACE").unwrap_or_else(|_| "extension-frontend-forge".to_string());
        Self {
            work_namespace: work_namespace.clone(),
            packager_image: env::var("PACKAGER_IMAGE").unwrap_or_else(|_| {
                "kubesphere/frontend-forge-extension-packager:latest".to_string()
            }),
            packager_service_account: env::var("PACKAGER_SERVICE_ACCOUNT").ok(),
            publisher_image: env::var("PUBLISHER_IMAGE").unwrap_or_else(|_| {
                "kubesphere/frontend-forge-extension-publisher:latest".to_string()
            }),
            publisher_service_account: env::var("PUBLISHER_SERVICE_ACCOUNT").ok(),
            artifact_configmap_namespace: env::var("ARTIFACT_CONFIGMAP_NAMESPACE")
                .unwrap_or(work_namespace),
            build_service_base_url: env::var("BUILD_SERVICE_BASE_URL").unwrap_or_else(|_| {
                "http://frontend-forge.extension-frontend-forge.svc".to_string()
            }),
            build_service_timeout_seconds: env::var("BUILD_SERVICE_TIMEOUT_SECONDS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(240),
            jsbundle_config_key: env::var("JSBUNDLE_CONFIG_KEY")
                .unwrap_or_else(|_| "index.js".to_string()),
            reconcile_requeue_seconds: env::var("RECONCILE_REQUEUE_SECONDS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5),
            job_active_deadline_seconds: env::var("JOB_ACTIVE_DEADLINE_SECONDS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(300),
            job_ttl_seconds_after_finished: env::var("JOB_TTL_SECONDS_AFTER_FINISHED")
                .ok()
                .and_then(|v| v.parse().ok())
                .or(Some(DEFAULT_JOB_TTL_SECONDS_AFTER_FINISHED)),
            artifact_retain_old_count: env::var("ARTIFACT_RETAIN_OLD_COUNT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(DEFAULT_ARTIFACT_RETAIN_OLD_COUNT),
            package_max_attempts: env::var("PACKAGE_MAX_ATTEMPTS")
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|attempts| *attempts > 0)
                .unwrap_or(DEFAULT_PACKAGE_MAX_ATTEMPTS),
        }
    }
}

#[derive(Clone)]
struct ContextData {
    client: Client,
    config: ControllerConfig,
}

const DEFAULT_JOB_TTL_SECONDS_AFTER_FINISHED: i32 = 60 * 60;
const DEFAULT_ARTIFACT_RETAIN_OLD_COUNT: usize = 1;
const DEFAULT_PACKAGE_MAX_ATTEMPTS: u32 = 3;

fn install_rustls_crypto_provider() {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        rustls::crypto::ring::default_provider()
            .install_default()
            .expect(
                "ring crypto provider should install before frontend extension controller startup",
            );
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    install_rustls_crypto_provider();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,frontend_extension_controller=debug".into()),
        )
        .init();

    let client = Client::try_default().await.context(KubeClientInitSnafu)?;
    let ctx = Arc::new(ContextData {
        client,
        config: ControllerConfig::from_env(),
    });
    run(ctx).await?;
    info!("frontend extension controller shutdown complete");
    Ok(())
}

async fn run(ctx: Arc<ContextData>) -> Result<(), Error> {
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
    let rebuild_token = frontend_extension_rebuild_token(&fe);
    let artifact_key =
        artifact_key(&source_hash, &rebuild_token).context(FrontendExtensionArtifactKeySnafu)?;
    let package_name = frontend_extension_package_name(&fe);
    let artifact_name = artifact_configmap_name(&package_name, &artifact_key);
    let package_attempts =
        list_package_jobs_for_artifact_key(&job_api, &work_ns, &fe, &artifact_key).await?;
    let latest_attempt = latest_package_attempt(package_attempts);

    info!(
        fe = %fe_name,
        source_hash = %source_hash,
        artifact_key = %artifact_key,
        rebuild_token = %rebuild_token,
        phase = ?fe.status.as_ref().map(|s| &s.phase),
        "frontend extension reconcile started"
    );

    if let Err(err) = validate_frontend_extension(&fe) {
        patch_fe_status(
            &fe_api,
            &fe,
            failed_fe_status(
                &fe,
                &source_hash,
                &rebuild_token,
                &artifact_key,
                None,
                "InvalidSource",
                &err.to_string(),
            ),
        )
        .await?;
        return Ok(Action::await_change());
    }

    if let Some(cm) =
        get_artifact_configmap_opt(&artifact_api, &artifact_ns, &artifact_name).await?
        && let Some(metadata) = artifact_metadata_from_configmap(&cm, &source_hash, &artifact_key)
    {
        let gc_keep_names = artifact_gc_keep_names(&fe, &cm);
        let publish = sync_publish(
            &fe,
            &job_api,
            &work_ns,
            &ctx.config,
            &artifact_key,
            &metadata,
            &cm,
        )
        .await?;
        let mut status = ready_fe_status(
            &fe,
            &source_hash,
            &rebuild_token,
            &artifact_key,
            &cm,
            metadata,
            current_or_existing_package_job(latest_attempt.as_ref().map(|a| &a.job), &fe),
        );
        apply_publish_sync(&mut status, &publish);
        patch_fe_status(&fe_api, &fe, status).await?;
        gc_artifact_configmaps(
            &artifact_api,
            &artifact_ns,
            &fe,
            &gc_keep_names,
            ctx.config.artifact_retain_old_count,
        )
        .await?;
        return Ok(requeue_if_publish_running(
            &publish,
            ctx.config.reconcile_requeue_seconds,
        ));
    }

    if let Some(attempt) = latest_attempt.as_ref() {
        let job = &attempt.job;
        match observed_job_phase(job.status.as_ref()) {
            ObservedJobPhase::Pending | ObservedJobPhase::Running => {
                patch_fe_status(
                    &fe_api,
                    &fe,
                    packaging_fe_status(
                        &fe,
                        &source_hash,
                        &rebuild_token,
                        &artifact_key,
                        job,
                        "Package job in progress",
                    ),
                )
                .await?;
                return Ok(Action::requeue(Duration::from_secs(
                    ctx.config.reconcile_requeue_seconds,
                )));
            }
            ObservedJobPhase::Failed => {
                if attempt.attempt >= ctx.config.package_max_attempts {
                    let latest_message = extract_job_message(job)
                        .unwrap_or_else(|| "Package job failed".to_string());
                    let message = package_attempts_exceeded_message(
                        &artifact_key,
                        attempt.attempt,
                        ctx.config.package_max_attempts,
                        &latest_message,
                    );
                    patch_fe_status(
                        &fe_api,
                        &fe,
                        failed_fe_status(
                            &fe,
                            &source_hash,
                            &rebuild_token,
                            &artifact_key,
                            Some(job),
                            "PackageAttemptsExceeded",
                            &message,
                        ),
                    )
                    .await?;
                    return Ok(Action::await_change());
                }
                let message =
                    extract_job_message(job).unwrap_or_else(|| "Package job failed".to_string());
                info!(
                    fe = %fe_name,
                    attempt = attempt.attempt,
                    max_attempts = ctx.config.package_max_attempts,
                    message,
                    "package job failed; creating next attempt"
                );
            }
            ObservedJobPhase::Succeeded => {
                if let Some(cm) =
                    get_artifact_configmap_opt(&artifact_api, &artifact_ns, &artifact_name).await?
                    && let Some(metadata) =
                        artifact_metadata_from_configmap(&cm, &source_hash, &artifact_key)
                {
                    let gc_keep_names = artifact_gc_keep_names(&fe, &cm);
                    let publish = sync_publish(
                        &fe,
                        &job_api,
                        &work_ns,
                        &ctx.config,
                        &artifact_key,
                        &metadata,
                        &cm,
                    )
                    .await?;
                    let mut status = ready_fe_status(
                        &fe,
                        &source_hash,
                        &rebuild_token,
                        &artifact_key,
                        &cm,
                        metadata,
                        Some(package_job_status(job)),
                    );
                    apply_publish_sync(&mut status, &publish);
                    patch_fe_status(&fe_api, &fe, status).await?;
                    gc_artifact_configmaps(
                        &artifact_api,
                        &artifact_ns,
                        &fe,
                        &gc_keep_names,
                        ctx.config.artifact_retain_old_count,
                    )
                    .await?;
                    return Ok(requeue_if_publish_running(
                        &publish,
                        ctx.config.reconcile_requeue_seconds,
                    ));
                }

                if attempt.attempt >= ctx.config.package_max_attempts {
                    let latest_message =
                        "Package job succeeded but artifact ConfigMap is missing or mismatched";
                    let message = package_attempts_exceeded_message(
                        &artifact_key,
                        attempt.attempt,
                        ctx.config.package_max_attempts,
                        latest_message,
                    );
                    patch_fe_status(
                        &fe_api,
                        &fe,
                        failed_fe_status(
                            &fe,
                            &source_hash,
                            &rebuild_token,
                            &artifact_key,
                            Some(job),
                            "PackageAttemptsExceeded",
                            &message,
                        ),
                    )
                    .await?;
                    return Ok(Action::await_change());
                }
                info!(
                    fe = %fe_name,
                    attempt = attempt.attempt,
                    max_attempts = ctx.config.package_max_attempts,
                    "package job succeeded without matching artifact; creating next attempt"
                );
            }
        }
    }

    let next_attempt = latest_attempt
        .as_ref()
        .map_or(1, |attempt| attempt.attempt.saturating_add(1));
    if next_attempt > ctx.config.package_max_attempts {
        let latest_message = latest_attempt
            .as_ref()
            .and_then(|attempt| extract_job_message(&attempt.job))
            .unwrap_or_else(|| "No package attempts can be created".to_string());
        let message = package_attempts_exceeded_message(
            &artifact_key,
            next_attempt.saturating_sub(1),
            ctx.config.package_max_attempts,
            &latest_message,
        );
        patch_fe_status(
            &fe_api,
            &fe,
            failed_fe_status(
                &fe,
                &source_hash,
                &rebuild_token,
                &artifact_key,
                latest_attempt.as_ref().map(|attempt| &attempt.job),
                "PackageAttemptsExceeded",
                &message,
            ),
        )
        .await?;
        return Ok(Action::await_change());
    }

    let job_name = package_job_name(&fe_name, &artifact_key, next_attempt);
    let desired_job = make_package_job(
        &fe,
        &ctx.config,
        &job_name,
        &source_hash,
        &artifact_key,
        &rebuild_token,
        &artifact_name,
    );
    let job = create_or_get_job(&job_api, &work_ns, desired_job, &job_name).await?;
    patch_fe_status(
        &fe_api,
        &fe,
        packaging_fe_status(
            &fe,
            &source_hash,
            &rebuild_token,
            &artifact_key,
            &job,
            "Package job created",
        ),
    )
    .await?;
    Ok(Action::requeue(Duration::from_secs(
        ctx.config.reconcile_requeue_seconds,
    )))
}

#[derive(Clone, Debug)]
struct PackageAttempt {
    attempt: u32,
    job: Job,
}

async fn list_package_jobs_for_artifact_key(
    job_api: &Api<Job>,
    namespace: &str,
    fe: &FrontendExtension,
    artifact_key: &str,
) -> Result<Vec<PackageAttempt>, Error> {
    let fe_name = fe.name_any();
    let selector = package_job_selector(fe, artifact_key);
    let jobs = job_api
        .list(&ListParams::default().labels(&selector))
        .await
        .with_context(|_| ListPackageJobsForArtifactKeySnafu {
            namespace: namespace.to_string(),
            fe_name: fe_name.clone(),
            artifact_key: artifact_key.to_string(),
        })?;
    Ok(jobs
        .items
        .into_iter()
        .filter_map(|job| {
            package_attempt_from_job(&job).map(|attempt| PackageAttempt { attempt, job })
        })
        .collect())
}

fn package_job_selector(fe: &FrontendExtension, artifact_key: &str) -> String {
    format!(
        "{}={},{}={},{}={},{}={}",
        LABEL_FE_NAME,
        fe.name_any(),
        LABEL_FE_UID,
        frontend_extension_uid_label(fe),
        LABEL_ARTIFACT_KEY_SHORT,
        hash_label_value(artifact_key),
        LABEL_PACKAGE_KIND,
        PACKAGE_KIND_VALUE
    )
}

fn package_attempt_from_job(job: &Job) -> Option<u32> {
    let name = job.name_any();
    let (_, attempt) = name.rsplit_once("-a")?;
    let attempt = attempt.parse::<u32>().ok()?;
    (attempt > 0).then_some(attempt)
}

fn latest_package_attempt(attempts: Vec<PackageAttempt>) -> Option<PackageAttempt> {
    attempts.into_iter().max_by(|a, b| {
        a.attempt
            .cmp(&b.attempt)
            .then_with(|| {
                a.job
                    .metadata
                    .creation_timestamp
                    .cmp(&b.job.metadata.creation_timestamp)
            })
            .then_with(|| a.job.name_any().cmp(&b.job.name_any()))
    })
}

fn frontend_extension_rebuild_token(fe: &FrontendExtension) -> String {
    fe.metadata
        .annotations
        .as_ref()
        .and_then(|annos| annos.get(ANNO_REBUILD_TOKEN))
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty())
        .unwrap_or_default()
}

fn frontend_extension_uid_label(fe: &FrontendExtension) -> String {
    fe.meta().uid.clone().unwrap_or_default()
}

fn package_attempts_exceeded_message(
    artifact_key: &str,
    latest_attempt: u32,
    max_attempts: u32,
    latest_message: &str,
) -> String {
    format!(
        "Package attempts exceeded for artifactKey short {}: latest attempt {}, max attempts {}. \
         Latest job failure: {}",
        hash_label_value(artifact_key),
        latest_attempt,
        max_attempts,
        latest_message
    )
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

async fn gc_artifact_configmaps(
    cm_api: &Api<ConfigMap>,
    namespace: &str,
    fe: &FrontendExtension,
    keep_names: &BTreeSet<String>,
    retain_old_count: usize,
) -> Result<(), Error> {
    let selector = format!(
        "{}={},{}={}",
        LABEL_FE_NAME,
        fe.name_any(),
        LABEL_PACKAGE_KIND,
        PACKAGE_KIND_VALUE
    );
    let configmaps = cm_api
        .list(&ListParams::default().labels(&selector))
        .await
        .with_context(|_| ListArtifactConfigMapsForGcSnafu {
            namespace: namespace.to_string(),
        })?;

    let delete_names =
        artifact_configmap_gc_candidates(configmaps.items, fe, keep_names, retain_old_count);
    for name in delete_names {
        match cm_api.delete(&name, &DeleteParams::default()).await {
            Ok(_) => {
                info!(
                    fe = %fe.name_any(),
                    namespace,
                    configmap = %name,
                    "deleted stale FrontendExtension artifact ConfigMap"
                );
            }
            Err(kube::Error::Api(ae)) if ae.code == 404 => {}
            Err(source) => {
                return Err(Error::DeleteArtifactConfigMap {
                    namespace: namespace.to_string(),
                    name,
                    source: Box::new(source),
                });
            }
        }
    }

    Ok(())
}

fn artifact_gc_keep_names(fe: &FrontendExtension, current_cm: &ConfigMap) -> BTreeSet<String> {
    let mut keep_names = BTreeSet::from([current_cm.name_any()]);
    if let Some(name) = status_artifact_configmap_name(fe) {
        keep_names.insert(name.to_string());
    }
    keep_names
}

fn status_artifact_configmap_name(fe: &FrontendExtension) -> Option<&str> {
    let artifact = fe.status.as_ref()?.artifact.as_ref()?;
    if artifact.storage.kind != ArtifactStorageKind::ConfigMap {
        return None;
    }
    Some(artifact.storage.ref_.name.as_str())
}

fn artifact_configmap_gc_candidates(
    configmaps: Vec<ConfigMap>,
    fe: &FrontendExtension,
    keep_names: &BTreeSet<String>,
    retain_old_count: usize,
) -> Vec<String> {
    let mut candidates = configmaps
        .into_iter()
        .filter(|cm| artifact_configmap_is_owned_by(cm, fe))
        .filter(|cm| !keep_names.contains(&cm.name_any()))
        .collect::<Vec<_>>();

    candidates.sort_by(|a, b| {
        b.metadata
            .creation_timestamp
            .cmp(&a.metadata.creation_timestamp)
            .then_with(|| b.name_any().cmp(&a.name_any()))
    });

    candidates
        .into_iter()
        .skip(retain_old_count)
        .map(|cm| cm.name_any())
        .collect()
}

fn artifact_configmap_is_owned_by(cm: &ConfigMap, fe: &FrontendExtension) -> bool {
    let Some(fe_uid) = fe.meta().uid.as_deref() else {
        return false;
    };
    cm.metadata
        .owner_references
        .as_ref()
        .is_some_and(|owners| owners.iter().any(|owner| owner.uid == fe_uid))
}

fn artifact_metadata_from_configmap(
    cm: &ConfigMap,
    source_hash: &str,
    artifact_key: &str,
) -> Option<PackageArtifactMetadata> {
    let annotations = cm.metadata.annotations.as_ref()?;
    if annotations.get(ANNO_SOURCE_HASH).map(String::as_str) != Some(source_hash) {
        return None;
    }
    if annotations.get(ANNO_ARTIFACT_KEY).map(String::as_str) != Some(artifact_key) {
        return None;
    }

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
    let observed_digest = format!("sha256:{}", sha256_hex(&bytes.0));
    if observed_digest != metadata.digest {
        return None;
    }
    if annotations.get(ANNO_ARTIFACT_DIGEST).map(String::as_str) != Some(metadata.digest.as_str()) {
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
    config: &ControllerConfig,
    artifact_key: &str,
    artifact: &PackageArtifactMetadata,
    artifact_cm: &ConfigMap,
) -> Result<PublishSync, Error> {
    let Some(request_id) = publish_request_id(fe) else {
        return Ok(PublishSync {
            status: Some(current_publish_for_artifact(
                fe,
                &artifact.digest,
                artifact_key,
            )),
            should_requeue: false,
        });
    };

    let request = match publish_request(fe, request_id, &artifact.digest) {
        Ok(request) => request,
        Err(status) => {
            return Ok(PublishSync {
                status: Some(*status),
                should_requeue: false,
            });
        }
    };
    let job_name = publish_job_name(&fe.name_any(), &request.request_id);
    let job = if let Some(job) =
        job_api
            .get_opt(&job_name)
            .await
            .with_context(|_| GetPublishJobSnafu {
                namespace: namespace.to_string(),
                name: job_name.clone(),
            })? {
        job
    } else {
        if publish_already_finished(fe, &request, artifact_key) {
            return Ok(PublishSync {
                status: fe.status.as_ref().and_then(|status| status.publish.clone()),
                should_requeue: false,
            });
        }
        let desired_job = make_publish_job(fe, config, &job_name, &request, artifact, artifact_cm);
        create_or_get_job(job_api, namespace, desired_job, &job_name).await?
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
) -> Result<PublishRequest, Box<PublishStatus>> {
    let annos = fe.metadata.annotations.as_ref();
    let requested_digest = annos
        .and_then(|annos| annos.get(ANNO_PUBLISH_ARTIFACT_DIGEST))
        .filter(|digest| !digest.is_empty())
        .cloned()
        .ok_or_else(|| {
            Box::new(failed_publish_status(
                request_id,
                None,
                "publish artifact digest annotation is required",
            ))
        })?;

    if requested_digest != current_artifact_digest {
        return Err(Box::new(failed_publish_status(
            request_id,
            Some(requested_digest),
            "publish artifact digest does not match current ready artifact",
        )));
    }

    let target_ref = publish_target_ref(fe).ok_or_else(|| {
        Box::new(failed_publish_status(
            request_id,
            Some(current_artifact_digest.to_string()),
            "publish targetRef is required",
        ))
    })?;
    let target_kind = publish_target_kind(fe);
    if !matches!(target_kind.as_str(), "ConfigMap" | "Secret") {
        return Err(Box::new(failed_publish_status(
            request_id,
            Some(current_artifact_digest.to_string()),
            "publish targetKind must be ConfigMap or Secret",
        )));
    }

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

fn publish_target_kind(fe: &FrontendExtension) -> String {
    fe.metadata
        .annotations
        .as_ref()
        .and_then(|annos| annos.get(ANNO_PUBLISH_TARGET_KIND))
        .filter(|kind| !kind.is_empty())
        .cloned()
        .or_else(|| {
            fe.spec
                .publish_policy
                .as_ref()
                .and_then(|policy| policy.default_target_kind.as_ref())
                .map(|kind| match kind {
                    PublishTargetKind::ConfigMap => "ConfigMap".to_string(),
                    PublishTargetKind::Secret => "Secret".to_string(),
                })
        })
        .unwrap_or_else(|| "ConfigMap".to_string())
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
    artifact_key: &str,
) -> PublishStatus {
    let publish = fe.status.as_ref().and_then(|status| status.publish.clone());
    match publish {
        Some(status)
            if status.artifact_digest.as_deref() == Some(artifact_digest)
                && current_status_artifact_key(fe) == Some(artifact_key) =>
        {
            status
        }
        _ => PublishStatus::default(),
    }
}

fn publish_already_finished(
    fe: &FrontendExtension,
    request: &PublishRequest,
    artifact_key: &str,
) -> bool {
    fe.status
        .as_ref()
        .and_then(|status| status.publish.as_ref())
        .is_some_and(|publish| {
            publish.request_id.as_deref() == Some(request.request_id.as_str())
                && publish.artifact_digest.as_deref() == Some(request.artifact_digest.as_str())
                && current_status_artifact_key(fe) == Some(artifact_key)
                && matches!(
                    publish.phase,
                    PublishPhase::Succeeded | PublishPhase::Failed
                )
        })
}

fn current_status_artifact_key(fe: &FrontendExtension) -> Option<&str> {
    fe.status
        .as_ref()
        .and_then(|status| status.artifact.as_ref())
        .and_then(|artifact| artifact.artifact_key.as_deref())
}

fn make_publish_job(
    fe: &FrontendExtension,
    config: &ControllerConfig,
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
            value: Some(fe_name),
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
    status.publish.clone_from(&publish.status);
    let generation = status.observed_generation;
    status
        .conditions
        .retain(|condition| condition.type_ != "PublishSucceeded");
    status.conditions.push(fe_publish_condition_from_status(
        status.publish.as_ref(),
        generation,
    ));
}

fn requeue_if_publish_running(publish: &PublishSync, requeue_seconds: u64) -> Action {
    if publish.should_requeue {
        Action::requeue(Duration::from_secs(requeue_seconds))
    } else {
        Action::await_change()
    }
}

fn make_package_job(
    fe: &FrontendExtension,
    config: &ControllerConfig,
    job_name: &str,
    source_hash: &str,
    artifact_key: &str,
    rebuild_token: &str,
    artifact_configmap_name: &str,
) -> Job {
    let fe_name = fe.name_any();
    let fe_uid = frontend_extension_uid_label(fe);
    let mut labels = BTreeMap::from([
        (LABEL_MANAGED_BY.to_string(), MANAGED_BY_VALUE.to_string()),
        (LABEL_FE_NAME.to_string(), fe_name.clone()),
        (LABEL_FE_UID.to_string(), fe_uid.clone()),
        (
            LABEL_SOURCE_HASH_SHORT.to_string(),
            hash_label_value(source_hash),
        ),
        (
            LABEL_ARTIFACT_KEY_SHORT.to_string(),
            hash_label_value(artifact_key),
        ),
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
    annotations.insert(ANNO_SOURCE_HASH.to_string(), source_hash.to_string());
    annotations.insert(ANNO_ARTIFACT_KEY.to_string(), artifact_key.to_string());
    if let Some(generation) = fe.metadata.generation {
        annotations.insert(ANNO_OBSERVED_GENERATION.to_string(), generation.to_string());
    }

    let env = vec![
        EnvVar {
            name: "FE_NAME".to_string(),
            value: Some(fe_name),
            ..Default::default()
        },
        EnvVar {
            name: "FE_UID".to_string(),
            value: Some(fe_uid),
            ..Default::default()
        },
        EnvVar {
            name: "SOURCE_HASH".to_string(),
            value: Some(source_hash.to_string()),
            ..Default::default()
        },
        EnvVar {
            name: "ARTIFACT_KEY".to_string(),
            value: Some(artifact_key.to_string()),
            ..Default::default()
        },
        EnvVar {
            name: "REBUILD_TOKEN".to_string(),
            value: Some(rebuild_token.to_string()),
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
        EnvVar {
            name: "BUILD_SERVICE_BASE_URL".to_string(),
            value: Some(config.build_service_base_url.clone()),
            ..Default::default()
        },
        EnvVar {
            name: "BUILD_SERVICE_TIMEOUT_SECONDS".to_string(),
            value: Some(config.build_service_timeout_seconds.to_string()),
            ..Default::default()
        },
        EnvVar {
            name: "JSBUNDLE_CONFIG_KEY".to_string(),
            value: Some(config.jsbundle_config_key.clone()),
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
    let nanos = u32::try_from(nanos).ok()?;
    chrono::DateTime::from_timestamp(time.0.as_second(), nanos)
}

fn existing_package_job(fe: &FrontendExtension) -> Option<PackageJobStatus> {
    fe.status
        .as_ref()
        .and_then(|status| status.package_job.clone())
}

fn current_or_existing_package_job(
    current_job: Option<&Job>,
    fe: &FrontendExtension,
) -> Option<PackageJobStatus> {
    current_job
        .map(package_job_status)
        .or_else(|| existing_package_job(fe))
}

fn packaging_fe_status(
    fe: &FrontendExtension,
    source_hash: &str,
    rebuild_token: &str,
    artifact_key: &str,
    job: &Job,
    message: &str,
) -> FrontendExtensionStatus {
    let generation = Some(fe.metadata.generation.unwrap_or_default());
    let publish = retained_publish_for_artifact_key(fe, artifact_key);
    FrontendExtensionStatus {
        phase: FrontendExtensionPhase::Packaging,
        observed_generation: generation,
        observed_source_hash: Some(source_hash.to_string()),
        observed_rebuild_token: Some(rebuild_token.to_string()),
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
    rebuild_token: &str,
    artifact_key: &str,
    cm: &ConfigMap,
    metadata: PackageArtifactMetadata,
    package_job: Option<PackageJobStatus>,
) -> FrontendExtensionStatus {
    let generation = Some(fe.metadata.generation.unwrap_or_default());
    FrontendExtensionStatus {
        phase: FrontendExtensionPhase::Ready,
        observed_generation: generation,
        observed_source_hash: Some(source_hash.to_string()),
        observed_rebuild_token: Some(rebuild_token.to_string()),
        artifact: Some(ExtensionArtifactStatus {
            storage: ArtifactStorageStatus {
                kind: ArtifactStorageKind::ConfigMap,
                ref_: namespaced_ref(cm),
                key: PACKAGE_KEY.to_string(),
            },
            digest: metadata.digest.clone(),
            size_bytes: i64::try_from(metadata.size_bytes).unwrap_or(i64::MAX),
            media_type: metadata.media_type.clone(),
            filename: metadata.filename.clone(),
            generated_at: metadata.generated_at,
            source_hash: metadata.source_hash,
            artifact_key: Some(artifact_key.to_string()),
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
    rebuild_token: &str,
    artifact_key: &str,
    job: Option<&Job>,
    reason: &str,
    message: &str,
) -> FrontendExtensionStatus {
    let generation = Some(fe.metadata.generation.unwrap_or_default());
    let publish = retained_publish_for_artifact_key(fe, artifact_key);
    FrontendExtensionStatus {
        phase: FrontendExtensionPhase::Failed,
        observed_generation: generation,
        observed_source_hash: Some(source_hash.to_string()),
        observed_rebuild_token: Some(rebuild_token.to_string()),
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
                message,
                generation,
            ),
            fe_condition("ArtifactReady", "False", reason, message, generation),
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

fn retained_publish_for_artifact_key(
    fe: &FrontendExtension,
    artifact_key: &str,
) -> Option<PublishStatus> {
    let status = fe.status.as_ref()?;
    if status
        .artifact
        .as_ref()
        .is_some_and(|artifact| artifact.artifact_key.as_deref() == Some(artifact_key))
    {
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
    } else if status
        .package_job
        .as_ref()
        .is_some_and(|package_job| package_job.message.is_none())
        && let Some(package_job) = status_object
            .get_mut("packageJob")
            .and_then(serde_json::Value::as_object_mut)
    {
        package_job.insert("message".to_string(), serde_json::Value::Null);
    }
    if status.publish.is_none() {
        status_object.insert("publish".to_string(), serde_json::Value::Null);
    } else if status
        .publish
        .as_ref()
        .is_some_and(|publish| publish.last_error.is_none())
        && let Some(publish) = status_object
            .get_mut("publish")
            .and_then(serde_json::Value::as_object_mut)
    {
        publish.insert("lastError".to_string(), serde_json::Value::Null);
    }

    Ok(json!({
        "status": status_value,
    }))
}

#[cfg(test)]
mod tests {
    use k8s_openapi::{
        ByteString, api::batch::v1::JobStatus, apimachinery::pkg::apis::meta::v1::OwnerReference,
    };

    use super::*;

    fn test_time(value: &str) -> Time {
        serde_json::from_value(json!(value)).unwrap()
    }

    fn artifact_cm(name: &str, fe_uid: &str, created_at: &str) -> ConfigMap {
        ConfigMap {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                creation_timestamp: Some(test_time(created_at)),
                owner_references: Some(vec![OwnerReference {
                    api_version: "frontend-forge.kubesphere.io/v1alpha1".to_string(),
                    kind: "FrontendExtension".to_string(),
                    name: "inspecttask".to_string(),
                    uid: fe_uid.to_string(),
                    controller: Some(true),
                    block_owner_deletion: None,
                }]),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn sample_fe() -> FrontendExtension {
        serde_json::from_value(json!({
            "apiVersion": "frontend-forge.kubesphere.io/v1alpha1",
            "kind": "FrontendExtension",
            "metadata": {
                "name": "inspecttask",
                "uid": "fe-uid",
                "generation": 7,
            },
            "spec": {
                "package": {
                    "version": "0.1.0",
                    "displayName": { "en": "Inspect Task" },
                    "description": { "en": "InspectTask extension package" }
                },
                "source": {
                    "type": "Inline",
                    "inline": {
                        "schemaVersion": "v1",
                        "frontend": {}
                    }
                }
            }
        }))
        .unwrap()
    }

    fn test_config() -> ControllerConfig {
        ControllerConfig {
            work_namespace: "extension-frontend-forge".to_string(),
            packager_image: "packager:latest".to_string(),
            packager_service_account: None,
            publisher_image: "publisher:latest".to_string(),
            publisher_service_account: Some("publisher-sa".to_string()),
            artifact_configmap_namespace: "extension-frontend-forge".to_string(),
            build_service_base_url: "http://frontend-forge.test".to_string(),
            build_service_timeout_seconds: 240,
            jsbundle_config_key: "index.js".to_string(),
            reconcile_requeue_seconds: 5,
            job_active_deadline_seconds: 300,
            job_ttl_seconds_after_finished: Some(3600),
            artifact_retain_old_count: 1,
            package_max_attempts: 3,
        }
    }

    #[test]
    fn rebuild_token_is_trimmed_and_empty_tokens_are_equivalent() {
        let mut fe = sample_fe();
        fe.metadata.annotations = Some(BTreeMap::from([(
            ANNO_REBUILD_TOKEN.to_string(),
            " token-1 ".to_string(),
        )]));

        assert_eq!(frontend_extension_rebuild_token(&fe), "token-1");

        fe.metadata.annotations = Some(BTreeMap::from([(
            ANNO_REBUILD_TOKEN.to_string(),
            "   ".to_string(),
        )]));

        assert_eq!(frontend_extension_rebuild_token(&fe), "");
    }

    #[test]
    fn package_job_selector_contains_fe_uid_artifact_key_and_kind() {
        let fe = sample_fe();
        let artifact_key = "sha256:0123456789abcdef0123456789abcdef";
        let selector = package_job_selector(&fe, artifact_key);

        assert!(selector.contains(&format!("{LABEL_FE_NAME}=inspecttask")));
        assert!(selector.contains(&format!("{LABEL_FE_UID}=fe-uid")));
        assert!(selector.contains(&format!(
            "{LABEL_ARTIFACT_KEY_SHORT}={}",
            hash_label_value(artifact_key)
        )));
        assert!(selector.contains(&format!("{LABEL_PACKAGE_KIND}={PACKAGE_KIND_VALUE}")));
    }

    #[test]
    fn package_attempt_parses_positive_suffix_only() {
        let job = Job {
            metadata: ObjectMeta {
                name: Some("fe-inspecttask-package-d46b92fa1234-a12".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        let invalid = Job {
            metadata: ObjectMeta {
                name: Some("fe-inspecttask-package-d46b92fa1234".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        let zero = Job {
            metadata: ObjectMeta {
                name: Some("fe-inspecttask-package-d46b92fa1234-a0".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        assert_eq!(package_attempt_from_job(&job), Some(12));
        assert_eq!(package_attempt_from_job(&invalid), None);
        assert_eq!(package_attempt_from_job(&zero), None);
    }

    #[test]
    fn latest_package_attempt_uses_highest_parsed_attempt() {
        let attempt_1 = PackageAttempt {
            attempt: 1,
            job: Job {
                metadata: ObjectMeta {
                    name: Some("fe-inspecttask-package-hash-a1".to_string()),
                    creation_timestamp: Some(test_time("2026-04-20T10:01:00Z")),
                    ..Default::default()
                },
                ..Default::default()
            },
        };
        let attempt_3 = PackageAttempt {
            attempt: 3,
            job: Job {
                metadata: ObjectMeta {
                    name: Some("fe-inspecttask-package-hash-a3".to_string()),
                    creation_timestamp: Some(test_time("2026-04-20T10:00:00Z")),
                    ..Default::default()
                },
                ..Default::default()
            },
        };

        let latest = latest_package_attempt(vec![attempt_1, attempt_3]).unwrap();

        assert_eq!(latest.attempt, 3);
        assert_eq!(latest.job.name_any(), "fe-inspecttask-package-hash-a3");
    }

    #[test]
    fn package_attempts_exceeded_message_includes_attempt_context() {
        let message = package_attempts_exceeded_message("sha256:d46b92fa1234abcdef", 3, 3, "boom");

        assert!(message.contains("d46b92fa1234abcdef"));
        assert!(message.contains("latest attempt 3"));
        assert!(message.contains("max attempts 3"));
        assert!(message.contains("boom"));
    }

    #[test]
    fn package_job_has_rebuild_identity_labels_annotations_and_env() {
        let fe = sample_fe();
        let config = test_config();
        let job = make_package_job(
            &fe,
            &config,
            "fe-inspecttask-package-d46b92fa1234-a1",
            "sha256:source",
            "sha256:artifactkey",
            "token-1",
            "fe-inspecttask-d46b92fa1234",
        );
        let labels = job.metadata.labels.as_ref().unwrap();
        let annotations = job.metadata.annotations.as_ref().unwrap();
        let env = job
            .spec
            .as_ref()
            .unwrap()
            .template
            .spec
            .as_ref()
            .unwrap()
            .containers[0]
            .env
            .as_ref()
            .unwrap()
            .iter()
            .map(|env| (env.name.as_str(), env.value.as_deref().unwrap_or_default()))
            .collect::<BTreeMap<_, _>>();

        assert_eq!(labels[LABEL_FE_NAME], "inspecttask");
        assert_eq!(labels[LABEL_FE_UID], "fe-uid");
        assert_eq!(labels[LABEL_SOURCE_HASH_SHORT], "source");
        assert_eq!(labels[LABEL_ARTIFACT_KEY_SHORT], "artifactkey");
        assert_eq!(annotations[ANNO_SOURCE_HASH], "sha256:source");
        assert_eq!(annotations[ANNO_ARTIFACT_KEY], "sha256:artifactkey");
        assert_eq!(env["FE_UID"], "fe-uid");
        assert_eq!(env["ARTIFACT_KEY"], "sha256:artifactkey");
        assert_eq!(env["REBUILD_TOKEN"], "token-1");
    }

    #[test]
    fn artifact_configmap_requires_matching_artifact_key_annotation() {
        let bytes = vec![1, 2, 3];
        let digest = format!("sha256:{}", sha256_hex(&bytes));
        let metadata = PackageArtifactMetadata {
            name: "inspecttask".to_string(),
            version: "0.1.0".to_string(),
            filename: "inspecttask-0.1.0.tgz".to_string(),
            media_type: "application/gzip".to_string(),
            digest: digest.clone(),
            size_bytes: 3,
            source_hash: "sha256:source".to_string(),
            generated_at: Utc::now(),
        };
        let mut cm = ConfigMap {
            metadata: ObjectMeta {
                annotations: Some(BTreeMap::from([
                    (ANNO_SOURCE_HASH.to_string(), "sha256:source".to_string()),
                    (
                        ANNO_ARTIFACT_KEY.to_string(),
                        "sha256:artifactkey".to_string(),
                    ),
                    (ANNO_ARTIFACT_DIGEST.to_string(), digest),
                ])),
                ..Default::default()
            },
            data: Some(BTreeMap::from([(
                ARTIFACT_METADATA_KEY.to_string(),
                serde_json::to_string(&metadata).unwrap(),
            )])),
            binary_data: Some(BTreeMap::from([(
                PACKAGE_KEY.to_string(),
                ByteString(bytes),
            )])),
            ..Default::default()
        };

        assert!(
            artifact_metadata_from_configmap(&cm, "sha256:source", "sha256:artifactkey").is_some()
        );

        cm.metadata
            .annotations
            .as_mut()
            .unwrap()
            .insert(ANNO_ARTIFACT_KEY.to_string(), "sha256:other".to_string());

        assert!(
            artifact_metadata_from_configmap(&cm, "sha256:source", "sha256:artifactkey").is_none()
        );
    }

    #[test]
    fn publish_status_is_retained_only_for_same_artifact_key() {
        let mut fe = sample_fe();
        fe.status = Some(FrontendExtensionStatus {
            phase: FrontendExtensionPhase::Ready,
            observed_generation: Some(7),
            observed_source_hash: Some("sha256:source".to_string()),
            observed_rebuild_token: Some("token-1".to_string()),
            artifact: Some(ExtensionArtifactStatus {
                storage: ArtifactStorageStatus {
                    kind: ArtifactStorageKind::ConfigMap,
                    ref_: NamespacedResourceRef {
                        namespace: "extension-frontend-forge".to_string(),
                        name: "fe-inspecttask-d46b92fa1234".to_string(),
                        uid: None,
                    },
                    key: PACKAGE_KEY.to_string(),
                },
                digest: "sha256:artifact".to_string(),
                size_bytes: 1,
                media_type: "application/gzip".to_string(),
                filename: "inspecttask-0.1.0.tgz".to_string(),
                generated_at: Utc::now(),
                source_hash: "sha256:source".to_string(),
                artifact_key: Some("sha256:artifactkey".to_string()),
            }),
            publish: Some(PublishStatus {
                phase: PublishPhase::Succeeded,
                artifact_digest: Some("sha256:artifact".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        });

        assert_eq!(
            retained_publish_for_artifact_key(&fe, "sha256:artifactkey")
                .unwrap()
                .phase,
            PublishPhase::Succeeded
        );
        assert_eq!(
            retained_publish_for_artifact_key(&fe, "sha256:newkey")
                .unwrap()
                .phase,
            PublishPhase::NotRequested
        );
    }

    #[test]
    fn current_job_status_overrides_existing_package_job() {
        let fe: FrontendExtension = serde_json::from_value(json!({
            "apiVersion": "frontend-forge.kubesphere.io/v1alpha1",
            "kind": "FrontendExtension",
            "metadata": {
                "name": "inspecttask",
            },
            "spec": {
                "package": {
                    "version": "0.1.0",
                    "displayName": {
                        "en": "Inspect Task",
                    },
                    "description": {
                        "en": "InspectTask extension package",
                    },
                },
                "source": {
                    "type": "Inline",
                    "inline": {
                        "schemaVersion": "v1",
                        "frontend": {
                            "menus": [{
                                "displayName": "Inspect Tasks",
                                "key": "inspecttasks",
                                "placement": "cluster",
                                "type": "page",
                            }],
                            "pages": [{
                                "key": "inspecttasks",
                                "type": "iframe",
                                "iframe": {
                                    "src": "http://example.test",
                                },
                            }],
                        },
                    },
                },
            },
            "status": {
                "phase": "Ready",
                "packageJob": {
                    "namespace": "extension-frontend-forge",
                    "name": "fe-inspecttask-package-oldhash",
                    "phase": "Running",
                },
            },
        }))
        .unwrap();

        let job = Job {
            metadata: ObjectMeta {
                name: Some("fe-inspecttask-package-newhash".to_string()),
                namespace: Some("extension-frontend-forge".to_string()),
                uid: Some("job-uid".to_string()),
                ..Default::default()
            },
            status: Some(JobStatus {
                succeeded: Some(1),
                ..Default::default()
            }),
            ..Default::default()
        };

        let package_job = current_or_existing_package_job(Some(&job), &fe).unwrap();

        assert_eq!(package_job.name, "fe-inspecttask-package-newhash");
        assert_eq!(package_job.phase, PackageJobPhase::Succeeded);
    }

    #[test]
    fn existing_package_job_is_fallback_when_current_job_missing() {
        let fe: FrontendExtension = serde_json::from_value(json!({
            "apiVersion": "frontend-forge.kubesphere.io/v1alpha1",
            "kind": "FrontendExtension",
            "metadata": {
                "name": "inspecttask",
            },
            "spec": {
                "package": {
                    "version": "0.1.0",
                    "displayName": {
                        "en": "Inspect Task",
                    },
                    "description": {
                        "en": "InspectTask extension package",
                    },
                },
                "source": {
                    "type": "Inline",
                    "inline": {
                        "schemaVersion": "v1",
                        "frontend": {
                            "menus": [{
                                "displayName": "Inspect Tasks",
                                "key": "inspecttasks",
                                "placement": "cluster",
                                "type": "page",
                            }],
                            "pages": [{
                                "key": "inspecttasks",
                                "type": "iframe",
                                "iframe": {
                                    "src": "http://example.test",
                                },
                            }],
                        },
                    },
                },
            },
            "status": {
                "phase": "Ready",
                "packageJob": {
                    "namespace": "extension-frontend-forge",
                    "name": "fe-inspecttask-package-oldhash",
                    "phase": "Succeeded",
                },
            },
        }))
        .unwrap();

        let package_job = current_or_existing_package_job(None, &fe).unwrap();

        assert_eq!(package_job.name, "fe-inspecttask-package-oldhash");
        assert_eq!(package_job.phase, PackageJobPhase::Succeeded);
    }

    #[test]
    fn status_patch_clears_stale_package_job_message() {
        let status = FrontendExtensionStatus {
            phase: FrontendExtensionPhase::Ready,
            observed_generation: Some(1),
            observed_source_hash: Some("sha256:source".to_string()),
            observed_rebuild_token: Some(String::new()),
            artifact: None,
            download: None,
            package_job: Some(PackageJobStatus {
                namespace: "extension-frontend-forge".to_string(),
                name: "fe-inspecttask-package-newhash".to_string(),
                uid: None,
                phase: PackageJobPhase::Succeeded,
                started_at: None,
                finished_at: None,
                message: None,
            }),
            publish: None,
            conditions: vec![],
        };

        let patch = frontend_extension_status_patch(&status, "inspecttask").unwrap();

        assert_eq!(
            patch["status"]["packageJob"]["message"],
            serde_json::Value::Null
        );
    }

    #[test]
    fn status_patch_clears_stale_publish_last_error() {
        let status = FrontendExtensionStatus {
            phase: FrontendExtensionPhase::Ready,
            observed_generation: Some(1),
            observed_source_hash: Some("sha256:source".to_string()),
            observed_rebuild_token: Some(String::new()),
            artifact: None,
            download: None,
            package_job: None,
            publish: Some(PublishStatus {
                phase: PublishPhase::Succeeded,
                request_id: Some("request-1".to_string()),
                artifact_digest: Some("sha256:artifact".to_string()),
                job_ref: None,
                started_at: None,
                finished_at: None,
                last_error: None,
            }),
            conditions: vec![],
        };

        let patch = frontend_extension_status_patch(&status, "inspecttask").unwrap();

        assert_eq!(
            patch["status"]["publish"]["lastError"],
            serde_json::Value::Null
        );
    }

    #[test]
    fn artifact_gc_keeps_current_status_ref_and_recent_old_artifacts() {
        let fe: FrontendExtension = serde_json::from_value(json!({
            "apiVersion": "frontend-forge.kubesphere.io/v1alpha1",
            "kind": "FrontendExtension",
            "metadata": {
                "name": "inspecttask",
                "uid": "fe-uid",
            },
            "spec": {
                "package": {
                    "version": "0.1.0",
                    "displayName": { "en": "Inspect Task" },
                    "description": { "en": "InspectTask extension package" }
                },
                "source": {
                    "type": "Inline",
                    "inline": {
                        "schemaVersion": "v1",
                        "frontend": {}
                    }
                }
            },
            "status": {
                "phase": "Ready",
                "artifact": {
                    "storage": {
                        "kind": "ConfigMap",
                        "ref": {
                            "namespace": "extension-frontend-forge",
                            "name": "fe-inspecttask-previous",
                        },
                        "key": "package.tgz"
                    },
                    "digest": "sha256:previous",
                    "sizeBytes": 1,
                    "mediaType": "application/gzip",
                    "filename": "inspecttask-0.1.0.tgz",
                    "generatedAt": "2026-04-20T10:00:00Z",
                    "sourceHash": "sha256:previous",
                    "artifactKey": "sha256:previous-key"
                }
            }
        }))
        .unwrap();
        let current = artifact_cm("fe-inspecttask-current", "fe-uid", "2026-04-20T10:04:00Z");
        let keep_names = artifact_gc_keep_names(&fe, &current);
        let configmaps = vec![
            current,
            artifact_cm("fe-inspecttask-previous", "fe-uid", "2026-04-20T10:03:00Z"),
            artifact_cm("fe-inspecttask-retained", "fe-uid", "2026-04-20T10:02:00Z"),
            artifact_cm("fe-inspecttask-delete-1", "fe-uid", "2026-04-20T10:01:00Z"),
            artifact_cm("fe-inspecttask-delete-2", "fe-uid", "2026-04-20T10:00:00Z"),
            artifact_cm(
                "fe-inspecttask-other-owner",
                "other-uid",
                "2026-04-20T09:59:00Z",
            ),
        ];

        let delete_names = artifact_configmap_gc_candidates(configmaps, &fe, &keep_names, 1);

        assert_eq!(
            delete_names,
            vec![
                "fe-inspecttask-delete-1".to_string(),
                "fe-inspecttask-delete-2".to_string(),
            ]
        );
    }

    #[test]
    fn artifact_gc_retain_zero_deletes_all_unreferenced_owned_artifacts() {
        let fe: FrontendExtension = serde_json::from_value(json!({
            "apiVersion": "frontend-forge.kubesphere.io/v1alpha1",
            "kind": "FrontendExtension",
            "metadata": {
                "name": "inspecttask",
                "uid": "fe-uid",
            },
            "spec": {
                "package": {
                    "version": "0.1.0",
                    "displayName": { "en": "Inspect Task" },
                    "description": { "en": "InspectTask extension package" }
                },
                "source": {
                    "type": "Inline",
                    "inline": {
                        "schemaVersion": "v1",
                        "frontend": {}
                    }
                }
            }
        }))
        .unwrap();
        let keep_names = BTreeSet::from(["fe-inspecttask-current".to_string()]);
        let configmaps = vec![
            artifact_cm("fe-inspecttask-current", "fe-uid", "2026-04-20T10:02:00Z"),
            artifact_cm("fe-inspecttask-delete", "fe-uid", "2026-04-20T10:01:00Z"),
        ];

        let delete_names = artifact_configmap_gc_candidates(configmaps, &fe, &keep_names, 0);

        assert_eq!(delete_names, vec!["fe-inspecttask-delete".to_string()]);
    }

    #[test]
    fn publish_job_env_includes_artifact_filename_and_target_ref() {
        let fe: FrontendExtension = serde_json::from_value(json!({
            "apiVersion": "frontend-forge.kubesphere.io/v1alpha1",
            "kind": "FrontendExtension",
            "metadata": {
                "name": "inspecttask",
                "generation": 7,
            },
            "spec": {
                "package": {
                    "version": "0.1.0",
                    "displayName": {
                        "en": "Inspect Task",
                    },
                    "description": {
                        "en": "InspectTask extension package",
                    },
                },
                "source": {
                    "type": "Inline",
                    "inline": {
                        "schemaVersion": "v1",
                        "frontend": {},
                    },
                },
            },
        }))
        .unwrap();
        let config = ControllerConfig {
            work_namespace: "extension-frontend-forge".to_string(),
            packager_image: "packager:latest".to_string(),
            packager_service_account: None,
            publisher_image: "publisher:latest".to_string(),
            publisher_service_account: Some("publisher-sa".to_string()),
            artifact_configmap_namespace: "extension-frontend-forge".to_string(),
            build_service_base_url: "http://frontend-forge.test".to_string(),
            build_service_timeout_seconds: 240,
            jsbundle_config_key: "index.js".to_string(),
            reconcile_requeue_seconds: 5,
            job_active_deadline_seconds: 300,
            job_ttl_seconds_after_finished: Some(3600),
            artifact_retain_old_count: 1,
            package_max_attempts: 3,
        };
        let request = PublishRequest {
            request_id: "request-1".to_string(),
            artifact_digest: "sha256:artifact".to_string(),
            target_ref: NamespacedResourceRef {
                namespace: "extension-frontend-forge".to_string(),
                name: "ksbuilder-publish-config".to_string(),
                uid: None,
            },
            target_kind: "Secret".to_string(),
        };
        let artifact = PackageArtifactMetadata {
            name: "inspecttask".to_string(),
            version: "0.1.0".to_string(),
            filename: "inspecttask-0.1.0.tgz".to_string(),
            media_type: "application/gzip".to_string(),
            digest: "sha256:artifact".to_string(),
            size_bytes: 1,
            source_hash: "sha256:source".to_string(),
            generated_at: Utc::now(),
        };
        let artifact_cm = ConfigMap {
            metadata: ObjectMeta {
                name: Some("fe-inspecttask-a1b2c3d4".to_string()),
                namespace: Some("extension-frontend-forge".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        let job = make_publish_job(
            &fe,
            &config,
            "fe-inspecttask-publish-request",
            &request,
            &artifact,
            &artifact_cm,
        );
        let env = job.spec.unwrap().template.spec.unwrap().containers[0]
            .env
            .clone()
            .unwrap()
            .into_iter()
            .map(|env| (env.name, env.value.unwrap_or_default()))
            .collect::<BTreeMap<_, _>>();

        assert_eq!(env["FE_NAME"], "inspecttask");
        assert_eq!(env["PUBLISH_REQUEST_ID"], "request-1");
        assert_eq!(env["ARTIFACT_DIGEST"], "sha256:artifact");
        assert_eq!(
            env["ARTIFACT_CONFIGMAP_NAMESPACE"],
            "extension-frontend-forge"
        );
        assert_eq!(env["ARTIFACT_CONFIGMAP_NAME"], "fe-inspecttask-a1b2c3d4");
        assert_eq!(env["ARTIFACT_CONFIGMAP_KEY"], PACKAGE_KEY);
        assert_eq!(env["ARTIFACT_FILENAME"], "inspecttask-0.1.0.tgz");
        assert_eq!(env["PUBLISH_TARGET_KIND"], "Secret");
        assert_eq!(env["PUBLISH_TARGET_NAMESPACE"], "extension-frontend-forge");
        assert_eq!(env["PUBLISH_TARGET_NAME"], "ksbuilder-publish-config");
    }

    #[test]
    fn publish_status_maps_failed_job_message() {
        let request = PublishRequest {
            request_id: "request-1".to_string(),
            artifact_digest: "sha256:artifact".to_string(),
            target_ref: NamespacedResourceRef {
                namespace: "extension-frontend-forge".to_string(),
                name: "ksbuilder-publish-config".to_string(),
                uid: None,
            },
            target_kind: "ConfigMap".to_string(),
        };
        let job = Job {
            metadata: ObjectMeta {
                name: Some("fe-inspecttask-publish-request".to_string()),
                namespace: Some("extension-frontend-forge".to_string()),
                uid: Some("job-uid".to_string()),
                ..Default::default()
            },
            status: Some(JobStatus {
                failed: Some(1),
                conditions: Some(vec![k8s_openapi::api::batch::v1::JobCondition {
                    type_: "Failed".to_string(),
                    status: "True".to_string(),
                    message: Some("ksbuilder publish failed".to_string()),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        };

        let status = publish_status_from_job(&request, &job);

        assert_eq!(status.phase, PublishPhase::Failed);
        assert_eq!(
            status.last_error.as_deref(),
            Some("ksbuilder publish failed")
        );
        assert_eq!(
            status.job_ref.unwrap().name,
            "fe-inspecttask-publish-request"
        );
    }
}
