pub mod fe;
pub mod fi;
pub mod webhook;

use frontend_forge_common::CommonError;
use frontend_forge_extension_package::ExtensionPackageError;
use k8s_openapi::api::batch::v1::{Job, JobStatus};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;
use kube::api::PostParams;
use kube::{Api, Client, Resource};
use snafu::{ResultExt, Snafu};
use std::env;
use std::net::{AddrParseError, SocketAddr};
use std::str::ParseBoolError;
use std::sync::Arc;
use tracing::info;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("spec/hash error: {source}"))]
    Common { source: CommonError },
    #[snafu(display("failed to hash FrontendExtension package source: {source}"))]
    FrontendExtensionSourceHash { source: ExtensionPackageError },
    #[snafu(display("failed to initialize Kubernetes client: {source}"))]
    KubeClientInit { source: kube::Error },
    #[snafu(display("failed to patch FrontendIntegration status {namespace}/{name}: {source}"))]
    PatchFrontendIntegrationStatus {
        namespace: String,
        name: String,
        source: kube::Error,
    },
    #[snafu(display("failed to patch FrontendExtension status {name}: {source}"))]
    PatchFrontendExtensionStatus { name: String, source: kube::Error },
    #[snafu(display("failed to patch FrontendIntegration metadata {namespace}/{name}: {source}"))]
    PatchFrontendIntegrationMetadata {
        namespace: String,
        name: String,
        source: kube::Error,
    },
    #[snafu(display("failed to get FrontendIntegration {namespace}/{name}: {source}"))]
    GetFrontendIntegration {
        namespace: String,
        name: String,
        source: kube::Error,
    },
    #[snafu(display(
        "failed to serialize FrontendIntegration status patch for {namespace}/{name}: {source}"
    ))]
    SerializeFrontendIntegrationStatusPatch {
        namespace: String,
        name: String,
        source: serde_json::Error,
    },
    #[snafu(display(
        "serialized FrontendIntegration status patch for {namespace}/{name} was not a JSON object"
    ))]
    InvalidFrontendIntegrationStatusPatchShape { namespace: String, name: String },
    #[snafu(display("failed to serialize FrontendExtension status patch for {name}: {source}"))]
    SerializeFrontendExtensionStatusPatch {
        name: String,
        source: serde_json::Error,
    },
    #[snafu(display("serialized FrontendExtension status patch for {name} was not a JSON object"))]
    InvalidFrontendExtensionStatusPatchShape { name: String },
    #[snafu(display(
        "failed to list Jobs in {namespace} for FrontendIntegration {fi_name} and specHash {spec_hash}: {source}"
    ))]
    ListJobsForHash {
        namespace: String,
        fi_name: String,
        spec_hash: String,
        source: kube::Error,
    },
    #[snafu(display(
        "failed to list package Jobs in {namespace} for FrontendExtension {fe_name} and sourceHash {source_hash}: {source}"
    ))]
    ListPackageJobsForHash {
        namespace: String,
        fe_name: String,
        source_hash: String,
        source: kube::Error,
    },
    #[snafu(display("failed to get artifact ConfigMap {namespace}/{name}: {source}"))]
    GetArtifactConfigMap {
        namespace: String,
        name: String,
        source: kube::Error,
    },
    #[snafu(display("failed to get JSBundle {namespace}/{name}: {source}"))]
    GetJsBundle {
        namespace: String,
        name: String,
        source: kube::Error,
    },
    #[snafu(display("failed to patch JSBundle {namespace}/{name}: {source}"))]
    PatchJsBundle {
        namespace: String,
        name: String,
        source: kube::Error,
    },
    #[snafu(display("failed to create Job {namespace}/{name}: {source}"))]
    CreateJob {
        namespace: String,
        name: String,
        source: kube::Error,
    },
    #[snafu(display("failed to get existing Job after conflict {namespace}/{name}: {source}"))]
    GetJobAfterConflict {
        namespace: String,
        name: String,
        source: kube::Error,
    },
    #[snafu(display("failed to get publish Job {namespace}/{name}: {source}"))]
    GetPublishJob {
        namespace: String,
        name: String,
        source: kube::Error,
    },
    #[snafu(display("invalid WEBHOOK_ENABLED value '{value}': {source}"))]
    InvalidWebhookEnabled {
        value: String,
        source: ParseBoolError,
    },
    #[snafu(display("invalid WEBHOOK_BIND_ADDR '{value}': {source}"))]
    InvalidWebhookBindAddr {
        value: String,
        source: AddrParseError,
    },
    #[snafu(display(
        "failed to load webhook TLS assets cert={cert_path} key={key_path}: {source}"
    ))]
    WebhookTlsConfig {
        cert_path: String,
        key_path: String,
        source: std::io::Error,
    },
    #[snafu(display("webhook server failed on {bind_addr}: {source}"))]
    WebhookServer {
        bind_addr: SocketAddr,
        source: std::io::Error,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct ControllerConfig {
    pub(crate) work_namespace: String,
    pub(crate) runner_image: String,
    pub(crate) runner_service_account: Option<String>,
    pub(crate) packager_image: String,
    pub(crate) packager_service_account: Option<String>,
    pub(crate) publisher_image: String,
    pub(crate) publisher_service_account: Option<String>,
    pub(crate) artifact_configmap_namespace: String,
    pub(crate) build_service_base_url: String,
    pub(crate) jsbundle_configmap_namespace: String,
    pub(crate) jsbundle_config_key: String,
    pub(crate) build_service_timeout_seconds: u64,
    pub(crate) stale_check_grace_seconds: u64,
    pub(crate) reconcile_requeue_seconds: u64,
    pub(crate) job_active_deadline_seconds: i64,
    pub(crate) job_ttl_seconds_after_finished: Option<i32>,
}

impl ControllerConfig {
    fn from_env() -> Self {
        let work_namespace =
            env::var("WORK_NAMESPACE").unwrap_or_else(|_| "extension-frontend-forge".to_string());
        Self {
            work_namespace: work_namespace.clone(),
            runner_image: env::var("RUNNER_IMAGE")
                .unwrap_or_else(|_| "spike2044/frontend-forge-runner:latest".to_string()),
            runner_service_account: env::var("RUNNER_SERVICE_ACCOUNT").ok(),
            packager_image: env::var("PACKAGER_IMAGE").unwrap_or_else(|_| {
                "spike2044/frontend-forge-extension-packager:latest".to_string()
            }),
            packager_service_account: env::var("PACKAGER_SERVICE_ACCOUNT").ok(),
            publisher_image: env::var("PUBLISHER_IMAGE").unwrap_or_else(|_| {
                "spike2044/frontend-forge-extension-publisher:latest".to_string()
            }),
            publisher_service_account: env::var("PUBLISHER_SERVICE_ACCOUNT").ok(),
            artifact_configmap_namespace: env::var("ARTIFACT_CONFIGMAP_NAMESPACE")
                .unwrap_or(work_namespace.clone()),
            build_service_base_url: env::var("BUILD_SERVICE_BASE_URL").unwrap_or_else(|_| {
                "http://frontend-forge.extension-frontend-forge.svc".to_string()
            }),
            jsbundle_configmap_namespace: env::var("JSBUNDLE_CONFIGMAP_NAMESPACE")
                .unwrap_or_else(|_| "extension-frontend-forge".to_string()),
            jsbundle_config_key: env::var("JSBUNDLE_CONFIG_KEY")
                .unwrap_or_else(|_| "index.js".to_string()),
            build_service_timeout_seconds: env::var("BUILD_SERVICE_TIMEOUT_SECONDS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(600),
            stale_check_grace_seconds: env::var("STALE_CHECK_GRACE_SECONDS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30),
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
        }
    }
}

#[derive(Clone)]
pub(crate) struct ContextData {
    pub(crate) client: Client,
    pub(crate) config: ControllerConfig,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ObservedJobPhase {
    Pending,
    Running,
    Succeeded,
    Failed,
}

pub(crate) const DEFAULT_JOB_TTL_SECONDS_AFTER_FINISHED: i32 = 60 * 60;

fn install_rustls_crypto_provider() {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        rustls::crypto::ring::default_provider()
            .install_default()
            .expect("ring crypto provider should install before controller startup");
    }
}

fn init_runtime(default_filter: &'static str) {
    install_rustls_crypto_provider();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| default_filter.into()),
        )
        .init();
}

async fn context_from_env() -> Result<Arc<ContextData>, Error> {
    let client = Client::try_default().await.context(KubeClientInitSnafu)?;
    Ok(Arc::new(ContextData {
        client: client.clone(),
        config: ControllerConfig::from_env(),
    }))
}

pub async fn run() -> Result<(), Error> {
    run_fi_controller().await
}

pub async fn run_fi_controller() -> Result<(), Error> {
    init_runtime("info,frontend_forge_controller=debug");

    let ctx = context_from_env().await?;
    let webhook_config = webhook::WebhookConfig::from_env()?;

    if webhook_config.enabled {
        info!(bind_addr = %webhook_config.bind_addr, "admission webhook enabled");
        tokio::try_join!(fi::run(ctx), webhook::run_webhook_server(webhook_config))?;
    } else {
        info!("admission webhook disabled");
        fi::run(ctx).await?;
    }

    info!("frontend integration controller shutdown complete");

    Ok(())
}

pub async fn run_fe_controller() -> Result<(), Error> {
    init_runtime("info,frontend_extension_controller=debug,frontend_forge_controller=debug");

    let ctx = context_from_env().await?;
    fe::run(ctx).await?;

    info!("frontend extension controller shutdown complete");

    Ok(())
}

pub(crate) fn observed_job_phase(status: Option<&JobStatus>) -> ObservedJobPhase {
    let Some(status) = status else {
        return ObservedJobPhase::Pending;
    };

    if status.failed.unwrap_or(0) > 0 {
        return ObservedJobPhase::Failed;
    }
    if status.succeeded.unwrap_or(0) > 0 {
        return ObservedJobPhase::Succeeded;
    }
    if status.active.unwrap_or(0) > 0 {
        return ObservedJobPhase::Running;
    }

    if let Some(conditions) = &status.conditions {
        for cond in conditions {
            if cond.status != "True" {
                continue;
            }
            if cond.type_ == "Failed" {
                return ObservedJobPhase::Failed;
            }
            if cond.type_ == "Complete" {
                return ObservedJobPhase::Succeeded;
            }
        }
    }

    ObservedJobPhase::Pending
}

pub(crate) fn extract_job_message(job: &Job) -> Option<String> {
    let status = job.status.as_ref()?;
    if let Some(conditions) = &status.conditions {
        if let Some(cond) = conditions
            .iter()
            .find(|c| c.status == "True" && c.type_ == "Failed")
        {
            return cond.message.clone().or_else(|| cond.reason.clone());
        }
    }
    None
}

pub(crate) fn base_owner_ref<T>(obj: &T) -> Option<OwnerReference>
where
    T: Resource<DynamicType = ()>,
{
    obj.controller_owner_ref(&())
}

pub(crate) async fn create_or_get_job(
    job_api: &Api<Job>,
    namespace: &str,
    job: Job,
    name: &str,
) -> Result<Job, Error> {
    match job_api.create(&PostParams::default(), &job).await {
        Ok(created) => Ok(created),
        Err(kube::Error::Api(ae)) if ae.code == 409 => {
            Ok(job_api
                .get(name)
                .await
                .with_context(|_| GetJobAfterConflictSnafu {
                    namespace: namespace.to_string(),
                    name: name.to_string(),
                })?)
        }
        Err(err) => Err(Error::CreateJob {
            namespace: namespace.to_string(),
            name: name.to_string(),
            source: err,
        }),
    }
}
