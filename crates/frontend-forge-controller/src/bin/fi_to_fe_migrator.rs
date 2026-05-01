use std::{collections::BTreeMap, env, fs, time::Duration};

use frontend_forge_api::{
    FrontendExtension, FrontendExtensionFrontendSpec, FrontendExtensionPackageSpec,
    FrontendExtensionPhase, FrontendExtensionSourceSpec, FrontendExtensionSourceType,
    FrontendExtensionSpec, FrontendIntegration, InlineFrontendExtensionSourceSpec,
    NamespacedResourceRef, PublishPolicyMode, PublishPolicySpec, PublishTargetKind,
};
use frontend_forge_common::sha256_hex;
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::{
    Api, Client, Resource, ResourceExt,
    api::{DeleteParams, DynamicObject, Patch, PatchParams, PostParams},
    core::{ApiResource, GroupVersionKind},
};
use reqwest::StatusCode;
use serde_json::{Value, json};
use snafu::Snafu;
use tokio::time::{Instant, sleep};
use tracing::{error, info, warn};

const LABEL_MANAGED_BY: &str = "frontend-forge.io/managed-by";
const MANAGED_BY_VALUE: &str = "frontend-forge-fi-migrator";
const ANNO_SOURCE_FI_NAME: &str = "frontend-forge.io/source-fi-name";
const ANNO_SOURCE_FI_UID: &str = "frontend-forge.io/source-fi-uid";
const DEFAULT_PACKAGE_VERSION: &str = "0.1.0";
const DEFAULT_SCHEMA_VERSION: &str = "v1";
const DEFAULT_READY_TIMEOUT_SECONDS: u64 = 600;
const DEFAULT_POLL_INTERVAL_SECONDS: u64 = 5;
const DEFAULT_KS_APISERVER_BASE_URL: &str = "https://ks-apiserver.kubesphere-system.svc";
const DEFAULT_FE_API_GROUP: &str = "frontend-forge-api.kubesphere.io";
const DEFAULT_FE_API_VERSION: &str = "v1alpha1";
const DEFAULT_API_SERVICE_NAME: &str = "v1alpha1.frontend-forge-api.kubesphere.io";
const DEFAULT_TOKEN_PATH: &str = "/var/run/secrets/kubernetes.io/serviceaccount/token";
const DEFAULT_CA_CERT_PATH: &str = "/var/run/secrets/kubernetes.io/serviceaccount/ca.crt";
const DEFAULT_PUBLISH_TARGET_KIND: &str = "ConfigMap";
const DEFAULT_PUBLISH_TARGET_NAME: &str = "ksbuilder-publish-config";

#[derive(Debug, Snafu)]
enum Error {
    #[snafu(display("failed to initialize Kubernetes client: {source}"))]
    KubeClientInit { source: kube::Error },
    #[snafu(display("Kubernetes operation failed while {action}: {source}"))]
    Kube { action: String, source: kube::Error },
    #[snafu(display("HTTP operation failed while {action}: {source}"))]
    Http {
        action: String,
        source: reqwest::Error,
    },
    #[snafu(display("failed to read file {path}: {source}"))]
    ReadFile {
        path: String,
        source: std::io::Error,
    },
    #[snafu(display("invalid {key} value {value:?}: {message}"))]
    InvalidEnv {
        key: &'static str,
        value: String,
        message: String,
    },
    #[snafu(display("{message}"))]
    Message { message: String },
}

type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug)]
struct MigratorConfig {
    package_version: String,
    schema_version: String,
    ready_timeout: Duration,
    poll_interval: Duration,
    ks_apiserver_base_url: String,
    ks_apiserver_insecure_skip_tls_verify: bool,
    ks_apiserver_ca_cert_path: Option<String>,
    service_account_token_path: Option<String>,
    fe_api_group: String,
    fe_api_version: String,
    fe_api_service_name: String,
    publish_target_kind: PublishTargetKind,
    publish_target_namespace: String,
    publish_target_name: String,
}

