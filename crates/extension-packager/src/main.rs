use std::{collections::BTreeMap, env};

use chrono::Utc;
use frontend_forge_api::FrontendExtension;
use frontend_forge_build_service_client::{
    BuildServiceClient, BuildServiceError, select_bundle_artifact,
};
use frontend_forge_common::{
    ANNO_ARTIFACT_DIGEST, ANNO_ARTIFACT_FILENAME, ANNO_ARTIFACT_KEY, ANNO_SOURCE_HASH,
    LABEL_ARTIFACT_KEY_SHORT, LABEL_FE_NAME, LABEL_FE_UID, LABEL_MANAGED_BY, LABEL_PACKAGE_KIND,
    LABEL_SOURCE_HASH_SHORT, MANAGED_BY_VALUE, PACKAGE_KIND_VALUE, hash_label_value,
    manifest_content_and_hash,
};
use frontend_forge_extension_package_core::{
    ExtensionPackageArtifact, ExtensionPackageError, build_extension_package,
    frontend_extension_source_hash,
};
use frontend_forge_manifest::{ManifestRenderError, render_frontend_extension_manifest};
use k8s_openapi::{
    ByteString, api::core::v1::ConfigMap, apimachinery::pkg::apis::meta::v1::ObjectMeta,
};
use kube::{Api, Client, Resource, api::PostParams};
use snafu::{ResultExt, Snafu};
use tracing::info;

#[derive(Debug, Snafu)]
enum Error {
    #[snafu(display("missing env {key}: {source}"))]
    MissingEnv {
        key: &'static str,
        source: std::env::VarError,
    },
    #[snafu(display("failed to initialize Kubernetes client in extension packager: {source}"))]
    KubeClientInit {
        #[snafu(source(from(kube::Error, Box::new)))]
        source: Box<kube::Error>,
    },
    #[snafu(display("failed to get FrontendExtension {name}: {source}"))]
    GetFrontendExtension {
        name: String,
        #[snafu(source(from(kube::Error, Box::new)))]
        source: Box<kube::Error>,
    },
    #[snafu(display(
        "FrontendExtension {name} source hash changed while packaging: expected {expected}, \
         observed {observed}"
    ))]
    SourceHashMismatch {
        name: String,
        expected: String,
        observed: String,
    },
    #[snafu(display("failed to build FrontendExtension package for {name}: {source}"))]
    BuildPackage {
        name: String,
        source: ExtensionPackageError,
    },
    #[snafu(display("failed to render FrontendExtension manifest for {name}: {source}"))]
    RenderManifest {
        name: String,
        source: ManifestRenderError,
    },
    #[snafu(display("failed to canonicalize FrontendExtension manifest for {name}: {source}"))]
    ManifestContent {
        name: String,
        source: frontend_forge_common::CommonError,
    },
    #[snafu(display("build-service failed for FrontendExtension {name}: {source}"))]
    BuildService {
        name: String,
        source: BuildServiceError,
    },
    #[snafu(display("failed to create artifact ConfigMap {namespace}/{name}: {source}"))]
    CreateArtifactConfigMap {
        namespace: String,
        name: String,
        #[snafu(source(from(kube::Error, Box::new)))]
        source: Box<kube::Error>,
    },
    #[snafu(display("failed to get existing artifact ConfigMap {namespace}/{name}: {source}"))]
    GetArtifactConfigMap {
        namespace: String,
        name: String,
        #[snafu(source(from(kube::Error, Box::new)))]
        source: Box<kube::Error>,
    },
    #[snafu(display("failed to replace artifact ConfigMap {namespace}/{name}: {source}"))]
    ReplaceArtifactConfigMap {
        namespace: String,
        name: String,
        #[snafu(source(from(kube::Error, Box::new)))]
        source: Box<kube::Error>,
    },
}

#[derive(Clone, Debug)]
struct PackagerConfig {
    fe_name: String,
    fe_uid: String,
    source_hash: String,
    artifact_key: String,
    rebuild_token: String,
    artifact_configmap_namespace: String,
    artifact_configmap_name: String,
    build_service_base_url: String,
    build_service_timeout_seconds: u64,
    jsbundle_config_key: String,
}

