use super::*;

#[derive(Clone)]
pub(crate) struct ControllerConfig {
    pub(crate) work_namespace: String,
    pub(crate) packager_image: String,
    pub(crate) packager_service_account: Option<String>,
    pub(crate) publisher_image: String,
    pub(crate) publisher_service_account: Option<String>,
    pub(crate) artifact_configmap_namespace: String,
    pub(crate) build_service_base_url: String,
    pub(crate) build_service_timeout_seconds: u64,
    pub(crate) jsbundle_config_key: String,
    pub(crate) reconcile_requeue_seconds: u64,
    pub(crate) job_active_deadline_seconds: i64,
    pub(crate) job_ttl_seconds_after_finished: Option<i32>,
    pub(crate) artifact_retain_old_count: usize,
    pub(crate) package_max_attempts: u32,
}

impl ControllerConfig {
    pub(crate) fn from_env() -> Self {
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
pub(crate) struct ContextData {
    pub(crate) client: Client,
    pub(crate) config: ControllerConfig,
}

pub(crate) const DEFAULT_JOB_TTL_SECONDS_AFTER_FINISHED: i32 = 60 * 60;
pub(crate) const DEFAULT_ARTIFACT_RETAIN_OLD_COUNT: usize = 1;
pub(crate) const DEFAULT_PACKAGE_MAX_ATTEMPTS: u32 = 3;