impl MigratorConfig {
    fn from_env() -> Result<Self> {
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
            ks_apiserver_base_url: trim_trailing_slash(&env_or(
                "KS_APISERVER_BASE_URL",
                DEFAULT_KS_APISERVER_BASE_URL,
            )),
            ks_apiserver_insecure_skip_tls_verify: parse_env_bool(
                "KS_APISERVER_INSECURE_SKIP_TLS_VERIFY",
                false,
            )?,
            ks_apiserver_ca_cert_path: optional_env("KS_APISERVER_CA_CERT_PATH")
                .or_else(|| Some(DEFAULT_CA_CERT_PATH.to_string()))
                .filter(|path| !path.is_empty()),
            service_account_token_path: optional_env("SERVICE_ACCOUNT_TOKEN_PATH")
                .or_else(|| Some(DEFAULT_TOKEN_PATH.to_string()))
                .filter(|path| !path.is_empty()),
            fe_api_group: env_or("FE_API_GROUP", DEFAULT_FE_API_GROUP),
            fe_api_version: env_or("FE_API_VERSION", DEFAULT_FE_API_VERSION),
            fe_api_service_name: env_or("FE_API_SERVICE_NAME", DEFAULT_API_SERVICE_NAME),
            publish_target_kind,
            publish_target_namespace: required_env("PUBLISH_TARGET_NAMESPACE")?,
            publish_target_name: env_or("PUBLISH_TARGET_NAME", DEFAULT_PUBLISH_TARGET_NAME),
        })
    }
}

fn install_rustls_crypto_provider() {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        rustls::crypto::ring::default_provider()
            .install_default()
            .expect("ring crypto provider should install before FI migrator startup");
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    install_rustls_crypto_provider();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,fi_to_fe_migrator=debug".into()),
        )
        .init();

    let cfg = MigratorConfig::from_env()?;
    let client = Client::try_default()
        .await
        .map_err(|source| Error::KubeClientInit { source })?;
    let http = publish_http_client(&cfg)?;

    run(client, http, cfg).await
}

async fn run(client: Client, http: reqwest::Client, cfg: MigratorConfig) -> Result<()> {
    wait_for_prerequisites(&client, &cfg).await?;

    let fi_api = Api::<FrontendIntegration>::all(client.clone());
    let items = fi_api
        .list(&Default::default())
        .await
        .map_err(|source| Error::Kube {
            action: "listing FrontendIntegrations".to_string(),
            source,
        })?
        .items;

    info!(count = items.len(), "starting FI to FE migration");
    let mut failures = Vec::new();

    for fi in items {
        let fi_name = fi.name_any();
        if let Err(err) = migrate_one(&client, &http, &cfg, fi).await {
            error!(fi = %fi_name, error = %err, "FI migration failed");
            failures.push(format!("{fi_name}: {err}"));
        }
    }

    if failures.is_empty() {
        info!("FI to FE migration completed");
        Ok(())
    } else {
        Err(Error::Message {
            message: format!(
                "FI to FE migration completed with {} failure(s): {}",
                failures.len(),
                failures.join("; ")
            ),
        })
    }
}

async fn migrate_one(
    client: &Client,
    http: &reqwest::Client,
    cfg: &MigratorConfig,
    fi: FrontendIntegration,
) -> Result<()> {
    let fi_name = fi.name_any();
    let fe_name = migrated_fe_name(&fi_name);
    let was_enabled = fi.spec.enabled();
    info!(fi = %fi_name, fe = %fe_name, was_enabled, "migrating FI");

    let fe_api = Api::<FrontendExtension>::all(client.clone());
    upsert_managed_fe(&fe_api, cfg, &fi, &fe_name).await?;
    let artifact_digest = wait_for_fe_ready(&fe_api, &fe_name, cfg).await?;
    delete_fi_and_wait(client, &fi_name, cfg).await?;

    if was_enabled {
        publish_fe(http, cfg, &fe_name, &artifact_digest).await?;
    } else {
        info!(fi = %fi_name, fe = %fe_name, "source FI was disabled; skipping publish");
    }

    Ok(())
}

