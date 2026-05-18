use super::*;

#[derive(Debug, Serialize)]
pub(crate) struct FrontendExtensionListResponse {
    pub(crate) items: Vec<FrontendExtensionSummary>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct FrontendExtensionListQuery {
    #[serde(default, rename = "labelSelector")]
    pub(crate) label_selector: Option<String>,
}

impl FrontendExtensionListQuery {
    pub(crate) fn list_params(&self) -> ListParams {
        match self.label_selector.as_deref().map(str::trim) {
            Some(selector) if !selector.is_empty() => ListParams::default().labels(selector),
            _ => ListParams::default(),
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct FrontendExtensionSummary {
    pub(crate) name: String,
    pub(crate) generation: Option<i64>,
    pub(crate) package: FrontendExtensionPackageSummary,
    pub(crate) phase: FrontendExtensionPhase,
    #[serde(skip_serializing_if = "Option::is_none", rename = "artifactDigest")]
    pub(crate) artifact_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) download: Option<DownloadSummary>,
    pub(crate) publish: PublishStatus,
}

#[derive(Debug, Serialize)]
pub(crate) struct FrontendExtensionPackageSummary {
    pub(crate) version: String,
    #[serde(rename = "displayName")]
    pub(crate) display_name: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct DownloadSummary {
    pub(crate) ready: bool,
    pub(crate) filename: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PublishRequest {
    #[serde(default, rename = "requestId")]
    pub(crate) request_id: Option<String>,
    #[serde(default, rename = "expectedArtifactDigest")]
    pub(crate) expected_artifact_digest: Option<String>,
}

#[derive(Debug)]
pub(crate) struct ResolvedPublishRequest {
    pub(crate) request_id: String,
    pub(crate) artifact_digest: Option<String>,
    pub(crate) generation: Option<i64>,
    pub(crate) source_hash: String,
    pub(crate) target_ref: NamespacedResourceRef,
    pub(crate) target_kind: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UnpublishRequest {
    #[serde(default, rename = "requestId")]
    pub(crate) request_id: Option<String>,
}

#[derive(Debug)]
pub(crate) struct ResolvedUnpublishRequest {
    pub(crate) request_id: String,
    pub(crate) extension_name: String,
    pub(crate) target_ref: NamespacedResourceRef,
    pub(crate) target_kind: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeleteRequest {
    #[serde(default)]
    pub(crate) unpublish: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct DeleteResponse {
    pub(crate) deleted: bool,
    #[serde(rename = "unpublishSkipped")]
    pub(crate) unpublish_skipped: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) unpublish: Option<UnpublishStatus>,
}
