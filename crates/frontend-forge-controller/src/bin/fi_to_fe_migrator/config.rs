use super::*;

#[derive(Clone, Debug)]
pub(crate) struct MigratorConfig {
    pub(crate) package_version: String,
    pub(crate) schema_version: String,
    pub(crate) ready_timeout: Duration,
    pub(crate) poll_interval: Duration,
    pub(crate) fe_api_base_url: String,
    pub(crate) fe_api_insecure_skip_tls_verify: bool,
    pub(crate) fe_api_ca_cert_path: Option<String>,
    pub(crate) fe_api_group: String,
    pub(crate) fe_api_version: String,
    pub(crate) publish_target_kind: PublishTargetKind,
    pub(crate) publish_target_namespace: String,
    pub(crate) publish_target_name: String,
}

impl MigratorConfig {
    pub(crate) fn from_env() -> Result<Self> {
        let publish_target_kind =
            parse_publish_target_kind(env_or("PUBLISH_TARGET_KIND", DEFAULT_PUBLISH_TARGET_KIND))?;
        Ok(Self {
            package_version: env_or("PACKAGE_VERSION", DEFAULT_PACKAGE_VERSION),
            schema_version: env_or("SCHEMA_VERSION", DEFAULT_SCHEMA_VERSION),
            ready_timeout: Duration::from_secs(parse_env_u64(
                "READY_TIMEOUT_SECONDS",
                DEFAULT_READY_TIMEOUT_SECONDS,
            )?),
            poll_interval: Duration::from_secs(parse_env_u64(
                "POLL_INTERVAL_SECONDS",
                DEFAULT_POLL_INTERVAL_SECONDS,
            )?),
            fe_api_base_url: trim_trailing_slash(&env_or(
                "FE_API_BASE_URL",
                DEFAULT_FE_API_BASE_URL,
            )),
            fe_api_insecure_skip_tls_verify: parse_env_bool(
                "FE_API_INSECURE_SKIP_TLS_VERIFY",
                false,
            )?,
            fe_api_ca_cert_path: optional_env("FE_API_CA_CERT_PATH"),
            fe_api_group: env_or("FE_API_GROUP", DEFAULT_FE_API_GROUP),
            fe_api_version: env_or("FE_API_VERSION", DEFAULT_FE_API_VERSION),
            publish_target_kind,
            publish_target_namespace: required_env("PUBLISH_TARGET_NAMESPACE")?,
            publish_target_name: env_or("PUBLISH_TARGET_NAME", DEFAULT_PUBLISH_TARGET_NAME),
        })
    }
}
pub(crate) fn parse_publish_target_kind(value: String) -> Result<PublishTargetKind> {
    match value.as_str() {
        "ConfigMap" => Ok(PublishTargetKind::ConfigMap),
        "Secret" => Ok(PublishTargetKind::Secret),
        _ => Err(Error::InvalidEnv {
            key: "PUBLISH_TARGET_KIND",
            value,
            message: "expected ConfigMap or Secret".to_string(),
        }),
    }
}

pub(crate) fn parse_env_u64(key: &'static str, default: u64) -> Result<u64> {
    optional_env(key).map_or(Ok(default), |value| {
        value.parse::<u64>().map_err(|err| Error::InvalidEnv {
            key,
            value,
            message: err.to_string(),
        })
    })
}

pub(crate) fn parse_env_bool(key: &'static str, default: bool) -> Result<bool> {
    optional_env(key).map_or(Ok(default), |value| match value.as_str() {
        "true" | "1" | "yes" => Ok(true),
        "false" | "0" | "no" => Ok(false),
        _ => Err(Error::InvalidEnv {
            key,
            value,
            message: "expected true/false".to_string(),
        }),
    })
}

pub(crate) fn required_env(key: &'static str) -> Result<String> {
    optional_env(key).ok_or_else(|| Error::InvalidEnv {
        key,
        value: String::new(),
        message: "required environment variable is empty".to_string(),
    })
}

pub(crate) fn env_or(key: &'static str, default: &str) -> String {
    optional_env(key).unwrap_or_else(|| default.to_string())
}

pub(crate) fn optional_env(key: &'static str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(crate) fn trim_trailing_slash(value: &str) -> String {
    value.trim_end_matches('/').to_string()
}
