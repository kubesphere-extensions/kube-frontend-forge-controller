use super::*;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub(crate) enum Error {
    #[snafu(display("failed to hash FrontendExtension package source: {source}"))]
    FrontendExtensionSourceHash { source: ExtensionPackageError },
    #[snafu(display("failed to compute FrontendExtension artifact key: {source}"))]
    FrontendExtensionArtifactKey {
        source: frontend_forge_common::CommonError,
    },
    #[snafu(display("failed to initialize Kubernetes client: {source}"))]
    KubeClientInit {
        #[snafu(source(from(kube::Error, Box::new)))]
        source: Box<kube::Error>,
    },
    #[snafu(display("failed to patch FrontendExtension status {name}: {source}"))]
    PatchFrontendExtensionStatus {
        name: String,
        #[snafu(source(from(kube::Error, Box::new)))]
        source: Box<kube::Error>,
    },
    #[snafu(display("failed to patch FrontendExtension status labels {name}: {source}"))]
    PatchFrontendExtensionStatusLabels {
        name: String,
        #[snafu(source(from(kube::Error, Box::new)))]
        source: Box<kube::Error>,
    },
    #[snafu(display("failed to serialize FrontendExtension status patch for {name}: {source}"))]
    SerializeFrontendExtensionStatusPatch {
        name: String,
        source: serde_json::Error,
    },
    #[snafu(display("serialized FrontendExtension status patch for {name} was not a JSON object"))]
    InvalidFrontendExtensionStatusPatchShape { name: String },
    #[snafu(display(
        "failed to list package Jobs in {namespace} for FrontendExtension {fe_name} and \
         artifactKey {artifact_key}: {source}"
    ))]
    ListPackageJobsForArtifactKey {
        namespace: String,
        fe_name: String,
        artifact_key: String,
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
    #[snafu(display("failed to list artifact ConfigMaps in {namespace} for GC: {source}"))]
    ListArtifactConfigMapsForGc {
        namespace: String,
        #[snafu(source(from(kube::Error, Box::new)))]
        source: Box<kube::Error>,
    },
    #[snafu(display("failed to delete artifact ConfigMap {namespace}/{name}: {source}"))]
    DeleteArtifactConfigMap {
        namespace: String,
        name: String,
        #[snafu(source(from(kube::Error, Box::new)))]
        source: Box<kube::Error>,
    },
    #[snafu(transparent)]
    Job {
        source: frontend_forge_common::JobError,
    },
    #[snafu(display("failed to get publish Job {namespace}/{name}: {source}"))]
    GetPublishJob {
        namespace: String,
        name: String,
        #[snafu(source(from(kube::Error, Box::new)))]
        source: Box<kube::Error>,
    },
    #[snafu(display("failed to get unpublish Job {namespace}/{name}: {source}"))]
    GetUnpublishJob {
        namespace: String,
        name: String,
        #[snafu(source(from(kube::Error, Box::new)))]
        source: Box<kube::Error>,
    },
    #[snafu(display("failed to delete FrontendExtension {name}: {source}"))]
    DeleteFrontendExtension {
        name: String,
        #[snafu(source(from(kube::Error, Box::new)))]
        source: Box<kube::Error>,
    },
}
