use std::{collections::BTreeMap, io::Write};

use chrono::{DateTime, Utc};
use flate2::{Compression, GzBuilder};
use frontend_forge_api::{
    ExtensionDependencySpec, ExtensionMaintainerSpec, ExtensionProviderSpec,
    ExtensionResourcesSpec, FrontendExtension, FrontendExtensionPackageSpec,
    FrontendExtensionSourceSpec,
};
use frontend_forge_common::{
    CommonError, manifest_content_and_hash, serializable_hash, sha256_hex,
};
use frontend_forge_manifest::{ManifestRenderError, render_frontend_extension_manifest};
use kube::ResourceExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use snafu::{ResultExt, Snafu};
use tar::{Builder as TarBuilder, Header};

pub const PACKAGE_KEY: &str = "package.tgz";
pub const PACKAGE_MEDIA_TYPE: &str = "application/gzip";
pub const ARTIFACT_METADATA_KEY: &str = "artifact.json";
pub const FILES_METADATA_KEY: &str = "files.json";

#[derive(Debug, Snafu)]
pub enum ExtensionPackageError {
    #[snafu(display("failed to hash FrontendExtension source identity: {source}"))]
    SourceHash { source: CommonError },
    #[snafu(display("failed to render frontend manifest: {source}"))]
    RenderManifest { source: ManifestRenderError },
    #[snafu(display("failed to serialize {name}: {source}"))]
    Serialize {
        name: &'static str,
        source: serde_yaml::Error,
    },
    #[snafu(display("failed to serialize {name}: {source}"))]
    SerializeJson {
        name: &'static str,
        source: serde_json::Error,
    },
    #[snafu(display("failed to hash package payload: {source}"))]
    PackageHash { source: CommonError },
    #[snafu(display("failed to build package archive: {source}"))]
    Archive { source: std::io::Error },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageFile {
    pub path: String,
    pub content: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageFileMeta {
    pub path: String,
    #[serde(rename = "sizeBytes")]
    pub size_bytes: usize,
    pub digest: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageArtifactMetadata {
    pub name: String,
    pub version: String,
    pub filename: String,
    #[serde(rename = "mediaType")]
    pub media_type: String,
    pub digest: String,
    #[serde(rename = "sizeBytes")]
    pub size_bytes: usize,
    #[serde(rename = "sourceHash")]
    pub source_hash: String,
    #[serde(rename = "generatedAt")]
    pub generated_at: DateTime<Utc>,
    pub files: Vec<PackageFileMeta>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigMapArtifactPayload {
    pub data: BTreeMap<String, String>,
    pub binary_data: BTreeMap<String, Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionPackageArtifact {
    pub filename: String,
    pub media_type: String,
    pub digest: String,
    pub size_bytes: usize,
    pub source_hash: String,
    pub generated_at: DateTime<Utc>,
    pub files: Vec<PackageFile>,
    pub payload: ConfigMapArtifactPayload,
}

#[derive(Serialize)]
struct SourceIdentity<'a> {
    package: &'a FrontendExtensionPackageSpec,
    source: &'a FrontendExtensionSourceSpec,
}

pub fn build_extension_package(
    fe: &FrontendExtension,
    generated_at: DateTime<Utc>,
) -> Result<ExtensionPackageArtifact, ExtensionPackageError> {
    let source_hash = frontend_extension_source_hash(fe)?;

    let mut files = package_files(fe)?;
    files.sort_by(|a, b| a.path.cmp(&b.path));

    let file_meta = package_file_meta(&files);
    let package_bytes = gzip_bytes(&tar_bytes(&files)?)?;
    let digest = format!("sha256:{}", sha256_hex(&package_bytes));
    let package_name = frontend_extension_package_name(fe);
    let filename = format!("{}-{}.tgz", package_name, fe.spec.package.version);

    let metadata = PackageArtifactMetadata {
        name: package_name,
        version: fe.spec.package.version.clone(),
        filename: filename.clone(),
        media_type: PACKAGE_MEDIA_TYPE.to_string(),
        digest: digest.clone(),
        size_bytes: package_bytes.len(),
        source_hash: source_hash.clone(),
        generated_at,
        files: file_meta,
    };

    let artifact_json = serde_json::to_string_pretty(&metadata).context(SerializeJsonSnafu {
        name: ARTIFACT_METADATA_KEY,
    })?;
    let files_json = serde_json::to_string_pretty(&metadata.files).context(SerializeJsonSnafu {
        name: FILES_METADATA_KEY,
    })?;

    Ok(ExtensionPackageArtifact {
        filename,
        media_type: PACKAGE_MEDIA_TYPE.to_string(),
        digest,
        size_bytes: package_bytes.len(),
        source_hash,
        generated_at,
        files,
        payload: ConfigMapArtifactPayload {
            data: BTreeMap::from([
                (ARTIFACT_METADATA_KEY.to_string(), artifact_json),
                (FILES_METADATA_KEY.to_string(), files_json),
            ]),
            binary_data: BTreeMap::from([(PACKAGE_KEY.to_string(), package_bytes)]),
        },
    })
}

fn package_files(fe: &FrontendExtension) -> Result<Vec<PackageFile>, ExtensionPackageError> {
    let manifest = render_frontend_extension_manifest(fe).context(RenderManifestSnafu)?;
    let manifest_content = canonical_json(&manifest)?;
    let package_metadata = package_metadata(fe);
    let charts_values = fe
        .spec
        .package
        .charts
        .as_ref()
        .map(|charts| charts.values.clone())
        .unwrap_or_default();
    let charts_values_path = format!("charts/{}/values.yaml", frontend_extension_package_name(fe));

    let mut files = vec![
        yaml_file("extension.yaml", &package_metadata)?,
        yaml_file(&charts_values_path, &charts_values)?,
        PackageFile {
            path: "frontend/manifest.json".to_string(),
            content: manifest_content.into_bytes(),
        },
    ];

    files.extend(extension_resource_files(fe, &manifest)?);

    Ok(files)
}

#[derive(Serialize)]
struct KsbuilderExtensionYaml {
    #[serde(rename = "apiVersion")]
    api_version: String,
    name: String,
    version: String,
    #[serde(rename = "displayName")]
    display_name: BTreeMap<String, String>,
    description: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    category: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    keywords: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    sources: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "kubeVersion")]
    kube_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "ksVersion")]
    ks_version: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    maintainers: Vec<ExtensionMaintainerSpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    home: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    provider: BTreeMap<String, ExtensionProviderSpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    dependencies: Vec<ExtensionDependencySpec>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "installationMode")]
    installation_mode: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    images: Vec<String>,
}

