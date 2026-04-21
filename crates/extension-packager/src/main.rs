use std::{collections::BTreeMap, env};

use chrono::Utc;
use frontend_forge_api::FrontendExtension;
use frontend_forge_common::{
    ANNO_ARTIFACT_DIGEST, ANNO_ARTIFACT_FILENAME, ANNO_SOURCE_HASH, LABEL_FE_NAME,
    LABEL_MANAGED_BY, LABEL_PACKAGE_KIND, LABEL_SOURCE_HASH, MANAGED_BY_VALUE, PACKAGE_KIND_VALUE,
    hash_label_value,
};
use frontend_forge_extension_package_core::{
    ExtensionPackageArtifact, ExtensionPackageError, build_extension_package,
    frontend_extension_source_hash,
};
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
    source_hash: String,
    artifact_configmap_namespace: String,
    artifact_configmap_name: String,
}

impl PackagerConfig {
    fn from_env() -> Result<Self, Error> {
        Ok(Self {
            fe_name: required_env("FE_NAME")?,
            source_hash: required_env("SOURCE_HASH")?,
            artifact_configmap_namespace: env::var("ARTIFACT_CONFIGMAP_NAMESPACE")
                .unwrap_or_else(|_| "extension-frontend-forge".to_string()),
            artifact_configmap_name: required_env("ARTIFACT_CONFIGMAP_NAME")?,
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

    let artifact =
        build_extension_package(&fe, Utc::now()).with_context(|_| BuildPackageSnafu {
            name: cfg.fe_name.clone(),
        })?;
    let configmap = artifact_configmap(&cfg, &fe, &artifact);
    upsert_configmap(&cm_api, &cfg, configmap).await?;

    info!(
        fe = %cfg.fe_name,
        configmap = %cfg.artifact_configmap_name,
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
                (
                    LABEL_SOURCE_HASH.to_string(),
                    hash_label_value(&artifact.source_hash),
                ),
                (
                    LABEL_PACKAGE_KIND.to_string(),
                    PACKAGE_KIND_VALUE.to_string(),
                ),
            ])),
            annotations: Some(BTreeMap::from([
                (ANNO_SOURCE_HASH.to_string(), artifact.source_hash.clone()),
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
            source_hash: "sha256:source".to_string(),
            artifact_configmap_namespace: "extension-frontend-forge".to_string(),
            artifact_configmap_name: "fe-inspecttask-a1b2c3d4".to_string(),
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
            placement: cluster
            type: page
        pages:
          - key: inspecttasks
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
        assert_eq!(
            cm.metadata.annotations.unwrap()[ANNO_ARTIFACT_DIGEST],
            "sha256:artifact"
        );
    }
}
