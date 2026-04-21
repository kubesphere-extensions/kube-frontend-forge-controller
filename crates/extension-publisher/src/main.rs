use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    fs,
    io::Cursor,
    path::{Component, Path, PathBuf},
    process::Command,
};

use flate2::read::GzDecoder;
use frontend_forge_common::sha256_hex;
use k8s_openapi::{
    ByteString,
    api::core::v1::{ConfigMap, Secret},
};
use kube::{Api, Client};
use snafu::{ResultExt, Snafu};
use tar::Archive;
use tracing::info;

#[derive(Debug, Snafu)]
enum Error {
    #[snafu(display("missing env {key}: {source}"))]
    MissingEnv {
        key: &'static str,
        source: std::env::VarError,
    },
    #[snafu(display("failed to initialize Kubernetes client in extension publisher: {source}"))]
    KubeClientInit {
        #[snafu(source(from(kube::Error, Box::new)))]
        source: Box<kube::Error>,
    },
    #[snafu(display("failed to get artifact ConfigMap {namespace}/{name}: {source}"))]
    GetArtifactConfigMap {
        namespace: String,
        name: String,
        #[snafu(source(from(kube::Error, Box::new)))]
        source: Box<kube::Error>,
    },
    #[snafu(display("artifact ConfigMap {namespace}/{name} is missing binaryData key {key}"))]
    MissingArtifactPackage {
        namespace: String,
        name: String,
        key: String,
    },
    #[snafu(display("artifact digest mismatch: expected {expected}, observed sha256:{observed}"))]
    ArtifactDigestMismatch { expected: String, observed: String },
    #[snafu(display("failed to prepare publish workdir {path}: {source}"))]
    PrepareWorkdir {
        path: String,
        source: std::io::Error,
    },
    #[snafu(display("failed to unpack package into {path}: {source}"))]
    UnpackPackage {
        path: String,
        source: std::io::Error,
    },
    #[snafu(display("failed to get target ConfigMap {namespace}/{name}: {source}"))]
    GetTargetConfigMap {
        namespace: String,
        name: String,
        #[snafu(source(from(kube::Error, Box::new)))]
        source: Box<kube::Error>,
    },
    #[snafu(display("failed to get target Secret {namespace}/{name}: {source}"))]
    GetTargetSecret {
        namespace: String,
        name: String,
        #[snafu(source(from(kube::Error, Box::new)))]
        source: Box<kube::Error>,
    },
    #[snafu(display("unsupported publish target kind {kind}; expected ConfigMap or Secret"))]
    UnsupportedTargetKind { kind: String },
    #[snafu(display("unsafe publish target data path {path}"))]
    UnsafeTargetDataPath { path: String },
    #[snafu(display("failed to write publish target data {path}: {source}"))]
    WriteTargetData {
        path: String,
        source: std::io::Error,
    },
    #[snafu(display("failed to run ksbuilder publish command {bin}: {source}"))]
    RunKsbuilder { bin: String, source: std::io::Error },
    #[snafu(display(
        "ksbuilder publish failed with status {status}; stdout: {stdout}; stderr: {stderr}"
    ))]
    KsbuilderFailed {
        status: String,
        stdout: String,
        stderr: String,
    },
}

#[derive(Clone, Debug)]
struct PublisherConfig {
    fe_name: String,
    request_id: String,
    artifact_digest: String,
    artifact_configmap_namespace: String,
    artifact_configmap_name: String,
    artifact_configmap_key: String,
    target_kind: Option<String>,
    target_namespace: Option<String>,
    target_name: Option<String>,
    workdir: PathBuf,
    ksbuilder_bin: String,
    publish_args: Vec<String>,
}