async fn wait_for_prerequisites(client: &Client, cfg: &MigratorConfig) -> Result<()> {
    let deadline = Instant::now() + cfg.ready_timeout;
    loop {
        let fi = frontend_integration_crd_ready(client).await;
        let fe = frontend_extension_crd_ready(client).await;
        let api_service = frontend_extension_api_service_ready(client, cfg).await;

        match (&fi, &fe, &api_service) {
            (Ok(()), Ok(()), Ok(())) => {
                info!("migration prerequisites are ready");
                return Ok(());
            }
            _ if Instant::now() >= deadline => {
                return Err(Error::Message {
                    message: format!(
                        "timed out waiting for migration prerequisites: fi={:?}; fe={:?}; \
                         apiService={:?}",
                        fi.err(),
                        fe.err(),
                        api_service.err()
                    ),
                });
            }
            _ => {
                warn!(
                    fi_ready = fi.is_ok(),
                    fe_ready = fe.is_ok(),
                    api_service_ready = api_service.is_ok(),
                    "waiting for migration prerequisites"
                );
                sleep(cfg.poll_interval).await;
            }
        }
    }
}

async fn frontend_integration_crd_ready(client: &Client) -> Result<()> {
    crd_ready(
        client,
        "frontendintegrations.frontend-forge.kubesphere.io",
        false,
    )
    .await
}

async fn frontend_extension_crd_ready(client: &Client) -> Result<()> {
    crd_ready(
        client,
        "frontendextensions.frontend-forge.kubesphere.io",
        true,
    )
    .await
}

async fn crd_ready(client: &Client, name: &str, require_status_subresource: bool) -> Result<()> {
    let api = Api::<CustomResourceDefinition>::all(client.clone());
    let crd = api.get(name).await.map_err(|source| Error::Kube {
        action: format!("getting CRD {name}"),
        source,
    })?;
    let value = serde_json::to_value(&crd).map_err(|source| Error::Message {
        message: format!("failed to serialize CRD {name}: {source}"),
    })?;

    if !crd_established(&value) {
        return Err(Error::Message {
            message: format!("CRD {name} is not Established"),
        });
    }
    if require_status_subresource && !crd_has_v1alpha1_status_subresource(&value) {
        return Err(Error::Message {
            message: format!("CRD {name} does not serve v1alpha1 status subresource"),
        });
    }
    Ok(())
}

async fn frontend_extension_api_service_ready(client: &Client, cfg: &MigratorConfig) -> Result<()> {
    let gvk = GroupVersionKind::gvk("extensions.kubesphere.io", "v1alpha1", "APIService");
    let resource = ApiResource::from_gvk_with_plural(&gvk, "apiservices");
    let api = Api::<DynamicObject>::all_with(client.clone(), &resource);
    let api_service = api
        .get(&cfg.fe_api_service_name)
        .await
        .map_err(|source| Error::Kube {
            action: format!("getting APIService {}", cfg.fe_api_service_name),
            source,
        })?;
    let group = api_service
        .data
        .pointer("/spec/group")
        .and_then(Value::as_str);
    let version = api_service
        .data
        .pointer("/spec/version")
        .and_then(Value::as_str);
    if group != Some(cfg.fe_api_group.as_str()) || version != Some(cfg.fe_api_version.as_str()) {
        return Err(Error::Message {
            message: format!(
                "APIService {} does not target {}/{}",
                cfg.fe_api_service_name, cfg.fe_api_group, cfg.fe_api_version
            ),
        });
    }
    let state = api_service
        .data
        .pointer("/status/state")
        .and_then(Value::as_str);
    if state.is_some_and(|state| state != "Available") {
        return Err(Error::Message {
            message: format!(
                "APIService {} is not Available: {}",
                cfg.fe_api_service_name,
                state.unwrap_or_default()
            ),
        });
    }
    Ok(())
}

