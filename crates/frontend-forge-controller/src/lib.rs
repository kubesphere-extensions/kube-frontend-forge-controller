pub mod fi;
pub mod webhook;

use std::{
    env,
    net::{AddrParseError, SocketAddr},
    str::ParseBoolError,
    sync::Arc,
};

use frontend_forge_common::CommonError;
use kube::Client;
use snafu::{ResultExt, Snafu};
use tracing::info;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("spec/hash error: {source}"))]
    Common { source: CommonError },
    #[snafu(display("failed to initialize Kubernetes client: {source}"))]
    KubeClientInit {
        #[snafu(source(from(kube::Error, Box::new)))]
        source: Box<kube::Error>,
    },
    #[snafu(display("failed to patch FrontendIntegration status {namespace}/{name}: {source}"))]
    PatchFrontendIntegrationStatus {
        namespace: String,
        name: String,
        #[snafu(source(from(kube::Error, Box::new)))]
        source: Box<kube::Error>,
    },
    #[snafu(display("failed to patch FrontendIntegration metadata {namespace}/{name}: {source}"))]
    PatchFrontendIntegrationMetadata {
        namespace: String,
        name: String,
        #[snafu(source(from(kube::Error, Box::new)))]
        source: Box<kube::Error>,
    },
    #[snafu(display("failed to get FrontendIntegration {namespace}/{name}: {source}"))]
    GetFrontendIntegration {
        namespace: String,
        name: String,
        #[snafu(source(from(kube::Error, Box::new)))]
        source: Box<kube::Error>,
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
    #[snafu(display(
        "failed to list Jobs in {namespace} for FrontendIntegration {fi_name} and specHash \
         {spec_hash}: {source}"
    ))]
    ListJobsForHash {
        namespace: String,
        fi_name: String,
        spec_hash: String,
        #[snafu(source(from(kube::Error, Box::new)))]
        source: Box<kube::Error>,
    },
    #[snafu(display("failed to get JSBundle {namespace}/{name}: {source}"))]
    GetJsBundle {
        namespace: String,
        name: String,
        #[snafu(source(from(kube::Error, Box::new)))]
        source: Box<kube::Error>,
    },
    #[snafu(display("failed to patch JSBundle {namespace}/{name}: {source}"))]
    PatchJsBundle {
        namespace: String,
        name: String,
        #[snafu(source(from(kube::Error, Box::new)))]
        source: Box<kube::Error>,
    },
    #[snafu(transparent)]
    Job {
        source: frontend_forge_common::JobError,
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
            work_namespace,
            runner_image: env::var("RUNNER_IMAGE")
                .unwrap_or_else(|_| "kubesphere/frontend-forge-runner:latest".to_string()),
            runner_service_account: env::var("RUNNER_SERVICE_ACCOUNT").ok(),
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
        client,
        config: ControllerConfig::from_env(),
    }))
}

pub async fn run() -> Result<(), Error> {
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