impl PublisherConfig {
    fn from_env() -> Result<Self, Error> {
        let request_id = required_env("PUBLISH_REQUEST_ID")?;
        let workdir = env::var("PUBLISH_WORKDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                PathBuf::from(format!(
                    "/tmp/frontend-extension-publish-{}",
                    sha256_hex(request_id.as_bytes())
                ))
            });

        Ok(Self {
            fe_name: required_env("FE_NAME")?,
            request_id,
            artifact_digest: required_env("ARTIFACT_DIGEST")?,
            artifact_configmap_namespace: env::var("ARTIFACT_CONFIGMAP_NAMESPACE")
                .unwrap_or_else(|_| "extension-frontend-forge".to_string()),
            artifact_configmap_name: required_env("ARTIFACT_CONFIGMAP_NAME")?,
            artifact_configmap_key: env::var("ARTIFACT_CONFIGMAP_KEY")
                .unwrap_or_else(|_| "package.tgz".to_string()),
            target_kind: env::var("PUBLISH_TARGET_KIND")
                .ok()
                .filter(|v| !v.is_empty()),
            target_namespace: env::var("PUBLISH_TARGET_NAMESPACE")
                .ok()
                .filter(|v| !v.is_empty()),
            target_name: env::var("PUBLISH_TARGET_NAME")
                .ok()
                .filter(|v| !v.is_empty()),
            workdir,
            ksbuilder_bin: env::var("KSBUILDER_BIN").unwrap_or_else(|_| "ksbuilder".to_string()),
            publish_args: env::var("KSBUILDER_PUBLISH_ARGS")
                .ok()
                .map(|args| split_args(&args))
                .unwrap_or_default(),
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
                .unwrap_or_else(|_| "info,frontend_forge_extension_publisher=debug".into()),
        )
        .init();

    let cfg = PublisherConfig::from_env()?;
    let client = Client::try_default().await.context(KubeClientInitSnafu)?;

    let package = load_artifact_package(&client, &cfg).await?;
    verify_artifact_digest(&package, &cfg.artifact_digest)?;
    prepare_workdir(&cfg.workdir)?;
    unpack_package(&package, &cfg.workdir)?;
    let target_data = load_publish_target(&client, &cfg).await?;
    let target_env = write_publish_target_data(&cfg.workdir, target_data.as_ref())?;
    run_ksbuilder_publish(&cfg, target_data.as_ref(), target_env)?;

    info!(
        fe = %cfg.fe_name,
        request_id = %cfg.request_id,
        artifact_digest = %cfg.artifact_digest,
        workdir = %cfg.workdir.display(),
        "extension package published"
    );

    Ok(())
}

async fn load_artifact_package(client: &Client, cfg: &PublisherConfig) -> Result<Vec<u8>, Error> {
    let cm_api = Api::<ConfigMap>::namespaced(client.clone(), &cfg.artifact_configmap_namespace);
    let cm = cm_api
        .get(&cfg.artifact_configmap_name)
        .await
        .with_context(|_| GetArtifactConfigMapSnafu {
            namespace: cfg.artifact_configmap_namespace.clone(),
            name: cfg.artifact_configmap_name.clone(),
        })?;

    cm.binary_data
        .as_ref()
        .and_then(|data| data.get(&cfg.artifact_configmap_key))
        .map(|bytes| bytes.0.clone())
        .ok_or_else(|| Error::MissingArtifactPackage {
            namespace: cfg.artifact_configmap_namespace.clone(),
            name: cfg.artifact_configmap_name.clone(),
            key: cfg.artifact_configmap_key.clone(),
        })
}

fn verify_artifact_digest(package: &[u8], expected: &str) -> Result<(), Error> {
    let observed = sha256_hex(package);
    if expected == format!("sha256:{observed}") {
        Ok(())
    } else {
        Err(Error::ArtifactDigestMismatch {
            expected: expected.to_string(),
            observed,
        })
    }
}

fn prepare_workdir(workdir: &Path) -> Result<(), Error> {
    if workdir.exists() {
        fs::remove_dir_all(workdir).with_context(|_| PrepareWorkdirSnafu {
            path: workdir.display().to_string(),
        })?;
    }
    fs::create_dir_all(workdir).with_context(|_| PrepareWorkdirSnafu {
        path: workdir.display().to_string(),
    })
}