impl PackagerConfig {
    fn from_env() -> Result<Self, Error> {
        Ok(Self {
            fe_name: required_env("FE_NAME")?,
            fe_uid: required_env("FE_UID")?,
            source_hash: required_env("SOURCE_HASH")?,
            artifact_key: required_env("ARTIFACT_KEY")?,
            rebuild_token: env::var("REBUILD_TOKEN")
                .map(|token| token.trim().to_string())
                .unwrap_or_default(),
            artifact_configmap_namespace: env::var("ARTIFACT_CONFIGMAP_NAMESPACE")
                .unwrap_or_else(|_| "extension-frontend-forge".to_string()),
            artifact_configmap_name: required_env("ARTIFACT_CONFIGMAP_NAME")?,
            build_service_base_url: required_env("BUILD_SERVICE_BASE_URL")?,
            build_service_timeout_seconds: env::var("BUILD_SERVICE_TIMEOUT_SECONDS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(240),
            jsbundle_config_key: env::var("JSBUNDLE_CONFIG_KEY")
                .unwrap_or_else(|_| "index.js".to_string()),
        })
    }
}

fn required_env(key: &'static str) -> Result<String, Error> {
    env::var(key).context(MissingEnvSnafu { key })
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,frontend_forge_extension_packager=debug".into()),
        )
        .init();

    let cfg = PackagerConfig::from_env()?;
    let client = Client::try_default().await.context(KubeClientInitSnafu)?;
    let fe_api = Api::<FrontendExtension>::all(client.clone());
    let cm_api = Api::<ConfigMap>::namespaced(client, &cfg.artifact_configmap_namespace);

    let fe = fe_api
        .get(&cfg.fe_name)
        .await
        .with_context(|_| GetFrontendExtensionSnafu {
            name: cfg.fe_name.clone(),
        })?;
    let observed_source_hash =
        frontend_extension_source_hash(&fe).with_context(|_| BuildPackageSnafu {
            name: cfg.fe_name.clone(),
        })?;

    if observed_source_hash != cfg.source_hash {
        return Err(Error::SourceHashMismatch {
            name: cfg.fe_name.clone(),
            expected: cfg.source_hash,
            observed: observed_source_hash,
        });
    }

    let manifest_value =
        render_frontend_extension_manifest(&fe).with_context(|_| RenderManifestSnafu {
            name: cfg.fe_name.clone(),
        })?;
    let (manifest_content, _) =
        manifest_content_and_hash(&manifest_value).with_context(|_| ManifestContentSnafu {
            name: cfg.fe_name.clone(),
        })?;
    let build_client = BuildServiceClient::new(
        &cfg.build_service_base_url,
        cfg.build_service_timeout_seconds,
    )
    .with_context(|_| BuildServiceSnafu {
        name: cfg.fe_name.clone(),
    })?;
    let files = build_client
        .build_project(&manifest_content)
        .await
        .with_context(|_| BuildServiceSnafu {
            name: cfg.fe_name.clone(),
        })?;
    let (_, index_js_content) = select_bundle_artifact(&cfg.jsbundle_config_key, files)
        .with_context(|_| BuildServiceSnafu {
            name: cfg.fe_name.clone(),
        })?;

    let artifact =
        build_extension_package(&fe, Utc::now(), &index_js_content).with_context(|_| {
            BuildPackageSnafu {
                name: cfg.fe_name.clone(),
            }
        })?;
    let configmap = artifact_configmap(&cfg, &fe, &artifact);
    upsert_configmap(&cm_api, &cfg, configmap).await?;

    info!(
        fe = %cfg.fe_name,
        configmap = %cfg.artifact_configmap_name,
        artifact_key = %cfg.artifact_key,
        rebuild_token_set = !cfg.rebuild_token.is_empty(),
        digest = %artifact.digest,
        filename = %artifact.filename,
        "extension package artifact written"
    );

    Ok(())
}

fn artifact_configmap(
    cfg: &PackagerConfig,
    fe: &FrontendExtension,
    artifact: &ExtensionPackageArtifact,
) -> ConfigMap {
    ConfigMap {
        metadata: ObjectMeta {
            name: Some(cfg.artifact_configmap_name.clone()),
            namespace: Some(cfg.artifact_configmap_namespace.clone()),
            labels: Some(BTreeMap::from([
                (LABEL_MANAGED_BY.to_string(), MANAGED_BY_VALUE.to_string()),
                (LABEL_FE_NAME.to_string(), cfg.fe_name.clone()),
                (LABEL_FE_UID.to_string(), cfg.fe_uid.clone()),
                (
                    LABEL_SOURCE_HASH_SHORT.to_string(),
                    hash_label_value(&artifact.source_hash),
                ),
                (
                    LABEL_ARTIFACT_KEY_SHORT.to_string(),
                    hash_label_value(&cfg.artifact_key),
                ),
                (
                    LABEL_PACKAGE_KIND.to_string(),
                    PACKAGE_KIND_VALUE.to_string(),
                ),
            ])),
            annotations: Some(BTreeMap::from([
                (ANNO_SOURCE_HASH.to_string(), artifact.source_hash.clone()),
                (ANNO_ARTIFACT_KEY.to_string(), cfg.artifact_key.clone()),
                (ANNO_ARTIFACT_DIGEST.to_string(), artifact.digest.clone()),
                (
                    ANNO_ARTIFACT_FILENAME.to_string(),
                    artifact.filename.clone(),
                ),
            ])),
            owner_references: fe.controller_owner_ref(&()).map(|owner| vec![owner]),
            ..Default::default()
        },
        data: Some(artifact.payload.data.clone()),
        binary_data: Some(
            artifact
                .payload
                .binary_data
                .iter()
                .map(|(key, value)| (key.clone(), ByteString(value.clone())))
                .collect(),
        ),
        immutable: None,
    }
}