async fn upsert_managed_fe(
    fe_api: &Api<FrontendExtension>,
    cfg: &MigratorConfig,
    fi: &FrontendIntegration,
    fe_name: &str,
) -> Result<()> {
    let desired = frontend_extension_from_fi(fi, fe_name, cfg);
    match fe_api
        .get_opt(fe_name)
        .await
        .map_err(|source| Error::Kube {
            action: format!("getting FrontendExtension {fe_name}"),
            source,
        })? {
        None => {
            fe_api
                .create(&PostParams::default(), &desired)
                .await
                .map_err(|source| Error::Kube {
                    action: format!("creating FrontendExtension {fe_name}"),
                    source,
                })?;
            info!(fe = %fe_name, "created migrated FrontendExtension");
        }
        Some(existing) => {
            ensure_existing_fe_is_managed_by_fi(&existing, fi)?;
            let patch = json!({
                "metadata": {
                    "labels": desired.metadata.labels,
                    "annotations": desired.metadata.annotations,
                },
                "spec": desired.spec,
            });
            fe_api
                .patch(fe_name, &PatchParams::default(), &Patch::Merge(&patch))
                .await
                .map_err(|source| Error::Kube {
                    action: format!("patching FrontendExtension {fe_name}"),
                    source,
                })?;
            info!(fe = %fe_name, "patched migrated FrontendExtension");
        }
    }
    Ok(())
}

fn ensure_existing_fe_is_managed_by_fi(
    fe: &FrontendExtension,
    fi: &FrontendIntegration,
) -> Result<()> {
    let fe_name = fe.name_any();
    let fi_name = fi.name_any();
    let managed_by = fe
        .metadata
        .labels
        .as_ref()
        .and_then(|labels| labels.get(LABEL_MANAGED_BY))
        .map(String::as_str);
    if managed_by != Some(MANAGED_BY_VALUE) {
        return Err(Error::Message {
            message: format!(
                "FrontendExtension {fe_name} already exists and is not migrator-owned"
            ),
        });
    }

    let source_fi_name = fe
        .metadata
        .annotations
        .as_ref()
        .and_then(|annos| annos.get(ANNO_SOURCE_FI_NAME))
        .map(String::as_str);
    if source_fi_name != Some(fi_name.as_str()) {
        return Err(Error::Message {
            message: format!(
                "FrontendExtension {fe_name} is migrator-owned but points to source FI {:?}",
                source_fi_name
            ),
        });
    }
    Ok(())
}

async fn wait_for_fe_ready(
    fe_api: &Api<FrontendExtension>,
    fe_name: &str,
    cfg: &MigratorConfig,
) -> Result<String> {
    let deadline = Instant::now() + cfg.ready_timeout;
    loop {
        let fe = fe_api.get(fe_name).await.map_err(|source| Error::Kube {
            action: format!("getting FrontendExtension {fe_name} while waiting for Ready"),
            source,
        })?;
        let phase = fe.status.as_ref().map(|status| status.phase.clone());
        let digest = fe
            .status
            .as_ref()
            .and_then(|status| status.artifact.as_ref())
            .map(|artifact| artifact.digest.clone())
            .filter(|digest| !digest.is_empty());
        if phase == Some(FrontendExtensionPhase::Ready)
            && let Some(digest) = digest
        {
            info!(fe = %fe_name, artifact_digest = %digest, "FrontendExtension is Ready");
            return Ok(digest);
        }
        if phase == Some(FrontendExtensionPhase::Failed) {
            return Err(Error::Message {
                message: format!("FrontendExtension {fe_name} reached Failed phase"),
            });
        }
        if Instant::now() >= deadline {
            return Err(Error::Message {
                message: format!(
                    "timed out waiting for FrontendExtension {fe_name} to become Ready"
                ),
            });
        }
        info!(
            fe = %fe_name,
            phase = ?phase,
            "waiting for FrontendExtension package Ready"
        );
        sleep(cfg.poll_interval).await;
    }
}

async fn delete_fi_and_wait(client: &Client, fi_name: &str, cfg: &MigratorConfig) -> Result<()> {
    let fi_api = Api::<FrontendIntegration>::all(client.clone());
    match fi_api.delete(fi_name, &DeleteParams::default()).await {
        Ok(_) => {}
        Err(kube::Error::Api(ae)) if ae.code == 404 => {}
        Err(source) => {
            return Err(Error::Kube {
                action: format!("deleting FrontendIntegration {fi_name}"),
                source,
            });
        }
    }

    let deadline = Instant::now() + cfg.ready_timeout;
    loop {
        match fi_api.get_opt(fi_name).await {
            Ok(None) => {
                info!(fi = %fi_name, "FrontendIntegration deleted");
                return Ok(());
            }
            Ok(Some(_)) if Instant::now() < deadline => {
                sleep(cfg.poll_interval).await;
            }
            Ok(Some(_)) => {
                return Err(Error::Message {
                    message: format!(
                        "timed out waiting for FrontendIntegration {fi_name} deletion"
                    ),
                });
            }
            Err(source) => {
                return Err(Error::Kube {
                    action: format!("checking FrontendIntegration {fi_name} deletion"),
                    source,
                });
            }
        }
    }
}

