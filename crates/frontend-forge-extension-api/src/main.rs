use std::{
    collections::BTreeMap,
    env,
    net::{AddrParseError, SocketAddr},
    sync::Arc,
};

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State},
    http::{
        HeaderValue, Response, StatusCode,
        header::{CONTENT_DISPOSITION, CONTENT_TYPE},
    },
    response::IntoResponse,
    routing::{get, post},
};
use chrono::Utc;
use frontend_forge_api::{
    ArtifactStorageKind, ExtensionArtifactStatus, FrontendExtension, FrontendExtensionPhase,
    NamespacedResourceRef, PublishPhase, PublishStatus, PublishTargetKind, UnpublishPhase,
    UnpublishStatus,
};
use frontend_forge_common::{
    ANNO_DELETE_AFTER_UNPUBLISH_REQUEST_ID, ANNO_PUBLISH_ARTIFACT_DIGEST, ANNO_PUBLISH_REQUEST_ID,
    ANNO_PUBLISH_TARGET_KIND, ANNO_PUBLISH_TARGET_NAME, ANNO_PUBLISH_TARGET_NAMESPACE,
    ANNO_UNPUBLISH_EXTENSION_NAME, ANNO_UNPUBLISH_REQUEST_ID, sha256_hex,
};
use frontend_forge_extension_package_core::{PACKAGE_KEY, frontend_extension_package_name};
use k8s_openapi::api::core::v1::ConfigMap;
use kube::{
    Api, Client, ResourceExt,
    api::{DeleteParams, ListParams, Patch, PatchParams, PostParams},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use snafu::{ResultExt, Snafu};
use tracing::info;

const CRD_API_GROUP: &str = "frontend-forge.kubesphere.io";
const DEFAULT_EXTENSION_API_GROUP: &str = "frontend-forge-api.kubesphere.io";
const DEFAULT_API_VERSION: &str = "v1alpha1";
const API_RESOURCE: &str = "frontendextensions";
const KUBERNETES_API_PREFIX: &str = "/apis";
const KUBESPHERE_API_PREFIX: &str = "/kapis";

#[derive(Debug, Snafu)]
enum Error {
    #[snafu(display("failed to initialize Kubernetes client: {source}"))]
    KubeClientInit { source: kube::Error },
    #[snafu(display("invalid EXTENSION_API_BIND_ADDR '{value}': {source}"))]
    InvalidBindAddr {
        value: String,
        source: AddrParseError,
    },
    #[snafu(display("extension API server failed on {bind_addr}: {source}"))]
    Server {
        bind_addr: SocketAddr,
        source: std::io::Error,
    },
}

#[derive(Clone)]
struct AppState {
    client: Client,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    fn kube(action: &str, source: &kube::Error) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("{action}: {source}"),
        )
    }

    fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, message)
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (
            self.status,
            Json(ErrorResponse {
                error: self.message,
            }),
        )
            .into_response()
    }
}

#[derive(Debug, Serialize)]
struct FrontendExtensionListResponse {
    items: Vec<FrontendExtensionSummary>,
}

#[derive(Debug, Serialize)]
struct FrontendExtensionSummary {
    name: String,
    generation: Option<i64>,
    package: FrontendExtensionPackageSummary,
    phase: FrontendExtensionPhase,
    #[serde(skip_serializing_if = "Option::is_none", rename = "artifactDigest")]
    artifact_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    download: Option<DownloadSummary>,
    publish: PublishStatus,
}

#[derive(Debug, Serialize)]
struct FrontendExtensionPackageSummary {
    version: String,
    #[serde(rename = "displayName")]
    display_name: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
struct DownloadSummary {
    ready: bool,
    filename: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PublishRequest {
    #[serde(default, rename = "requestId")]
    request_id: Option<String>,
    #[serde(default, rename = "expectedArtifactDigest")]
    expected_artifact_digest: Option<String>,
}

#[derive(Debug)]
struct ResolvedPublishRequest {
    request_id: String,
    artifact_digest: String,
    target_ref: NamespacedResourceRef,
    target_kind: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UnpublishRequest {
    #[serde(default, rename = "requestId")]
    request_id: Option<String>,
}

#[derive(Debug)]
struct ResolvedUnpublishRequest {
    request_id: String,
    extension_name: String,
    target_ref: NamespacedResourceRef,
    target_kind: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteRequest {
    #[serde(default)]
    unpublish: bool,
}

#[derive(Debug, Serialize)]
struct DeleteResponse {
    deleted: bool,
    #[serde(rename = "unpublishSkipped")]
    unpublish_skipped: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    unpublish: Option<UnpublishStatus>,
}

fn install_rustls_crypto_provider() {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        rustls::crypto::ring::default_provider()
            .install_default()
            .expect("ring crypto provider should install before extension API startup");
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    install_rustls_crypto_provider();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,frontend_forge_extension_api=debug".into()),
        )
        .init();

