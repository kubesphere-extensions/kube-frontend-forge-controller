use std::{
    collections::BTreeMap,
    env,
    net::{AddrParseError, SocketAddr},
    sync::Arc,
};

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State},
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

mod artifact;
mod error;
mod handlers;
mod publish;
mod requests;
mod routes;
mod state;

#[cfg(test)]
mod tests;

pub(crate) use artifact::*;
pub(crate) use error::*;
pub(crate) use handlers::*;
pub(crate) use publish::*;
pub(crate) use requests::*;
pub(crate) use routes::*;
pub(crate) use state::*;

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