async fn upsert_configmap(
    cm_api: &Api<ConfigMap>,
    cfg: &PackagerConfig,
    mut cm: ConfigMap,
) -> Result<(), Error> {
    match cm_api.create(&PostParams::default(), &cm).await {
        Ok(_) => Ok(()),
        Err(kube::Error::Api(ae)) if ae.code == 409 => {
            let existing = cm_api
                .get(&cfg.artifact_configmap_name)
                .await
                .with_context(|_| GetArtifactConfigMapSnafu {
                    namespace: cfg.artifact_configmap_namespace.clone(),
                    name: cfg.artifact_configmap_name.clone(),
                })?;
            cm.metadata.resource_version = existing.metadata.resource_version;
            cm_api
                .replace(&cfg.artifact_configmap_name, &PostParams::default(), &cm)
                .await
                .with_context(|_| ReplaceArtifactConfigMapSnafu {
                    namespace: cfg.artifact_configmap_namespace.clone(),
                    name: cfg.artifact_configmap_name.clone(),
                })?;
            Ok(())
        }
        Err(source) => Err(Error::CreateArtifactConfigMap {
            namespace: cfg.artifact_configmap_namespace.clone(),
            name: cfg.artifact_configmap_name.clone(),
            source: Box::new(source),
        }),
    }
}

#[cfg(test)]
mod tests {
    use frontend_forge_extension_package_core::{PACKAGE_KEY, PackageFile};

    use super::*;

    #[test]
    fn artifact_configmap_uses_binary_package_key() {
        let cfg = PackagerConfig {
            fe_name: "inspecttask".to_string(),
            fe_uid: "fe-uid".to_string(),
            source_hash: "sha256:source".to_string(),
            artifact_key: "sha256:artifactkey".to_string(),
            rebuild_token: "token-1".to_string(),
            artifact_configmap_namespace: "extension-frontend-forge".to_string(),
            artifact_configmap_name: "fe-inspecttask-a1b2c3d4".to_string(),
            build_service_base_url: "http://builder".to_string(),
            build_service_timeout_seconds: 240,
            jsbundle_config_key: "index.js".to_string(),
        };
        let fe: FrontendExtension = serde_yaml::from_str(
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
      frontend:
        menus:
          - displayName: Inspect Tasks
            key: inspecttasks
            pageKey: inspecttasks
            placement: cluster
            type: page
        pages:
          - key: inspecttasks
            placement: cluster
            type: iframe
            iframe:
              src: http://example.test
"#,
        )
        .unwrap();
        let artifact = ExtensionPackageArtifact {
            filename: "inspecttask-0.1.0.tgz".to_string(),
            media_type: "application/gzip".to_string(),
            digest: "sha256:artifact".to_string(),
            size_bytes: 3,
            source_hash: "sha256:source".to_string(),
            generated_at: Utc::now(),
            files: vec![PackageFile {
                path: "extension.yaml".to_string(),
                content: vec![1],
            }],
            payload: frontend_forge_extension_package_core::ConfigMapArtifactPayload {
                data: BTreeMap::new(),
                binary_data: BTreeMap::from([(PACKAGE_KEY.to_string(), vec![1, 2, 3])]),
            },
        };

        let cm = artifact_configmap(&cfg, &fe, &artifact);

        assert_eq!(
            cm.binary_data.unwrap()[PACKAGE_KEY],
            ByteString(vec![1, 2, 3])
        );
        assert_eq!(
            cm.metadata.labels.unwrap()[LABEL_PACKAGE_KIND],
            PACKAGE_KIND_VALUE
        );
        let annotations = cm.metadata.annotations.unwrap();
        assert_eq!(annotations[ANNO_ARTIFACT_DIGEST], "sha256:artifact");
        assert_eq!(annotations[ANNO_ARTIFACT_KEY], "sha256:artifactkey");
    }
}