    let client = Client::try_default().await.context(KubeClientInitSnafu)?;
    let state = Arc::new(AppState { client });
    let extension_api_group =
        env::var("EXTENSION_API_GROUP").unwrap_or_else(|_| DEFAULT_EXTENSION_API_GROUP.to_string());
    let extension_api_version =
        env::var("EXTENSION_API_VERSION").unwrap_or_else(|_| DEFAULT_API_VERSION.to_string());
    let mut app = api_routes(KUBERNETES_API_PREFIX, CRD_API_GROUP, DEFAULT_API_VERSION);
    if extension_api_group != CRD_API_GROUP || extension_api_version != DEFAULT_API_VERSION {
        app = app.merge(api_routes(
            KUBERNETES_API_PREFIX,
            &extension_api_group,
            &extension_api_version,
        ));
    }
    let app = app
        .merge(api_routes(
            KUBESPHERE_API_PREFIX,
            &extension_api_group,
            &extension_api_version,
        ))
        .with_state(state);

    let bind_addr_raw =
        env::var("EXTENSION_API_BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    let bind_addr: SocketAddr = bind_addr_raw
        .parse()
        .with_context(|_| InvalidBindAddrSnafu {
            value: bind_addr_raw.clone(),
        })?;
    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .context(ServerSnafu { bind_addr })?;
    info!(%bind_addr, "extension API server started");
    axum::serve(listener, app)
        .await
        .context(ServerSnafu { bind_addr })?;

    Ok(())
}

fn api_routes(prefix: &str, group: &str, version: &str) -> Router<Arc<AppState>> {
    let resource_prefix = format!("{prefix}/{group}/{version}/{API_RESOURCE}");
    Router::new()
        .route(
            &resource_prefix,
            get(list_extensions).post(create_extension),
        )
        .route(&format!("{resource_prefix}/{{name}}"), get(get_extension))
        .route(
            &format!("{resource_prefix}/{{name}}/download"),
            get(download_extension),
        )
        .route(
            &format!("{resource_prefix}/{{name}}/publish"),
            get(get_publish_status).post(trigger_publish),
        )
        .route(
            &format!("{resource_prefix}/{{name}}/unpublish"),
            get(get_unpublish_status).post(trigger_unpublish),
        )
        .route(
            &format!("{resource_prefix}/{{name}}/delete"),
            post(delete_extension),
        )
}

async fn list_extensions(
    State(state): State<Arc<AppState>>,
) -> Result<Json<FrontendExtensionListResponse>, ApiError> {
    let api = Api::<FrontendExtension>::all(state.client.clone());
    let list = api
        .list(&ListParams::default())
        .await
        .map_err(|err| ApiError::kube("failed to list FrontendExtensions", &err))?;
    Ok(Json(FrontendExtensionListResponse {
        items: list.items.iter().map(extension_summary).collect(),
    }))
}

async fn create_extension(
    State(state): State<Arc<AppState>>,
    Json(extension): Json<FrontendExtension>,
) -> Result<Json<FrontendExtension>, ApiError> {
    let api = Api::<FrontendExtension>::all(state.client.clone());
    let created = api
        .create(&PostParams::default(), &extension)
        .await
        .map_err(|err| ApiError::kube("failed to create FrontendExtension", &err))?;
    Ok(Json(created))
}

async fn get_extension(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<FrontendExtension>, ApiError> {
    let extension = get_fe(&state, &name).await?;
    Ok(Json(extension))
}

async fn get_publish_status(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<PublishStatus>, ApiError> {
    let extension = get_fe(&state, &name).await?;
    Ok(Json(
        extension
            .status
            .and_then(|status| status.publish)
            .unwrap_or_default(),
    ))
}

async fn get_unpublish_status(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<UnpublishStatus>, ApiError> {
    let extension = get_fe(&state, &name).await?;
    Ok(Json(
        extension
            .status
            .and_then(|status| status.unpublish)
            .unwrap_or_default(),
    ))
}

async fn download_extension(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Response<Body>, ApiError> {
    let extension = get_fe(&state, &name).await?;
    let artifact = ready_artifact(&extension)?;
    let bytes = artifact_bytes(&state, artifact).await?;
    verify_artifact_digest(&bytes, &artifact.digest)?;

    let mut response = Response::new(Body::from(bytes));
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_str(&artifact.media_type).map_err(|err| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("invalid artifact media type: {err}"),
            )
        })?,
    );
    response.headers_mut().insert(
        CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{}\"", artifact.filename)).map_err(
            |err| {
                ApiError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("invalid artifact filename: {err}"),
                )
            },
        )?,
    );
    Ok(response)
}

async fn trigger_unpublish(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(request): Json<UnpublishRequest>,
) -> Result<(StatusCode, Json<UnpublishStatus>), ApiError> {
    let extension = get_fe(&state, &name).await?;
    let request = resolve_unpublish_request(&extension, &request)?;

    if let Some(current) = extension
        .status
        .as_ref()
        .and_then(|status| status.unpublish.as_ref())
        && current.request_id.as_deref() == Some(request.request_id.as_str())
        && current.extension_name.as_deref() == Some(request.extension_name.as_str())
    {
        return Ok((StatusCode::ACCEPTED, Json(current.clone())));
    }

    patch_unpublish_request(&state, &name, &request, None).await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(UnpublishStatus {
            phase: UnpublishPhase::Pending,
            request_id: Some(request.request_id),
            extension_name: Some(request.extension_name),
            ..Default::default()
        }),
    ))
}