async fn publish_fe(
    http: &reqwest::Client,
    cfg: &MigratorConfig,
    fe_name: &str,
    artifact_digest: &str,
) -> Result<()> {
    let url = format!(
        "{}/kapis/{}/{}/frontendextensions/{}/publish",
        cfg.ks_apiserver_base_url, cfg.fe_api_group, cfg.fe_api_version, fe_name
    );
    let request_id = publish_request_id(fe_name, artifact_digest);
    let response = http
        .post(&url)
        .bearer_auth(read_service_account_token(cfg)?)
        .json(&json!({
            "requestId": request_id,
            "expectedArtifactDigest": artifact_digest,
        }))
        .send()
        .await
        .map_err(|source| Error::Http {
            action: format!("posting FE publish request to {url}"),
            source,
        })?;
    let status = response.status();
    if matches!(
        status,
        StatusCode::OK | StatusCode::ACCEPTED | StatusCode::CREATED
    ) {
        info!(fe = %fe_name, %request_id, "publish request accepted");
        return Ok(());
    }
    let body = response.text().await.unwrap_or_default();
    Err(Error::Message {
        message: format!("FE publish request for {fe_name} failed with {status}: {body}"),
    })
}

fn publish_http_client(cfg: &MigratorConfig) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .danger_accept_invalid_certs(cfg.ks_apiserver_insecure_skip_tls_verify);
    if !cfg.ks_apiserver_insecure_skip_tls_verify
        && let Some(path) = cfg.ks_apiserver_ca_cert_path.as_ref()
    {
        match fs::read(path) {
            Ok(bytes) => {
                let cert =
                    reqwest::Certificate::from_pem(&bytes).map_err(|source| Error::Http {
                        action: format!("loading CA certificate {path}"),
                        source,
                    })?;
                builder = builder.add_root_certificate(cert);
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(Error::ReadFile {
                    path: path.clone(),
                    source,
                });
            }
        }
    }
    builder.build().map_err(|source| Error::Http {
        action: "building HTTP client".to_string(),
        source,
    })
}

fn read_service_account_token(cfg: &MigratorConfig) -> Result<String> {
    let Some(path) = cfg.service_account_token_path.as_ref() else {
        return Ok(String::new());
    };
    fs::read_to_string(path)
        .map(|token| token.trim().to_string())
        .map_err(|source| Error::ReadFile {
            path: path.clone(),
            source,
        })
}

fn frontend_extension_from_fi(
    fi: &FrontendIntegration,
    fe_name: &str,
    cfg: &MigratorConfig,
) -> FrontendExtension {
    let display_name = fi_display_name(fi);
    let description = fi_description(fi, &display_name);
    let schema_version = fi
        .spec
        .engine_version()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| cfg.schema_version.clone());
    let mut fe = FrontendExtension::new(
        fe_name,
        FrontendExtensionSpec {
            package: FrontendExtensionPackageSpec {
                name: Some(fe_name.to_string()),
                version: cfg.package_version.clone(),
                display_name: localized_map(display_name),
                description: localized_map(description),
                category: None,
                keywords: Vec::new(),
                sources: Vec::new(),
                kube_version: None,
                ks_version: None,
                maintainers: Vec::new(),
                home: None,
                provider: BTreeMap::new(),
                icon: None,
                static_file_directory: None,
                dependencies: None,
                installation_mode: None,
                images: Vec::new(),
                charts: None,
            },
            source: FrontendExtensionSourceSpec {
                type_: FrontendExtensionSourceType::Inline,
                inline: InlineFrontendExtensionSourceSpec {
                    schema_version,
                    frontend: FrontendExtensionFrontendSpec {
                        display_name: fi.spec.display_name.clone(),
                        locales: fi.spec.locales.clone(),
                        menus: fi.spec.menus.clone(),
                        pages: fi.spec.pages.clone(),
                    },
                    extension_resources: None,
                },
            },
            publish_policy: Some(PublishPolicySpec {
                mode: PublishPolicyMode::Manual,
                default_target_kind: Some(cfg.publish_target_kind.clone()),
                default_target_ref: Some(NamespacedResourceRef {
                    namespace: cfg.publish_target_namespace.clone(),
                    name: cfg.publish_target_name.clone(),
                    uid: None,
                }),
            }),
        },
    );
    fe.metadata.labels = Some(migrator_labels());
    fe.metadata.annotations = Some(migrator_annotations(fi));
    fe
}