fn package_metadata(fe: &FrontendExtension) -> KsbuilderExtensionYaml {
    let package = &fe.spec.package;
    KsbuilderExtensionYaml {
        api_version: "kubesphere.io/v1alpha1".to_string(),
        name: frontend_extension_package_name(fe),
        version: package.version.clone(),
        display_name: package.display_name.clone(),
        description: package.description.clone(),
        category: package.category.clone(),
        keywords: package.keywords.clone(),
        sources: package.sources.clone(),
        kube_version: package.kube_version.clone(),
        ks_version: package.ks_version.clone(),
        maintainers: package.maintainers.clone(),
        home: package.home.clone(),
        provider: package.provider.clone(),
        icon: package.icon.clone(),
        dependencies: package.dependencies.clone(),
        installation_mode: package.installation_mode.clone(),
        images: package.images.clone(),
    }
}

#[must_use]
pub fn frontend_extension_package_name(fe: &FrontendExtension) -> String {
    fe.spec
        .package
        .name
        .clone()
        .unwrap_or_else(|| fe.name_any())
}

pub fn frontend_extension_source_hash(
    fe: &FrontendExtension,
) -> Result<String, ExtensionPackageError> {
    serializable_hash(&SourceIdentity {
        package: &fe.spec.package,
        source: &fe.spec.source,
    })
    .context(SourceHashSnafu)
}