async fn delete_extension(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(request): Json<DeleteRequest>,
) -> Result<(StatusCode, Json<DeleteResponse>), ApiError> {
    let extension = get_fe(&state, &name).await?;
    if request.unpublish && currently_published(&extension) {
        let unpublish_request =
            resolve_unpublish_request(&extension, &UnpublishRequest { request_id: None })?;
        patch_unpublish_request(
            &state,
            &name,
            &unpublish_request,
            Some(unpublish_request.request_id.as_str()),
        )
        .await?;
        return Ok((
            StatusCode::ACCEPTED,
            Json(DeleteResponse {
                deleted: false,
                unpublish_skipped: false,
                unpublish: Some(UnpublishStatus {
                    phase: UnpublishPhase::Pending,
                    request_id: Some(unpublish_request.request_id),
                    extension_name: Some(unpublish_request.extension_name),
                    ..Default::default()
                }),
            }),
        ));
    }

    delete_fe(&state, &name).await?;
    Ok((
        StatusCode::OK,
        Json(DeleteResponse {
            deleted: true,
            unpublish_skipped: true,
            unpublish: None,
        }),
    ))
}

async fn trigger_publish(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(request): Json<PublishRequest>,
) -> Result<(StatusCode, Json<PublishStatus>), ApiError> {
    let extension = get_fe(&state, &name).await?;
    let artifact = ready_artifact(&extension)?;
    if request
        .expected_artifact_digest
        .as_deref()
        .is_some_and(|expected| expected != artifact.digest)
    {
        return Err(ApiError::conflict(
            "publish expectedArtifactDigest does not match current ready artifact",
        ));
    }
    let request = resolve_publish_request(&extension, &request, artifact)?;

    if let Some(current) = extension
        .status
        .as_ref()
        .and_then(|status| status.publish.as_ref())
        && current.request_id.as_deref() == Some(request.request_id.as_str())
        && current.artifact_digest.as_deref() == Some(request.artifact_digest.as_str())
    {
        return Ok((StatusCode::ACCEPTED, Json(current.clone())));
    }

    patch_publish_request(&state, &name, &request).await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(PublishStatus {
            phase: frontend_forge_api::PublishPhase::Pending,
            request_id: Some(request.request_id),
            artifact_digest: Some(request.artifact_digest),
            ..Default::default()
        }),
    ))
}