fn migrator_labels() -> BTreeMap<String, String> {
    BTreeMap::from([(LABEL_MANAGED_BY.to_string(), MANAGED_BY_VALUE.to_string())])
}

fn migrator_annotations(fi: &FrontendIntegration) -> BTreeMap<String, String> {
    let mut annotations = BTreeMap::from([(ANNO_SOURCE_FI_NAME.to_string(), fi.name_any())]);
    if let Some(uid) = fi.meta().uid.as_ref() {
        annotations.insert(ANNO_SOURCE_FI_UID.to_string(), uid.clone());
    }
    annotations
}

fn migrated_fe_name(fi_name: &str) -> String {
    let raw = format!("fi-{fi_name}");
    if raw.len() <= 63 {
        return raw;
    }
    let hash = sha256_hex(fi_name.as_bytes())
        .chars()
        .take(12)
        .collect::<String>();
    let prefix_len = 63 - "fi-".len() - "-".len() - hash.len();
    let slice = dns_label_prefix(fi_name, prefix_len);
    format!("fi-{slice}-{hash}")
}

fn dns_label_prefix(value: &str, max_len: usize) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if out.len() >= max_len {
            break;
        }
        let normalized = if ch.is_ascii_alphanumeric() || ch == '-' {
            ch.to_ascii_lowercase()
        } else {
            '-'
        };
        out.push(normalized);
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "x".to_string()
    } else {
        trimmed
    }
}

fn fi_display_name(fi: &FrontendIntegration) -> String {
    fi.spec
        .display_name
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| fi.name_any())
}

fn fi_description(fi: &FrontendIntegration, display_name: &str) -> String {
    fi.metadata
        .annotations
        .as_ref()
        .and_then(|annos| annos.get("kubesphere.io/description"))
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| display_name.to_string())
}

fn localized_map(value: String) -> BTreeMap<String, String> {
    BTreeMap::from([("en".to_string(), value)])
}

fn crd_established(value: &Value) -> bool {
    value
        .pointer("/status/conditions")
        .and_then(Value::as_array)
        .is_some_and(|conditions| {
            conditions.iter().any(|condition| {
                condition.get("type").and_then(Value::as_str) == Some("Established")
                    && condition.get("status").and_then(Value::as_str) == Some("True")
            })
        })
}

fn crd_has_v1alpha1_status_subresource(value: &Value) -> bool {
    value
        .pointer("/spec/versions")
        .and_then(Value::as_array)
        .is_some_and(|versions| {
            versions.iter().any(|version| {
                version.get("name").and_then(Value::as_str) == Some("v1alpha1")
                    && version.get("served").and_then(Value::as_bool) == Some(true)
                    && version.pointer("/subresources/status").is_some()
            })
        })
}

fn publish_request_id(fe_name: &str, artifact_digest: &str) -> String {
    let digest = artifact_digest
        .strip_prefix("sha256:")
        .unwrap_or(artifact_digest)
        .chars()
        .take(12)
        .collect::<String>();
    format!("fi-migration-{fe_name}-{digest}")
}

