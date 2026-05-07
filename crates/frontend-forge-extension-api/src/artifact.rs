use super::*;

pub(crate) async fn artifact_bytes(
    state: &AppState,
    artifact: &ExtensionArtifactStatus,
) -> Result<Vec<u8>, ApiError> {
    let key = if artifact.storage.key.is_empty() {
        PACKAGE_KEY
    } else {
        artifact.storage.key.as_str()
    };
    let cm_api =
        Api::<ConfigMap>::namespaced(state.client.clone(), &artifact.storage.ref_.namespace);
    let cm = cm_api
        .get(&artifact.storage.ref_.name)
        .await
        .map_err(|err| ApiError::kube("failed to get artifact ConfigMap", &err))?;
    cm.binary_data
        .as_ref()
        .and_then(|data| data.get(key))
        .map(|bytes| bytes.0.clone())
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("artifact ConfigMap is missing binaryData key {key}"),
            )
        })
}

pub(crate) fn verify_artifact_digest(bytes: &[u8], expected: &str) -> Result<(), ApiError> {
    let observed = format!("sha256:{}", sha256_hex(bytes));
    if observed == expected {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "artifact digest mismatch",
        ))
    }
}