fn resolve_publish_request(
    fe: &FrontendExtension,
    request: &PublishRequest,
    artifact: &ExtensionArtifactStatus,
) -> Result<ResolvedPublishRequest, ApiError> {
    let target_ref = publish_target_ref(fe)
        .ok_or_else(|| ApiError::conflict("publish targetRef is required"))?;
    if target_ref.namespace.is_empty() || target_ref.name.is_empty() {
        return Err(ApiError::conflict(
            "publish targetRef namespace and name are required",
        ));
    }
    let target_kind = publish_target_kind(fe);
    if !matches!(target_kind.as_str(), "ConfigMap" | "Secret") {
        return Err(ApiError::conflict(
            "publish targetKind must be ConfigMap or Secret",
        ));
    }

    Ok(ResolvedPublishRequest {
        request_id: request
            .request_id
            .as_deref()
            .map(str::trim)
            .filter(|request_id| !request_id.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| generated_publish_request_id(&artifact.digest)),
        artifact_digest: artifact.digest.clone(),
        target_ref,
        target_kind,
    })
}

fn resolve_unpublish_request(
    fe: &FrontendExtension,
    request: &UnpublishRequest,
) -> Result<ResolvedUnpublishRequest, ApiError> {
    let target_ref = publish_target_ref(fe)
        .ok_or_else(|| ApiError::conflict("publish targetRef is required"))?;
    if target_ref.namespace.is_empty() || target_ref.name.is_empty() {
        return Err(ApiError::conflict(
            "publish targetRef namespace and name are required",
        ));
    }
    let target_kind = publish_target_kind(fe);
    if !matches!(target_kind.as_str(), "ConfigMap" | "Secret") {
        return Err(ApiError::conflict(
            "publish targetKind must be ConfigMap or Secret",
        ));
    }

    let extension_name = frontend_extension_package_name(fe);
    Ok(ResolvedUnpublishRequest {
        request_id: request
            .request_id
            .as_deref()
            .map(str::trim)
            .filter(|request_id| !request_id.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| generated_unpublish_request_id(&extension_name)),
        extension_name,
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

fn generated_publish_request_id(artifact_digest: &str) -> String {
    let timestamp = Utc::now().timestamp_nanos_opt().map_or_else(
        || Utc::now().timestamp_millis().to_string(),
        |ts| ts.to_string(),
    );
    let digest_suffix = artifact_digest
        .strip_prefix("sha256:")
        .unwrap_or(artifact_digest)
        .chars()
        .take(12)
        .collect::<String>();
    format!("api-{timestamp}-{digest_suffix}")
}

fn generated_unpublish_request_id(extension_name: &str) -> String {
    let timestamp = Utc::now().timestamp_nanos_opt().map_or_else(
        || Utc::now().timestamp_millis().to_string(),
        |ts| ts.to_string(),
    );
    let name_hash = sha256_hex(extension_name.as_bytes())
        .chars()
        .take(12)
        .collect::<String>();
    format!("api-unpublish-{timestamp}-{name_hash}")
}

async fn get_fe(state: &AppState, name: &str) -> Result<FrontendExtension, ApiError> {
    let api = Api::<FrontendExtension>::all(state.client.clone());
    api.get(name).await.map_err(|err| match err {
        kube::Error::Api(ae) if ae.code == 404 => {
            ApiError::not_found(format!("FrontendExtension {name} not found"))
        }
        err => ApiError::kube("failed to get FrontendExtension", &err),
    })
}

async fn delete_fe(state: &AppState, name: &str) -> Result<(), ApiError> {
    let api = Api::<FrontendExtension>::all(state.client.clone());
    api.delete(name, &DeleteParams::default())
        .await
        .map_err(|err| match err {
            kube::Error::Api(ae) if ae.code == 404 => {
                ApiError::not_found(format!("FrontendExtension {name} not found"))
            }
            err => ApiError::kube("failed to delete FrontendExtension", &err),
        })?;
    Ok(())
}

fn extension_summary(fe: &FrontendExtension) -> FrontendExtensionSummary {
    let status = fe.status.clone().unwrap_or_default();
    FrontendExtensionSummary {
        name: fe.name_any(),
        generation: fe.metadata.generation,
        package: FrontendExtensionPackageSummary {
            version: fe.spec.package.version.clone(),
            display_name: fe.spec.package.display_name.clone(),
        },
        phase: status.phase,
        artifact_digest: status
            .artifact
            .as_ref()
            .map(|artifact| artifact.digest.clone()),
        download: status.download.map(|download| DownloadSummary {
            ready: download.ready,
            filename: download.filename,
        }),
        publish: status.publish.unwrap_or_default(),
    }
}

fn currently_published(fe: &FrontendExtension) -> bool {
    fe.status
        .as_ref()
        .and_then(|status| status.publish.as_ref())
        .is_some_and(|publish| matches!(publish.phase, PublishPhase::Succeeded) && publish.active)
}

fn ready_artifact(fe: &FrontendExtension) -> Result<&ExtensionArtifactStatus, ApiError> {
    let status = fe
        .status
        .as_ref()
        .ok_or_else(|| ApiError::conflict("FrontendExtension has no status yet"))?;
    if !matches!(status.phase, FrontendExtensionPhase::Ready) {
        return Err(ApiError::conflict(
            "FrontendExtension artifact is not ready",
        ));
    }
    let download = status
        .download
        .as_ref()
        .ok_or_else(|| ApiError::conflict("FrontendExtension download status is missing"))?;
    if !download.ready {
        return Err(ApiError::conflict(
            "FrontendExtension artifact is not downloadable",
        ));
    }
    let artifact = status
        .artifact
        .as_ref()
        .ok_or_else(|| ApiError::conflict("FrontendExtension artifact status is missing"))?;
    if status.observed_source_hash.as_deref() != Some(artifact.source_hash.as_str()) {
        return Err(ApiError::conflict(
            "FrontendExtension artifact does not match observed source hash",
        ));
    }
    if !matches!(artifact.storage.kind, ArtifactStorageKind::ConfigMap) {
        return Err(ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "unsupported artifact storage kind",
        ));
    }
    Ok(artifact)
}

async fn artifact_bytes(
    state: &AppState,
    artifact: &ExtensionArtifactStatus,
) -> Result<Vec<u8>, ApiError> {
    let key = if artifact.storage.key.is_empty() {
        PACKAGE_KEY
    } else {
        artifact.storage.key.as_str()
    };
    let cm_api =
        Api::<ConfigMap>::namespaced(state.client.clone(), &artifact.storage.ref_.namespace);
    let cm = cm_api
        .get(&artifact.storage.ref_.name)
        .await
        .map_err(|err| ApiError::kube("failed to get artifact ConfigMap", &err))?;
    cm.binary_data
        .as_ref()
        .and_then(|data| data.get(key))
        .map(|bytes| bytes.0.clone())
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("artifact ConfigMap is missing binaryData key {key}"),
            )
        })
}