fn unpack_package(package: &[u8], workdir: &Path) -> Result<(), Error> {
    let decoder = GzDecoder::new(Cursor::new(package));
    let mut archive = Archive::new(decoder);
    let entries = archive.entries().with_context(|_| UnpackPackageSnafu {
        path: workdir.display().to_string(),
    })?;
    for entry in entries {
        let mut entry = entry.with_context(|_| UnpackPackageSnafu {
            path: workdir.display().to_string(),
        })?;
        entry
            .unpack_in(workdir)
            .with_context(|_| UnpackPackageSnafu {
                path: workdir.display().to_string(),
            })?;
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct TargetData {
    values: BTreeMap<String, Vec<u8>>,
}

async fn load_publish_target(
    client: &Client,
    cfg: &PublisherConfig,
) -> Result<Option<TargetData>, Error> {
    let Some(name) = cfg.target_name.as_ref() else {
        return Ok(None);
    };
    let namespace = cfg
        .target_namespace
        .clone()
        .unwrap_or_else(|| "extension-frontend-forge".to_string());
    let kind = cfg
        .target_kind
        .clone()
        .unwrap_or_else(|| "ConfigMap".to_string());

    match kind.as_str() {
        "ConfigMap" => {
            let api = Api::<ConfigMap>::namespaced(client.clone(), &namespace);
            let cm = api
                .get(name)
                .await
                .with_context(|_| GetTargetConfigMapSnafu {
                    namespace: namespace.clone(),
                    name: name.clone(),
                })?;
            let values = cm
                .data
                .unwrap_or_default()
                .into_iter()
                .map(|(key, value)| (key, value.into_bytes()))
                .chain(
                    cm.binary_data
                        .unwrap_or_default()
                        .into_iter()
                        .map(|(key, value)| (key, value.0)),
                )
                .collect();
            Ok(Some(TargetData { values }))
        }
        "Secret" => {
            let api = Api::<Secret>::namespaced(client.clone(), &namespace);
            let secret = api.get(name).await.with_context(|_| GetTargetSecretSnafu {
                namespace: namespace.clone(),
                name: name.clone(),
            })?;
            let values = secret
                .data
                .unwrap_or_default()
                .into_iter()
                .map(|(key, ByteString(value))| (key, value))
                .collect();
            Ok(Some(TargetData { values }))
        }
        _ => Err(Error::UnsupportedTargetKind { kind }),
    }
}

fn write_publish_target_data(
    workdir: &Path,
    target: Option<&TargetData>,
) -> Result<BTreeMap<String, OsString>, Error> {
    let mut envs = BTreeMap::new();
    let Some(target) = target else {
        return Ok(envs);
    };

    let target_dir = workdir.join(".frontend-forge-publish-target");
    fs::create_dir_all(&target_dir).with_context(|_| WriteTargetDataSnafu {
        path: target_dir.display().to_string(),
    })?;
    envs.insert(
        "FRONTEND_FORGE_PUBLISH_TARGET_DIR".to_string(),
        target_dir.as_os_str().to_os_string(),
    );

    for (key, value) in &target.values {
        if let Some(env_name) = key.strip_prefix("env.") {
            envs.insert(
                env_name.to_string(),
                OsString::from(String::from_utf8_lossy(value).into_owned()),
            );
            continue;
        }

        if key == "args" {
            continue;
        }

        let path = safe_child_path(&target_dir, key)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|_| WriteTargetDataSnafu {
                path: parent.display().to_string(),
            })?;
        }
        fs::write(&path, value).with_context(|_| WriteTargetDataSnafu {
            path: path.display().to_string(),
        })?;
    }

    Ok(envs)
}

fn safe_child_path(root: &Path, raw: &str) -> Result<PathBuf, Error> {
    let path = Path::new(raw);
    if path.is_absolute() {
        return Err(Error::UnsafeTargetDataPath {
            path: raw.to_string(),
        });
    }

    let mut out = PathBuf::from(root);
    for component in path.components() {
        match component {
            Component::Normal(part) => out.push(part),
            _ => {
                return Err(Error::UnsafeTargetDataPath {
                    path: raw.to_string(),
                });
            }
        }
    }

    Ok(out)
}

fn run_ksbuilder_publish(
    cfg: &PublisherConfig,
    target: Option<&TargetData>,
    target_env: BTreeMap<String, OsString>,
) -> Result<(), Error> {
    let mut args = cfg.publish_args.clone();
    if let Some(target) = target
        && let Some(target_args) = target.values.get("args")
        && let Ok(raw) = std::str::from_utf8(target_args)
    {
        args.extend(split_args(raw));
    }

    let output = Command::new(&cfg.ksbuilder_bin)
        .arg("publish")
        .args(args)
        .current_dir(&cfg.workdir)
        .envs(target_env)
        .output()
        .with_context(|_| RunKsbuilderSnafu {
            bin: cfg.ksbuilder_bin.clone(),
        })?;

    if output.status.success() {
        Ok(())
    } else {
        Err(Error::KsbuilderFailed {
            status: output
                .status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "signal".to_string()),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

fn split_args(raw: &str) -> Vec<String> {
    raw.split_whitespace()
        .filter(|arg| !arg.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifies_matching_artifact_digest() {
        let package = b"package";
        let digest = format!("sha256:{}", sha256_hex(package));

        assert!(verify_artifact_digest(package, &digest).is_ok());
    }

    #[test]
    fn rejects_unsafe_target_data_paths() {
        let root = Path::new("/tmp/frontend-forge-publish-test");

        assert!(safe_child_path(root, "../secret").is_err());
        assert!(safe_child_path(root, "/secret").is_err());
        assert!(safe_child_path(root, "config.yaml").is_ok());
    }
}
