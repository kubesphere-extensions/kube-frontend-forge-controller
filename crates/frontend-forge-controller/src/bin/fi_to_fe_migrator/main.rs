use std::{collections::BTreeMap, env, time::Duration};

use frontend_forge_api::{
    ExtensionProviderSpec, FrontendExtension, FrontendExtensionFrontendSpec,
    FrontendExtensionPackageSpec, FrontendExtensionPageSpec, FrontendExtensionPrimaryMenuSpec,
    FrontendExtensionSourceSpec, FrontendExtensionSourceType, FrontendExtensionSpec,
    FrontendIntegration, InlineFrontendExtensionSourceSpec, MenuNodeType, MenuPlacement,
    NamespacedResourceRef, PublishPolicyMode, PublishPolicySpec, PublishTargetKind,
};
use frontend_forge_common::{
    ANNO_PUBLISH_ARTIFACT_DIGEST, ANNO_PUBLISH_REQUEST_GENERATION, ANNO_PUBLISH_REQUEST_ID,
    ANNO_PUBLISH_REQUEST_SOURCE_HASH, ANNO_PUBLISH_TARGET_KIND, ANNO_PUBLISH_TARGET_NAME,
    ANNO_PUBLISH_TARGET_NAMESPACE, sha256_hex,
};
use frontend_forge_extension_package_core::frontend_extension_source_hash;
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::{
    Api, Client, Resource, ResourceExt,
    api::{DeleteParams, Patch, PatchParams, PostParams},
};
use serde_json::{Value, json};
use snafu::Snafu;
use tokio::time::{Instant, sleep};
use tracing::{error, info, warn};
const LABEL_MANAGED_BY: &str = "frontend-forge.kubesphere.io/managed-by";
const MANAGED_BY_VALUE: &str = "frontend-forge-fi-migrator";
const ANNO_SOURCE_FI_NAME: &str = "frontend-forge.kubesphere.io/source-fi-name";
const ANNO_SOURCE_FI_UID: &str = "frontend-forge.kubesphere.io/source-fi-uid";
const DEFAULT_PACKAGE_VERSION: &str = "0.1.0";
const DEFAULT_SCHEMA_VERSION: &str = "v1";
const DEFAULT_PACKAGE_ICON: &str = "./static/favicon.svg";
const DEFAULT_PACKAGE_CATEGORY: &str = "dev-tools";
const DEFAULT_PROVIDER_NAME: &str = "Fi Migration Bot";
const ANNO_CREATOR: &str = "kubesphere.io/creator";
const DEFAULT_READY_TIMEOUT_SECONDS: u64 = 600;
const DEFAULT_POLL_INTERVAL_SECONDS: u64 = 5;
const DEFAULT_PUBLISH_TARGET_KIND: &str = "ConfigMap";
const DEFAULT_PUBLISH_TARGET_NAME: &str = "ksbuilder-publish-config";

#[derive(Debug, Snafu)]
enum Error {
    #[snafu(display("failed to initialize Kubernetes client: {source}"))]
    KubeClientInit { source: Box<kube::Error> },
    #[snafu(display("Kubernetes operation failed while {action}: {source}"))]
    Kube {
        action: String,
        source: Box<kube::Error>,
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

mod config;
mod crd;
mod frontend_extension;
mod migration;
mod naming;
mod publish;

#[cfg(test)]
mod tests;

pub(crate) use config::*;
pub(crate) use crd::*;
pub(crate) use frontend_extension::*;
pub(crate) use migration::*;
pub(crate) use naming::*;
pub(crate) use publish::*;

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
        .map_err(|source| Error::KubeClientInit {
            source: Box::new(source),
        })?;
    run(client, cfg).await
}