fn verify_artifact_digest(bytes: &[u8], expected: &str) -> Result<(), ApiError> {
    let observed = format!("sha256:{}", sha256_hex(bytes));
    if observed == expected {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "artifact digest mismatch",
        ))
    }
}

async fn patch_publish_request(
    state: &AppState,
    name: &str,
    request: &ResolvedPublishRequest,
) -> Result<(), ApiError> {
    let api = Api::<FrontendExtension>::all(state.client.clone());
    let annotations = BTreeMap::from([
        (
            ANNO_PUBLISH_REQUEST_ID.to_string(),
            request.request_id.clone(),
        ),
        (
            ANNO_PUBLISH_ARTIFACT_DIGEST.to_string(),
            request.artifact_digest.clone(),
        ),
        (
            ANNO_PUBLISH_TARGET_KIND.to_string(),
            request.target_kind.clone(),
        ),
        (
            ANNO_PUBLISH_TARGET_NAMESPACE.to_string(),
            request.target_ref.namespace.clone(),
        ),
        (
            ANNO_PUBLISH_TARGET_NAME.to_string(),
            request.target_ref.name.clone(),
        ),
    ]);
    let patch = json!({
        "metadata": {
            "annotations": annotations,
        }
    });
    api.patch(name, &PatchParams::default(), &Patch::Merge(&patch))
        .await
        .map_err(|err| ApiError::kube("failed to patch publish request", &err))?;
    Ok(())
}

