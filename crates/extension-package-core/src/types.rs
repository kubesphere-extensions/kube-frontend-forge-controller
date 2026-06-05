use super::*;

pub const PACKAGE_KEY: &str = "package.tgz";
pub const PACKAGE_MEDIA_TYPE: &str = "application/gzip";
pub const ARTIFACT_METADATA_KEY: &str = "artifact.json";
pub const FILES_METADATA_KEY: &str = "files.json";

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
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
    #[snafu(display("failed to build package archive: {source}"))]
    Archive { source: std::io::Error },
    #[snafu(display("package template file {path} is missing"))]
    TemplateMissing { path: &'static str },
    #[snafu(display("package template file {path} is not valid UTF-8"))]
    TemplateUtf8 { path: &'static str },
    #[snafu(display("failed to render package template {path}: {source}"))]
    RenderTemplate {
        path: &'static str,
        source: minijinja::Error,
    },
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
