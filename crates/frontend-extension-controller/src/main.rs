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
    PublishTargetKind, UnpublishPhase, UnpublishStatus,
};
use frontend_forge_common::{
    ANNO_ARTIFACT_DIGEST, ANNO_ARTIFACT_KEY, ANNO_DELETE_AFTER_UNPUBLISH_REQUEST_ID,
    ANNO_OBSERVED_GENERATION, ANNO_PUBLISH_ARTIFACT_DIGEST, ANNO_PUBLISH_REQUEST_GENERATION,
    ANNO_PUBLISH_REQUEST_ID, ANNO_PUBLISH_REQUEST_SOURCE_HASH, ANNO_PUBLISH_TARGET_KIND,
    ANNO_PUBLISH_TARGET_NAME, ANNO_PUBLISH_TARGET_NAMESPACE, ANNO_REBUILD_TOKEN, ANNO_SOURCE_HASH,
    ANNO_UNPUBLISH_EXTENSION_NAME, ANNO_UNPUBLISH_REQUEST_ID, DEPRECATED_LABEL_FE_PACKAGE_STATUS,
    DEPRECATED_LABEL_FE_PUBLISH_STATUS, FE_PACKAGE_STATUS_FAILED, FE_PACKAGE_STATUS_PACKAGING,
    FE_PACKAGE_STATUS_READY, FE_PUBLISH_STATUS_FAILED, FE_PUBLISH_STATUS_NOT_PUBLISHED,
    FE_PUBLISH_STATUS_PUBLISHED, FE_PUBLISH_STATUS_PUBLISHING, LABEL_ARTIFACT_KEY_SHORT,
    LABEL_BUILD_KIND, LABEL_FE_NAME, LABEL_FE_PACKAGE_STATUS, LABEL_FE_PUBLISH_STATUS,
    LABEL_FE_UID, LABEL_MANAGED_BY, LABEL_PACKAGE_KIND, LABEL_PUBLISH_KIND,
    LABEL_PUBLISH_REQUEST_HASH, LABEL_SOURCE_HASH_SHORT, LABEL_UNPUBLISH_KIND,
    LABEL_UNPUBLISH_REQUEST_HASH, MANAGED_BY_VALUE, ObservedJobPhase, PACKAGE_KIND_VALUE,
    PUBLISH_KIND_VALUE, UNPUBLISH_KIND_VALUE, artifact_configmap_name, artifact_key,
    base_owner_ref, create_or_get_job, extract_job_message, hash_label_value, observed_job_phase,
    package_job_name, publish_job_name, sha256_hex, unpublish_job_name,
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

mod artifact;
mod config;
mod controller;
mod error;
mod package;
mod publish;
mod status;
mod unpublish;

#[cfg(test)]
mod tests;

pub(crate) use artifact::*;
pub(crate) use config::*;
pub(crate) use controller::run;
pub(crate) use error::*;
pub(crate) use package::*;
pub(crate) use publish::*;
pub(crate) use status::*;
pub(crate) use unpublish::*;

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
