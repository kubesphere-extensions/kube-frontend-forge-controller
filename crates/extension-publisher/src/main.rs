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
use tracing::info;

const DEFAULT_IN_CLUSTER_KUBECONFIG_PATH: &str = "/tmp/frontend-forge-kubeconfig/config";

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
    #[snafu(display("failed to write artifact package {path}: {source}"))]
    WriteArtifactPackage {
        path: String,
        source: std::io::Error,
    },
    #[snafu(display("failed to unpack artifact package into {path}: {source}"))]
    UnpackArtifactPackage {
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
    #[snafu(display("failed to write in-cluster kubeconfig {path}: {source}"))]
    WriteKubeconfig {
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
enum PublisherAction {
    Publish,
    Unpublish,
}

#[derive(Clone, Debug)]
struct PublisherConfig {
    action: PublisherAction,
    fe_name: String,
    request_id: String,
    artifact_digest: Option<String>,
    artifact_configmap_namespace: String,
    artifact_configmap_name: Option<String>,
    artifact_configmap_key: String,
    artifact_filename: String,
    unpublish_extension_name: Option<String>,
    target_kind: Option<String>,
    target_namespace: Option<String>,
    target_name: Option<String>,
    workdir: PathBuf,
    ksbuilder_bin: String,
    publish_args: Vec<String>,
}

impl PublisherConfig {
    fn from_env() -> Result<Self, Error> {
        let action = match env::var("PUBLISH_ACTION").ok().as_deref() {
            Some("unpublish") => PublisherAction::Unpublish,
            _ => PublisherAction::Publish,
        };
        let request_id = match action {
            PublisherAction::Publish => required_env("PUBLISH_REQUEST_ID")?,
            PublisherAction::Unpublish => required_env("UNPUBLISH_REQUEST_ID")?,
        };
        let workdir = env::var("PUBLISH_WORKDIR").map_or_else(
            |_| {
                PathBuf::from(format!(
                    "/tmp/frontend-extension-publish-{}",
                    sha256_hex(request_id.as_bytes())
                ))
            },
            PathBuf::from,
        );

        Ok(Self {
            action,
            fe_name: required_env("FE_NAME")?,
            request_id,
            artifact_digest: env::var("ARTIFACT_DIGEST").ok().filter(|v| !v.is_empty()),
            artifact_configmap_namespace: env::var("ARTIFACT_CONFIGMAP_NAMESPACE")
                .unwrap_or_else(|_| "extension-frontend-forge".to_string()),
            artifact_configmap_name: env::var("ARTIFACT_CONFIGMAP_NAME")
                .ok()
                .filter(|v| !v.is_empty()),
            artifact_configmap_key: env::var("ARTIFACT_CONFIGMAP_KEY")
                .unwrap_or_else(|_| "package.tgz".to_string()),
            artifact_filename: env::var("ARTIFACT_FILENAME")
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| "package.tgz".to_string()),
            unpublish_extension_name: env::var("UNPUBLISH_EXTENSION_NAME")
                .ok()
                .filter(|v| !v.is_empty()),
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

    prepare_workdir(&cfg.workdir)?;
    let target_data = load_publish_target(&client, &cfg).await?;
    let mut target_env = write_publish_target_data(&cfg.workdir, target_data.as_ref())?;
    for (key, value) in ensure_in_cluster_kubeconfig()? {
        target_env.entry(key).or_insert(value);
    }

    match cfg.action {
        PublisherAction::Publish => {
            let package = load_artifact_package(&client, &cfg).await?;
            let artifact_digest =
                cfg.artifact_digest
                    .as_ref()
                    .ok_or_else(|| Error::MissingEnv {
                        key: "ARTIFACT_DIGEST",
                        source: std::env::VarError::NotPresent,
                    })?;
            verify_artifact_digest(&package, artifact_digest)?;
            let package_path =
                write_artifact_package(&package, &cfg.workdir, &cfg.artifact_filename)?;
            let package_dir = unpack_artifact_package(&package_path, &cfg.workdir)?;
            run_ksbuilder_publish(&cfg, &package_dir, target_data.as_ref(), target_env)?;

            info!(
                fe = %cfg.fe_name,
                request_id = %cfg.request_id,
                artifact_digest = %artifact_digest,
                workdir = %cfg.workdir.display(),
                "extension package published"
            );
        }
        PublisherAction::Unpublish => {
            let extension_name =
                cfg.unpublish_extension_name
                    .as_ref()
                    .ok_or_else(|| Error::MissingEnv {
                        key: "UNPUBLISH_EXTENSION_NAME",
                        source: std::env::VarError::NotPresent,
                    })?;
            run_ksbuilder_unpublish(&cfg, extension_name, target_data.as_ref(), target_env)?;

            info!(
                fe = %cfg.fe_name,
                request_id = %cfg.request_id,
                extension_name = %extension_name,
                workdir = %cfg.workdir.display(),
                "extension package unpublished"
            );
        }
    }

    Ok(())
}

async fn load_artifact_package(client: &Client, cfg: &PublisherConfig) -> Result<Vec<u8>, Error> {
    let artifact_configmap_name =
        cfg.artifact_configmap_name
            .as_ref()
            .ok_or_else(|| Error::MissingEnv {
                key: "ARTIFACT_CONFIGMAP_NAME",
                source: std::env::VarError::NotPresent,
            })?;
    let cm_api = Api::<ConfigMap>::namespaced(client.clone(), &cfg.artifact_configmap_namespace);
    let cm = cm_api
        .get(artifact_configmap_name)
        .await
        .with_context(|_| GetArtifactConfigMapSnafu {
            namespace: cfg.artifact_configmap_namespace.clone(),
            name: artifact_configmap_name.clone(),
        })?;

    cm.binary_data
        .as_ref()
        .and_then(|data| data.get(&cfg.artifact_configmap_key))
        .map(|bytes| bytes.0.clone())
        .ok_or_else(|| Error::MissingArtifactPackage {
            namespace: cfg.artifact_configmap_namespace.clone(),
            name: artifact_configmap_name.clone(),
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

fn write_artifact_package(
    package: &[u8],
    workdir: &Path,
    filename: &str,
) -> Result<PathBuf, Error> {
    let path = safe_child_path(workdir, filename)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|_| WriteArtifactPackageSnafu {
            path: parent.display().to_string(),
        })?;
    }
    fs::write(&path, package).with_context(|_| WriteArtifactPackageSnafu {
        path: path.display().to_string(),
    })?;
    Ok(path)
}

fn unpack_artifact_package(package_path: &Path, workdir: &Path) -> Result<PathBuf, Error> {
    let package_dir = workdir.join("package");
    if package_dir.exists() {
        fs::remove_dir_all(&package_dir).with_context(|_| UnpackArtifactPackageSnafu {
            path: package_dir.display().to_string(),
        })?;
    }
    fs::create_dir_all(&package_dir).with_context(|_| UnpackArtifactPackageSnafu {
        path: package_dir.display().to_string(),
    })?;

    let package = fs::read(package_path).with_context(|_| UnpackArtifactPackageSnafu {
        path: package_path.display().to_string(),
    })?;
    let decoder = GzDecoder::new(Cursor::new(package));
    let mut archive = tar::Archive::new(decoder);
    for entry in archive
        .entries()
        .with_context(|_| UnpackArtifactPackageSnafu {
            path: package_path.display().to_string(),
        })?
    {
        let mut entry = entry.with_context(|_| UnpackArtifactPackageSnafu {
            path: package_path.display().to_string(),
        })?;
        let relative_path = entry.path().with_context(|_| UnpackArtifactPackageSnafu {
            path: package_path.display().to_string(),
        })?;
        let output_path = safe_child_path(&package_dir, relative_path.as_ref())?;

        if entry.header().entry_type().is_dir() {
            fs::create_dir_all(&output_path).with_context(|_| UnpackArtifactPackageSnafu {
                path: output_path.display().to_string(),
            })?;
            continue;
        }

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).with_context(|_| UnpackArtifactPackageSnafu {
                path: parent.display().to_string(),
            })?;
        }
        let mut file =
            fs::File::create(&output_path).with_context(|_| UnpackArtifactPackageSnafu {
                path: output_path.display().to_string(),
            })?;
        std::io::copy(&mut entry, &mut file).with_context(|_| UnpackArtifactPackageSnafu {
            path: output_path.display().to_string(),
        })?;
    }

    Ok(package_dir)
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

fn ensure_in_cluster_kubeconfig() -> Result<BTreeMap<String, OsString>, Error> {
    let path = configured_kubeconfig_path(
        env::var("KSBUILDER_KUBECONFIG_PATH").ok(),
        env::var("KUBECONFIG").ok(),
    );
    if path.exists() {
        return Ok(kubeconfig_env(&path));
    }

    let service_host = env::var("KUBERNETES_SERVICE_HOST")
        .unwrap_or_else(|_| "kubernetes.default.svc".to_string());
    let service_port = env::var("KUBERNETES_SERVICE_PORT").unwrap_or_else(|_| "443".to_string());
    let config = in_cluster_kubeconfig(&service_host, &service_port);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|_| WriteKubeconfigSnafu {
            path: parent.display().to_string(),
        })?;
    }
    fs::write(&path, config).with_context(|_| WriteKubeconfigSnafu {
        path: path.display().to_string(),
    })?;
    Ok(kubeconfig_env(&path))
}

fn configured_kubeconfig_path(
    ksbuilder_kubeconfig_path: Option<String>,
    kubeconfig: Option<String>,
) -> PathBuf {
    ksbuilder_kubeconfig_path
        .filter(|value| !value.is_empty())
        .or_else(|| kubeconfig.filter(|value| !value.is_empty()))
        .map_or_else(
            || PathBuf::from(DEFAULT_IN_CLUSTER_KUBECONFIG_PATH),
            PathBuf::from,
        )
}

fn kubeconfig_env(path: &Path) -> BTreeMap<String, OsString> {
    BTreeMap::from([("KUBECONFIG".to_string(), path.as_os_str().to_os_string())])
}

fn in_cluster_kubeconfig(service_host: &str, service_port: &str) -> String {
    format!(
        r#"apiVersion: v1
kind: Config
clusters:
- name: in-cluster
  cluster:
    certificate-authority: /var/run/secrets/kubernetes.io/serviceaccount/ca.crt
    server: https://{service_host}:{service_port}
contexts:
- name: in-cluster
  context:
    cluster: in-cluster
    user: sa
current-context: in-cluster
users:
- name: sa
  user:
    tokenFile: /var/run/secrets/kubernetes.io/serviceaccount/token
"#
    )
}

fn safe_child_path(root: &Path, raw: impl AsRef<Path>) -> Result<PathBuf, Error> {
    let raw = raw.as_ref();
    if raw.as_os_str().is_empty() {
        return Err(Error::UnsafeTargetDataPath {
            path: raw.display().to_string(),
        });
    }

    if raw.is_absolute() {
        return Err(Error::UnsafeTargetDataPath {
            path: raw.display().to_string(),
        });
    }

    let mut out = PathBuf::from(root);
    for component in raw.components() {
        match component {
            Component::Normal(part) => out.push(part),
            _ => {
                return Err(Error::UnsafeTargetDataPath {
                    path: raw.display().to_string(),
                });
            }
        }
    }

    Ok(out)
}

fn run_ksbuilder_publish(
    cfg: &PublisherConfig,
    package_dir: &Path,
    target: Option<&TargetData>,
    target_env: BTreeMap<String, OsString>,
) -> Result<(), Error> {
    let args = ksbuilder_publish_args(cfg, package_dir, target);

    let output = Command::new(&cfg.ksbuilder_bin)
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
                .map_or_else(|| "signal".to_string(), |code| code.to_string()),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

fn run_ksbuilder_unpublish(
    cfg: &PublisherConfig,
    extension_name: &str,
    target: Option<&TargetData>,
    target_env: BTreeMap<String, OsString>,
) -> Result<(), Error> {
    let args = ksbuilder_unpublish_args(cfg, extension_name, target);

    let output = Command::new(&cfg.ksbuilder_bin)
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
                .map_or_else(|| "signal".to_string(), |code| code.to_string()),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

fn ksbuilder_publish_args(
    cfg: &PublisherConfig,
    package_dir: &Path,
    target: Option<&TargetData>,
) -> Vec<String> {
    let mut args = vec!["publish".to_string(), package_dir.display().to_string()];
    args.extend(cfg.publish_args.clone());
    if let Some(target) = target
        && let Some(target_args) = target.values.get("args")
        && let Ok(raw) = std::str::from_utf8(target_args)
    {
        args.extend(split_args(raw));
    }
    args
}

fn ksbuilder_unpublish_args(
    cfg: &PublisherConfig,
    extension_name: &str,
    target: Option<&TargetData>,
) -> Vec<String> {
    let mut args = vec!["unpublish".to_string(), extension_name.to_string()];
    args.extend(cfg.publish_args.clone());
    if let Some(target) = target
        && let Some(target_args) = target.values.get("args")
        && let Ok(raw) = std::str::from_utf8(target_args)
    {
        args.extend(split_args(raw));
    }
    args
}

fn split_args(raw: &str) -> Vec<String> {
    raw.split_whitespace()
        .filter(|arg| !arg.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use flate2::{Compression, write::GzEncoder};

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

        assert!(safe_child_path(root, "").is_err());
        assert!(safe_child_path(root, "../secret").is_err());
        assert!(safe_child_path(root, "/secret").is_err());
        assert!(safe_child_path(root, "config.yaml").is_ok());
    }

    #[test]
    fn writes_artifact_package_to_safe_filename() {
        let root = env::temp_dir().join(format!(
            "frontend-forge-publisher-test-{}",
            sha256_hex(b"writes_artifact_package_to_safe_filename")
        ));
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        fs::create_dir_all(&root).unwrap();

        let path = write_artifact_package(b"package", &root, "inspecttask-0.1.0.tgz").unwrap();

        assert_eq!(path, root.join("inspecttask-0.1.0.tgz"));
        assert_eq!(fs::read(path).unwrap(), b"package");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_unsafe_artifact_filenames() {
        let root = Path::new("/tmp/frontend-forge-publish-test");

        assert!(write_artifact_package(b"package", root, "").is_err());
        assert!(write_artifact_package(b"package", root, "../package.tgz").is_err());
        assert!(write_artifact_package(b"package", root, "/package.tgz").is_err());
    }

    #[test]
    fn unpacks_artifact_package_to_directory() {
        let root = env::temp_dir().join(format!(
            "frontend-forge-publisher-test-{}",
            sha256_hex(b"unpacks_artifact_package_to_directory")
        ));
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        fs::create_dir_all(&root).unwrap();

        let package = test_tgz(&[
            ("extension.yaml", b"name: inspecttask\n".as_slice()),
            ("static/favicon.svg", b"<svg />".as_slice()),
        ]);
        let package_path =
            write_artifact_package(&package, &root, "inspecttask-0.1.0.tgz").unwrap();
        let package_dir = unpack_artifact_package(&package_path, &root).unwrap();

        assert_eq!(package_dir, root.join("package"));
        assert_eq!(
            fs::read_to_string(package_dir.join("extension.yaml")).unwrap(),
            "name: inspecttask\n"
        );
        assert_eq!(
            fs::read_to_string(package_dir.join("static/favicon.svg")).unwrap(),
            "<svg />"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn builds_ksbuilder_publish_args_with_package_directory_first() {
        let cfg = PublisherConfig {
            action: PublisherAction::Publish,
            fe_name: "inspecttask".to_string(),
            request_id: "request-1".to_string(),
            artifact_digest: Some("sha256:artifact".to_string()),
            artifact_configmap_namespace: "extension-frontend-forge".to_string(),
            artifact_configmap_name: Some("fe-inspecttask-a1b2c3d4".to_string()),
            artifact_configmap_key: "package.tgz".to_string(),
            artifact_filename: "inspecttask-0.1.0.tgz".to_string(),
            unpublish_extension_name: None,
            target_kind: None,
            target_namespace: None,
            target_name: None,
            workdir: PathBuf::from("/tmp/frontend-forge-publish-test"),
            ksbuilder_bin: "ksbuilder".to_string(),
            publish_args: vec!["--kubeconfig".to_string(), "kubeconfig".to_string()],
        };
        let target = TargetData {
            values: BTreeMap::from([(
                "args".to_string(),
                b"--token token-value --endpoint https://example.test".to_vec(),
            )]),
        };

        assert_eq!(
            ksbuilder_publish_args(&cfg, Path::new("/work/package"), Some(&target)),
            vec![
                "publish",
                "/work/package",
                "--kubeconfig",
                "kubeconfig",
                "--token",
                "token-value",
                "--endpoint",
                "https://example.test",
            ]
        );
    }

    #[test]
    fn builds_ksbuilder_unpublish_args_with_extension_name_first() {
        let cfg = PublisherConfig {
            action: PublisherAction::Unpublish,
            fe_name: "inspecttask".to_string(),
            request_id: "request-1".to_string(),
            artifact_digest: None,
            artifact_configmap_namespace: "extension-frontend-forge".to_string(),
            artifact_configmap_name: None,
            artifact_configmap_key: "package.tgz".to_string(),
            artifact_filename: "package.tgz".to_string(),
            unpublish_extension_name: Some("inspecttask".to_string()),
            target_kind: None,
            target_namespace: None,
            target_name: None,
            workdir: PathBuf::from("/tmp/frontend-forge-publish-test"),
            ksbuilder_bin: "ksbuilder".to_string(),
            publish_args: vec!["--kubeconfig".to_string(), "kubeconfig".to_string()],
        };
        let target = TargetData {
            values: BTreeMap::from([(
                "args".to_string(),
                b"--token token-value --endpoint https://example.test".to_vec(),
            )]),
        };

        assert_eq!(
            ksbuilder_unpublish_args(&cfg, "inspecttask", Some(&target)),
            vec![
                "unpublish",
                "inspecttask",
                "--kubeconfig",
                "kubeconfig",
                "--token",
                "token-value",
                "--endpoint",
                "https://example.test",
            ]
        );
    }

    #[test]
    fn builds_in_cluster_kubeconfig() {
        let config = in_cluster_kubeconfig("10.96.0.1", "443");

        assert!(config.contains("server: https://10.96.0.1:443"));
        assert!(config.contains(
            "certificate-authority: /var/run/secrets/kubernetes.io/serviceaccount/ca.crt"
        ));
        assert!(config.contains("tokenFile: /var/run/secrets/kubernetes.io/serviceaccount/token"));
    }

    #[test]
    fn defaults_in_cluster_kubeconfig_path_to_tmp() {
        assert_eq!(
            configured_kubeconfig_path(None, None),
            PathBuf::from(DEFAULT_IN_CLUSTER_KUBECONFIG_PATH)
        );
    }

    #[test]
    fn configured_kubeconfig_path_prefers_ksbuilder_override() {
        assert_eq!(
            configured_kubeconfig_path(
                Some("/custom/ksbuilder/config".to_string()),
                Some("/custom/kube/config".to_string())
            ),
            PathBuf::from("/custom/ksbuilder/config")
        );
    }

    #[test]
    fn configured_kubeconfig_path_uses_kubeconfig_when_override_is_empty() {
        assert_eq!(
            configured_kubeconfig_path(
                Some(String::new()),
                Some("/custom/kube/config".to_string())
            ),
            PathBuf::from("/custom/kube/config")
        );
    }

    fn test_tgz(files: &[(&str, &[u8])]) -> Vec<u8> {
        let encoder = GzEncoder::new(Vec::new(), Compression::default());
        let mut archive = tar::Builder::new(encoder);
        for (path, content) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            archive
                .append_data(&mut header, *path, *content)
                .expect("append test tar entry");
        }
        let encoder = archive.into_inner().expect("finish test tar");
        encoder.finish().expect("finish gzip")
    }
}