fn extension_resource_files(
    fe: &FrontendExtension,
    manifest: &Value,
) -> Result<Vec<PackageFile>, ExtensionPackageError> {
    let resources = &fe.spec.source.inline.extension_resources;
    let mut files = Vec::new();

    if let Some(resources) = resources {
        if let Some(jsbundle) = resources.js_bundle.as_ref() {
            files.push(yaml_file(
                "resources/jsbundle.yaml",
                &json!({
                    "apiVersion": "extensions.kubesphere.io/v1alpha1",
                    "kind": "JSBundle",
                    "metadata": {
                        "name": jsbundle.name,
                        "annotations": {
                            "frontend-forge.io/frontend-manifest-path": "frontend/manifest.json"
                        }
                    },
                    "spec": {
                        "raw": canonical_json(manifest)?
                    }
                }),
            )?);
        }

        for role_template in &resources.role_templates {
            files.push(yaml_file(
                &format!("resources/roletemplates/{}.yaml", role_template.name),
                &json!({
                    "apiVersion": "iam.kubesphere.io/v1beta1",
                    "kind": "RoleTemplate",
                    "metadata": {
                        "name": role_template.name
                    },
                    "spec": {
                        "displayName": role_template.display_name,
                        "rules": role_template.rules
                    }
                }),
            )?);
        }
    }

    if resources_missing(resources.as_ref()) {
        files.push(yaml_file(
            "resources/extension-resources.yaml",
            &json!({
                "jsBundle": null,
                "roleTemplates": []
            }),
        )?);
    }

    Ok(files)
}

fn resources_missing(resources: Option<&ExtensionResourcesSpec>) -> bool {
    resources.is_none_or(|r| r.js_bundle.is_none() && r.role_templates.is_empty())
}

fn yaml_file<T>(path: &str, value: &T) -> Result<PackageFile, ExtensionPackageError>
where
    T: Serialize,
{
    let content = serde_yaml::to_string(value).context(SerializeSnafu { name: "yaml file" })?;
    Ok(PackageFile {
        path: path.to_string(),
        content: content.into_bytes(),
    })
}

fn canonical_json(value: &Value) -> Result<String, ExtensionPackageError> {
    let (content, _) = manifest_content_and_hash(value).context(PackageHashSnafu)?;
    Ok(content)
}

fn package_file_meta(files: &[PackageFile]) -> Vec<PackageFileMeta> {
    files
        .iter()
        .map(|file| PackageFileMeta {
            path: file.path.clone(),
            size_bytes: file.content.len(),
            digest: format!("sha256:{}", sha256_hex(&file.content)),
        })
        .collect()
}

fn tar_bytes(files: &[PackageFile]) -> Result<Vec<u8>, ExtensionPackageError> {
    let mut builder = TarBuilder::new(Vec::new());

    for file in files {
        append_tar_file(&mut builder, file)?;
    }

    builder.finish().context(ArchiveSnafu)?;
    builder.into_inner().context(ArchiveSnafu)
}

fn append_tar_file(
    builder: &mut TarBuilder<Vec<u8>>,
    file: &PackageFile,
) -> Result<(), ExtensionPackageError> {
    let mut header = Header::new_ustar();
    header.set_size(file.content.len() as u64);
    header.set_mode(0o644);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_cksum();

    builder
        .append_data(&mut header, file.path.as_str(), file.content.as_slice())
        .context(ArchiveSnafu)
}

fn gzip_bytes(input: &[u8]) -> Result<Vec<u8>, ExtensionPackageError> {
    let mut encoder = GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), Compression::default());
    encoder.write_all(input).context(ArchiveSnafu)?;
    encoder.finish().context(ArchiveSnafu)
}

#[cfg(test)]
mod tests {
    use frontend_forge_api::FrontendExtension;

    use super::*;

    fn sample_fe() -> FrontendExtension {
        serde_yaml::from_str(
            r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendExtension
metadata:
  name: fe-inspecttask
spec:
  package:
    name: inspecttask
    version: 0.1.0
    displayName:
      zh: 巡检任务
      en: Inspect Task
    description:
      zh: InspectTask extension package
      en: InspectTask extension package
    category: dev-tools
    keywords:
      - Frontend
    sources:
      - https://github.com/kubesphere-extensions/frontend-forge
    kubeVersion: ">=1.23.0-0"
    ksVersion: ">=4.2.1-0"
    maintainers:
      - name: KubeSphere
        email: kubesphere@yunify.com
    home: https://kubesphere.com.cn/
    provider:
      zh:
        name: 北京青云科技股份有限公司
        email: kubesphere@yunify.com
        url: https://kubesphere.com.cn/
      en:
        name: QingCloud Technologies
        email: kubesphere@yunify.com
        url: https://kubesphere.co/
    icon: ./static/frontend-forge.ico
    dependencies:
      - name: frontend
        tags:
          - extension
      - name: frontend-forge
        tags:
          - extension
    installationMode: HostOnly
    images:
      - kubesphere/frontend-forge-console:v1.0.0
      - kubesphere/frontend-forge-controller:v1.0.0
      - kubesphere/frontend-forge-runner:v1.0.0
    charts:
      values:
        replicaCount: 1
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
      extensionResources:
        jsBundle:
          name: inspecttask
        roleTemplates:
          - name: inspecttask-view
            displayName: InspectTask Viewer
            rules:
              - apiGroups: ["kubeeye.kubesphere.io"]
                resources: ["inspecttasks"]
                verbs: ["get", "list", "watch"]
"#,
        )
        .unwrap()
    }