async fn patch_unpublish_request(
    state: &AppState,
    name: &str,
    request: &ResolvedUnpublishRequest,
    delete_after_request_id: Option<&str>,
) -> Result<(), ApiError> {
    let api = Api::<FrontendExtension>::all(state.client.clone());
    let mut annotations = BTreeMap::from([
        (
            ANNO_UNPUBLISH_REQUEST_ID.to_string(),
            request.request_id.clone(),
        ),
        (
            ANNO_UNPUBLISH_EXTENSION_NAME.to_string(),
            request.extension_name.clone(),
        ),
        (
            ANNO_PUBLISH_TARGET_KIND.to_string(),
            request.target_kind.clone(),
        ),
        (
            ANNO_PUBLISH_TARGET_NAMESPACE.to_string(),
            request.target_ref.namespace.clone(),
        ),
        (
            ANNO_PUBLISH_TARGET_NAME.to_string(),
            request.target_ref.name.clone(),
        ),
    ]);
    if let Some(delete_after_request_id) = delete_after_request_id {
        annotations.insert(
            ANNO_DELETE_AFTER_UNPUBLISH_REQUEST_ID.to_string(),
            delete_after_request_id.to_string(),
        );
    }
    let patch = json!({
        "metadata": {
            "annotations": annotations,
        }
    });
    api.patch(name, &PatchParams::default(), &Patch::Merge(&patch))
        .await
        .map_err(|err| ApiError::kube("failed to patch unpublish request", &err))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use frontend_forge_api::{
        ArtifactStorageStatus, ExtensionDownloadStatus, FrontendExtensionStatus, PublishPolicyMode,
        PublishPolicySpec,
    };
    use kube::core::ObjectMeta;

    use super::*;

    fn ready_fe() -> FrontendExtension {
        serde_yaml::from_str(
            r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendExtension
metadata:
  name: inspecttask
spec:
  package:
    version: 0.1.0
    displayName:
      en: Inspect Task
    description:
      en: InspectTask extension package
  source:
    type: Inline
    inline:
      schemaVersion: v1
      frontend: {}
"#,
        )
        .unwrap()
    }

    #[test]
    fn ready_artifact_requires_matching_source_hash() {
        let mut fe = ready_fe();
        fe.metadata = ObjectMeta {
            name: Some("inspecttask".to_string()),
            ..Default::default()
        };
        fe.status = Some(FrontendExtensionStatus {
            phase: FrontendExtensionPhase::Ready,
            observed_source_hash: Some("sha256:new".to_string()),
            artifact: Some(ExtensionArtifactStatus {
                storage: ArtifactStorageStatus {
                    kind: ArtifactStorageKind::ConfigMap,
                    ref_: NamespacedResourceRef {
                        namespace: "extension-frontend-forge".to_string(),
                        name: "fe-inspecttask-a1b2c3d4".to_string(),
                        uid: None,
                    },
                    key: "package.tgz".to_string(),
                },
                digest: "sha256:artifact".to_string(),
                size_bytes: 1,
                media_type: "application/gzip".to_string(),
                filename: "inspecttask-0.1.0.tgz".to_string(),
                generated_at: chrono::Utc::now(),
                source_hash: "sha256:old".to_string(),
                artifact_key: Some("sha256:artifact-key".to_string()),
            }),
            download: Some(ExtensionDownloadStatus {
                ready: true,
                filename: "inspecttask-0.1.0.tgz".to_string(),
                media_type: "application/gzip".to_string(),
            }),
            ..Default::default()
        });

        assert!(ready_artifact(&fe).is_err());
    }

    #[test]
    fn resolve_publish_request_uses_fe_publish_policy_and_current_artifact() {
        let mut fe = ready_fe();
        fe.spec.publish_policy = Some(PublishPolicySpec {
            mode: PublishPolicyMode::Manual,
            default_target_kind: Some(PublishTargetKind::Secret),
            default_target_ref: Some(NamespacedResourceRef {
                namespace: "extension-frontend-forge".to_string(),
                name: "ksbuilder-publish-config".to_string(),
                uid: None,
            }),
        });
        let artifact = ExtensionArtifactStatus {
            storage: ArtifactStorageStatus {
                kind: ArtifactStorageKind::ConfigMap,
                ref_: NamespacedResourceRef {
                    namespace: "extension-frontend-forge".to_string(),
                    name: "fe-inspecttask-a1b2c3d4".to_string(),
                    uid: None,
                },
                key: "package.tgz".to_string(),
            },
            digest: "sha256:artifact".to_string(),
            size_bytes: 1,
            media_type: "application/gzip".to_string(),
            filename: "inspecttask-0.1.0.tgz".to_string(),
            generated_at: chrono::Utc::now(),
            source_hash: "sha256:source".to_string(),
            artifact_key: Some("sha256:artifact-key".to_string()),
        };
        let request = PublishRequest {
            request_id: Some(" request-1 ".to_string()),
            expected_artifact_digest: None,
        };

        let resolved = resolve_publish_request(&fe, &request, &artifact).unwrap();

        assert_eq!(resolved.request_id, "request-1");
        assert_eq!(resolved.artifact_digest, "sha256:artifact");
        assert_eq!(resolved.target_kind, "Secret");
        assert_eq!(resolved.target_ref.name, "ksbuilder-publish-config");
    }

    #[test]
    fn currently_published_requires_succeeded_and_active() {
        let mut fe = ready_fe();
        fe.status = Some(FrontendExtensionStatus {
            publish: Some(PublishStatus {
                phase: PublishPhase::Succeeded,
                active: true,
                ..Default::default()
            }),
            ..Default::default()
        });
        assert!(currently_published(&fe));

        fe.status = Some(FrontendExtensionStatus {
            publish: Some(PublishStatus {
                phase: PublishPhase::Succeeded,
                active: false,
                ..Default::default()
            }),
            ..Default::default()
        });
        assert!(!currently_published(&fe));
    }

    #[test]
    fn ready_artifact_requires_ready_phase() {
        let mut fe = ready_fe();
        fe.status = Some(FrontendExtensionStatus {
            phase: FrontendExtensionPhase::Packaging,
            ..Default::default()
        });

        let err = ready_artifact(&fe).unwrap_err();

        assert_eq!(err.status, StatusCode::CONFLICT);
        assert_eq!(err.message, "FrontendExtension artifact is not ready");
    }

    #[test]
    fn ready_artifact_requires_download_ready() {
        let mut fe = ready_fe();
        fe.status = Some(FrontendExtensionStatus {
            phase: FrontendExtensionPhase::Ready,
            observed_source_hash: Some("sha256:source".to_string()),
            artifact: Some(ExtensionArtifactStatus {
                storage: ArtifactStorageStatus {
                    kind: ArtifactStorageKind::ConfigMap,
                    ref_: NamespacedResourceRef {
                        namespace: "extension-frontend-forge".to_string(),
                        name: "fe-inspecttask-a1b2c3d4".to_string(),
                        uid: None,
                    },
                    key: "package.tgz".to_string(),
                },
                digest: "sha256:artifact".to_string(),
                size_bytes: 1,
                media_type: "application/gzip".to_string(),
                filename: "inspecttask-0.1.0.tgz".to_string(),
                generated_at: chrono::Utc::now(),
                source_hash: "sha256:source".to_string(),
                artifact_key: Some("sha256:artifact-key".to_string()),
            }),
            download: Some(ExtensionDownloadStatus {
                ready: false,
                filename: "inspecttask-0.1.0.tgz".to_string(),
                media_type: "application/gzip".to_string(),
            }),
            ..Default::default()
        });

        let err = ready_artifact(&fe).unwrap_err();

        assert_eq!(err.status, StatusCode::CONFLICT);
        assert_eq!(
            err.message,
            "FrontendExtension artifact is not downloadable"
        );
    }

    #[test]
    fn verify_artifact_digest_rejects_mismatch() {
        let err = verify_artifact_digest(b"package", "sha256:mismatch").unwrap_err();

        assert_eq!(err.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(err.message, "artifact digest mismatch");
    }
}