fn parse_publish_target_kind(value: String) -> Result<PublishTargetKind> {
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

fn parse_env_u64(key: &'static str, default: u64) -> Result<u64> {
    optional_env(key).map_or(Ok(default), |value| {
        value.parse::<u64>().map_err(|err| Error::InvalidEnv {
            key,
            value,
            message: err.to_string(),
        })
    })
}

fn parse_env_bool(key: &'static str, default: bool) -> Result<bool> {
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

fn required_env(key: &'static str) -> Result<String> {
    optional_env(key).ok_or_else(|| Error::InvalidEnv {
        key,
        value: String::new(),
        message: "required environment variable is empty".to_string(),
    })
}

fn env_or(key: &'static str, default: &str) -> String {
    optional_env(key).unwrap_or_else(|| default.to_string())
}

fn optional_env(key: &'static str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn trim_trailing_slash(value: &str) -> String {
    value.trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use frontend_forge_api::{
        BuilderSpec, FrontendIntegrationSpec, IframePageSpec, MenuNodeType, MenuPlacement,
        PageSpec, PageType, PrimaryMenuSpec,
    };
    use kube::core::ObjectMeta;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    fn cfg() -> MigratorConfig {
        MigratorConfig {
            package_version: "0.1.0".to_string(),
            schema_version: "v1".to_string(),
            ready_timeout: Duration::from_secs(1),
            poll_interval: Duration::from_millis(1),
            ks_apiserver_base_url: "https://ks-apiserver.kubesphere-system.svc".to_string(),
            ks_apiserver_insecure_skip_tls_verify: false,
            ks_apiserver_ca_cert_path: None,
            service_account_token_path: None,
            fe_api_group: DEFAULT_FE_API_GROUP.to_string(),
            fe_api_version: DEFAULT_FE_API_VERSION.to_string(),
            fe_api_service_name: DEFAULT_API_SERVICE_NAME.to_string(),
            publish_target_kind: PublishTargetKind::ConfigMap,
            publish_target_namespace: "extension-frontend-forge".to_string(),
            publish_target_name: "ksbuilder-publish-config".to_string(),
        }
    }

    fn fi(name: &str) -> FrontendIntegration {
        FrontendIntegration {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                annotations: Some(BTreeMap::from([(
                    "kubesphere.io/description".to_string(),
                    "description from annotation".to_string(),
                )])),
                uid: Some("fi-uid".to_string()),
                ..Default::default()
            },
            spec: FrontendIntegrationSpec {
                display_name: Some("Display Name".to_string()),
                locales: BTreeMap::from([(
                    "en".to_string(),
                    BTreeMap::from([("title".to_string(), "Title".to_string())]),
                )]),
                enabled: Some(true),
                menus: vec![PrimaryMenuSpec {
                    display_name: "Menu".to_string(),
                    key: "menu".to_string(),
                    icon: None,
                    placement: MenuPlacement::Global,
                    type_: MenuNodeType::Page,
                    children: vec![],
                }],
                pages: vec![PageSpec {
                    key: "menu".to_string(),
                    type_: PageType::Iframe,
                    crd_table: None,
                    iframe: Some(IframePageSpec {
                        src: "https://example.test".to_string(),
                    }),
                }],
                builder: Some(BuilderSpec {
                    engine_version: Some("v1alpha1".to_string()),
                }),
            },
            status: None,
        }
    }

    #[test]
    fn migrated_fe_name_always_prefixes_fi() {
        assert_eq!(migrated_fe_name("foo"), "fi-foo");
        assert_eq!(migrated_fe_name("fi-foo"), "fi-fi-foo");
    }

    #[test]
    fn migrated_fe_name_uses_slice_hash_when_too_long() {
        let name = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz";
        let migrated = migrated_fe_name(name);
        assert_eq!(migrated.len(), 63);
        assert!(migrated.starts_with("fi-"));
        assert_ne!(migrated, format!("fi-{name}"));
        assert!(migrated.ends_with(&sha256_hex(name.as_bytes())[..12]));
    }

    #[test]
    fn frontend_extension_from_fi_copies_source_fields_and_defaults_package() {
        let fi = fi("demo");
        let fe = frontend_extension_from_fi(&fi, "fi-demo", &cfg());

        assert_eq!(
            fe.metadata.labels.unwrap()[LABEL_MANAGED_BY],
            MANAGED_BY_VALUE
        );
        let annotations = fe.metadata.annotations.unwrap();
        assert_eq!(annotations[ANNO_SOURCE_FI_NAME], "demo");
        assert_eq!(annotations[ANNO_SOURCE_FI_UID], "fi-uid");
        assert_eq!(fe.spec.package.name.as_deref(), Some("fi-demo"));
        assert_eq!(fe.spec.package.version, "0.1.0");
        assert_eq!(fe.spec.package.display_name["en"], "Display Name");
        assert_eq!(
            fe.spec.package.description["en"],
            "description from annotation"
        );
        assert_eq!(fe.spec.source.inline.schema_version, "v1alpha1");
        assert_eq!(
            fe.spec.source.inline.frontend.display_name.as_deref(),
            Some("Display Name")
        );
        assert_eq!(
            fe.spec.source.inline.frontend.locales["en"]["title"],
            "Title"
        );
        assert_eq!(fe.spec.source.inline.frontend.menus[0].key, "menu");
        assert_eq!(fe.spec.source.inline.frontend.pages[0].key, "menu");
    }

    #[test]
    fn frontend_extension_from_fi_falls_back_for_package_metadata() {
        let mut fi = fi("demo");
        fi.metadata.annotations = None;
        fi.spec.display_name = None;
        fi.spec.builder = None;
        let fe = frontend_extension_from_fi(&fi, "fi-demo", &cfg());

        assert_eq!(fe.spec.package.display_name["en"], "demo");
        assert_eq!(fe.spec.package.description["en"], "demo");
        assert_eq!(fe.spec.source.inline.schema_version, "v1");
    }

    #[test]
    fn unmanaged_existing_fe_is_rejected() {
        let fi = fi("demo");
        let fe = FrontendExtension::new(
            "fi-demo",
            frontend_extension_from_fi(&fi, "fi-demo", &cfg()).spec,
        );
        let err = ensure_existing_fe_is_managed_by_fi(&fe, &fi).unwrap_err();
        assert!(err.to_string().contains("not migrator-owned"));
    }

    #[test]
    fn publish_request_id_is_stable_for_fe_and_digest() {
        assert_eq!(
            publish_request_id("fi-demo", "sha256:abcdef1234567890"),
            "fi-migration-fi-demo-abcdef123456"
        );
    }

    #[tokio::test]
    async fn publish_fe_posts_to_ks_apiserver_kapis() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let token_path =
            std::env::temp_dir().join(format!("fi-to-fe-migrator-token-{}", std::process::id()));
        std::fs::write(&token_path, "token-1").unwrap();

        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buf = [0_u8; 1024];
            loop {
                let n = stream.read(&mut buf).await.unwrap();
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..n]);
                if String::from_utf8_lossy(&request).contains("expectedArtifactDigest") {
                    break;
                }
            }
            stream
                .write_all(b"HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n")
                .await
                .unwrap();
            String::from_utf8(request).unwrap()
        });

        let mut cfg = cfg();
        cfg.ks_apiserver_base_url = format!("http://{addr}");
        cfg.ks_apiserver_insecure_skip_tls_verify = true;
        cfg.ks_apiserver_ca_cert_path = None;
        cfg.service_account_token_path = Some(token_path.to_string_lossy().to_string());
        let http = publish_http_client(&cfg).unwrap();

        publish_fe(&http, &cfg, "fi-demo", "sha256:abcdef1234567890")
            .await
            .unwrap();
        let request = server.await.unwrap();
        std::fs::remove_file(token_path).unwrap();

        assert!(request.starts_with(
            "POST /kapis/frontend-forge-api.kubesphere.io/v1alpha1/frontendextensions/fi-demo/\
             publish "
        ));
        assert!(request.contains("authorization: Bearer token-1"));
        assert!(request.contains("\"requestId\":\"fi-migration-fi-demo-abcdef123456\""));
        assert!(request.contains("\"expectedArtifactDigest\":\"sha256:abcdef1234567890\""));
    }

    #[test]
    fn crd_status_subresource_detection() {
        let value = json!({
            "status": {
                "conditions": [{ "type": "Established", "status": "True" }]
            },
            "spec": {
                "versions": [{
                    "name": "v1alpha1",
                    "served": true,
                    "subresources": { "status": {} }
                }]
            }
        });

        assert!(crd_established(&value));
        assert!(crd_has_v1alpha1_status_subresource(&value));
    }
}