    #[test]
    fn builds_extension_package_artifact_payload() {
        let generated_at = DateTime::from_timestamp(1_775_200_000, 0).unwrap();
        let artifact = build_extension_package(&sample_fe(), generated_at).unwrap();

        assert_eq!(artifact.filename, "inspecttask-0.1.0.tgz");
        assert_eq!(artifact.media_type, PACKAGE_MEDIA_TYPE);
        assert!(artifact.digest.starts_with("sha256:"));
        assert_eq!(artifact.generated_at, generated_at);
        assert!(artifact.payload.data.contains_key(ARTIFACT_METADATA_KEY));
        assert!(artifact.payload.data.contains_key(FILES_METADATA_KEY));
        assert_eq!(
            artifact.payload.binary_data[PACKAGE_KEY][..3],
            [0x1f, 0x8b, 0x08]
        );
        assert!(
            artifact
                .files
                .iter()
                .any(|file| file.path == "frontend/manifest.json")
        );
        assert!(
            artifact
                .files
                .iter()
                .any(|file| file.path == "charts/inspecttask/values.yaml")
        );
        assert!(
            !artifact
                .files
                .iter()
                .any(|file| file.path == "charts/values.yaml")
        );
        assert!(
            artifact
                .files
                .iter()
                .any(|file| file.path == "resources/jsbundle.yaml")
        );
        assert!(
            artifact
                .files
                .iter()
                .any(|file| file.path == "resources/roletemplates/inspecttask-view.yaml")
        );

        let extension_yaml = artifact
            .files
            .iter()
            .find(|file| file.path == "extension.yaml")
            .unwrap();
        let content = std::str::from_utf8(&extension_yaml.content).unwrap();

        assert!(content.contains("apiVersion: kubesphere.io/v1alpha1"));
        assert!(content.contains("name: inspecttask"));
        assert!(content.contains("displayName:"));
        assert!(content.contains("installationMode: HostOnly"));
        assert!(content.contains("kubesphere/frontend-forge-controller:v1.0.0"));
    }

    #[test]
    fn package_manifest_uses_frontend_extension_routes() {
        let generated_at = DateTime::from_timestamp(1_775_200_000, 0).unwrap();
        let artifact = build_extension_package(&sample_fe(), generated_at).unwrap();
        let manifest = artifact
            .files
            .iter()
            .find(|file| file.path == "frontend/manifest.json")
            .unwrap();
        let content = std::str::from_utf8(&manifest.content).unwrap();

        assert!(content.contains("/frontendextensions/inspecttask/inspecttasks"));
        assert!(!content.contains("/frontendintegrations/inspecttask/inspecttasks"));
    }

    #[test]
    fn source_hash_changes_with_package_content() {
        let generated_at = DateTime::from_timestamp(1_775_200_000, 0).unwrap();
        let a = sample_fe();
        let mut b = sample_fe();
        b.spec.package.version = "0.2.0".to_string();

        let a = build_extension_package(&a, generated_at).unwrap();
        let b = build_extension_package(&b, generated_at).unwrap();

        assert_ne!(a.source_hash, b.source_hash);
        assert_ne!(a.digest, b.digest);
    }

    #[test]
    fn gzip_payload_is_deterministic() {
        let generated_at = DateTime::from_timestamp(1_775_200_000, 0).unwrap();
        let a = build_extension_package(&sample_fe(), generated_at).unwrap();
        let b = build_extension_package(&sample_fe(), generated_at).unwrap();

        assert_eq!(a.digest, b.digest);
        assert_eq!(
            a.payload.binary_data[PACKAGE_KEY],
            b.payload.binary_data[PACKAGE_KEY]
        );
    }
}
