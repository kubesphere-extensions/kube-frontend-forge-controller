use super::*;

pub(crate) fn resolve_publish_request(
    fe: &FrontendExtension,
    request: &PublishRequest,
    artifact: Option<&ExtensionArtifactStatus>,
) -> Result<ResolvedPublishRequest, ApiError> {
    let target_ref = publish_target_ref(fe)
        .ok_or_else(|| ApiError::conflict("publish targetRef is required"))?;
    if target_ref.namespace.is_empty() || target_ref.name.is_empty() {
        return Err(ApiError::conflict(
            "publish targetRef namespace and name are required",
        ));
    }
    let target_kind = publish_target_kind(fe);
    if !matches!(target_kind.as_str(), "ConfigMap" | "Secret") {
        return Err(ApiError::conflict(
            "publish targetKind must be ConfigMap or Secret",
        ));
    }
    let source_hash = frontend_extension_source_hash(fe)
        .map_err(|err| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    let request_identity = artifact
        .map(|artifact| artifact.digest.as_str())
        .unwrap_or(source_hash.as_str());

    Ok(ResolvedPublishRequest {
        request_id: request
            .request_id
            .as_deref()
            .map(str::trim)
            .filter(|request_id| !request_id.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| generated_publish_request_id(request_identity)),
        artifact_digest: artifact.map(|artifact| artifact.digest.clone()),
        generation: fe.metadata.generation,
        source_hash,
        target_ref,
        target_kind,
    })
}

pub(crate) fn resolve_unpublish_request(
    fe: &FrontendExtension,
    request: &UnpublishRequest,
) -> Result<ResolvedUnpublishRequest, ApiError> {
    let target_ref = publish_target_ref(fe)
        .ok_or_else(|| ApiError::conflict("publish targetRef is required"))?;
    if target_ref.namespace.is_empty() || target_ref.name.is_empty() {
        return Err(ApiError::conflict(
            "publish targetRef namespace and name are required",
        ));
    }
    let target_kind = publish_target_kind(fe);
    if !matches!(target_kind.as_str(), "ConfigMap" | "Secret") {
        return Err(ApiError::conflict(
            "publish targetKind must be ConfigMap or Secret",
        ));
    }

    let extension_name = frontend_extension_package_name(fe);
    Ok(ResolvedUnpublishRequest {
        request_id: request
            .request_id
            .as_deref()
            .map(str::trim)
            .filter(|request_id| !request_id.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| generated_unpublish_request_id(&extension_name)),
        extension_name,
        target_ref,
        target_kind,
    })
}

pub(crate) fn publish_target_ref(fe: &FrontendExtension) -> Option<NamespacedResourceRef> {
    let annos = fe.metadata.annotations.as_ref();
    let target_name = annos
        .and_then(|annos| annos.get(ANNO_PUBLISH_TARGET_NAME))
        .filter(|name| !name.is_empty());
    if let Some(name) = target_name {
        return Some(NamespacedResourceRef {
            namespace: annos
                .and_then(|annos| annos.get(ANNO_PUBLISH_TARGET_NAMESPACE))
                .filter(|namespace| !namespace.is_empty())
                .cloned()
                .unwrap_or_else(|| "extension-frontend-forge".to_string()),
            name: name.clone(),
            uid: None,
        });
    }

    fe.spec
        .publish_policy
        .as_ref()
        .and_then(|policy| policy.default_target_ref.clone())
}

pub(crate) fn publish_target_kind(fe: &FrontendExtension) -> String {
    fe.metadata
        .annotations
        .as_ref()
        .and_then(|annos| annos.get(ANNO_PUBLISH_TARGET_KIND))
        .filter(|kind| !kind.is_empty())
        .cloned()
        .or_else(|| {
            fe.spec
                .publish_policy
                .as_ref()
                .and_then(|policy| policy.default_target_kind.as_ref())
                .map(|kind| match kind {
                    PublishTargetKind::ConfigMap => "ConfigMap".to_string(),
                    PublishTargetKind::Secret => "Secret".to_string(),
                })
        })
        .unwrap_or_else(|| "ConfigMap".to_string())
}

pub(crate) fn generated_publish_request_id(artifact_digest: &str) -> String {
    let timestamp = Utc::now().timestamp_nanos_opt().map_or_else(
        || Utc::now().timestamp_millis().to_string(),
        |ts| ts.to_string(),
    );
    let digest_suffix = artifact_digest
        .strip_prefix("sha256:")
        .unwrap_or(artifact_digest)
        .chars()
        .take(12)
        .collect::<String>();
    format!("api-{timestamp}-{digest_suffix}")
}

pub(crate) fn generated_unpublish_request_id(extension_name: &str) -> String {
    let timestamp = Utc::now().timestamp_nanos_opt().map_or_else(
        || Utc::now().timestamp_millis().to_string(),
        |ts| ts.to_string(),
    );
    let name_hash = sha256_hex(extension_name.as_bytes())
        .chars()
        .take(12)
        .collect::<String>();
    format!("api-unpublish-{timestamp}-{name_hash}")
}
pub(crate) async fn patch_publish_request(
    state: &AppState,
    name: &str,
    request: &ResolvedPublishRequest,
) -> Result<(), ApiError> {
    let api = Api::<FrontendExtension>::all(state.client.clone());
    let mut annotations = serde_json::Map::from_iter([
        (
            ANNO_PUBLISH_REQUEST_ID.to_string(),
            json!(request.request_id),
        ),
        (
            ANNO_PUBLISH_REQUEST_SOURCE_HASH.to_string(),
            json!(request.source_hash),
        ),
        (
            ANNO_PUBLISH_TARGET_KIND.to_string(),
            json!(request.target_kind),
        ),
        (
            ANNO_PUBLISH_TARGET_NAMESPACE.to_string(),
            json!(request.target_ref.namespace),
        ),
        (
            ANNO_PUBLISH_TARGET_NAME.to_string(),
            json!(request.target_ref.name),
        ),
    ]);
    if let Some(generation) = request.generation {
        annotations.insert(
            ANNO_PUBLISH_REQUEST_GENERATION.to_string(),
            json!(generation.to_string()),
        );
    }
    if let Some(artifact_digest) = request.artifact_digest.as_ref() {
        annotations.insert(
            ANNO_PUBLISH_ARTIFACT_DIGEST.to_string(),
            json!(artifact_digest),
        );
    } else {
        annotations.insert(
            ANNO_PUBLISH_ARTIFACT_DIGEST.to_string(),
            serde_json::Value::Null,
        );
    }
    let patch = json!({
        "metadata": {
            "annotations": annotations,
        }
    });
    api.patch(name, &PatchParams::default(), &Patch::Merge(&patch))
        .await
        .map_err(|err| ApiError::kube("failed to patch publish request", &err))?;
    Ok(())
}

pub(crate) async fn patch_unpublish_request(
    state: &AppState,
    name: &str,
    request: &ResolvedUnpublishRequest,
    delete_after_request_id: Option<&str>,
) -> Result<(), ApiError> {
    let api = Api::<FrontendExtension>::all(state.client.clone());
    let mut annotations = BTreeMap::from([
        (
            ANNO_UNPUBLISH_REQUEST_ID.to_string(),
            request.request_id.clone(),
        ),
        (
            ANNO_UNPUBLISH_EXTENSION_NAME.to_string(),
            request.extension_name.clone(),
        ),
        (
            ANNO_PUBLISH_TARGET_KIND.to_string(),
            request.target_kind.clone(),
        ),
        (
            ANNO_PUBLISH_TARGET_NAMESPACE.to_string(),
            request.target_ref.namespace.clone(),
        ),
        (
            ANNO_PUBLISH_TARGET_NAME.to_string(),
            request.target_ref.name.clone(),
        ),
    ]);
    if let Some(delete_after_request_id) = delete_after_request_id {
        annotations.insert(
            ANNO_DELETE_AFTER_UNPUBLISH_REQUEST_ID.to_string(),
            delete_after_request_id.to_string(),
        );
    }
    let patch = json!({
        "metadata": {
            "annotations": annotations,
        }
    });
    api.patch(name, &PatchParams::default(), &Patch::Merge(&patch))
        .await
        .map_err(|err| ApiError::kube("failed to patch unpublish request", &err))?;
    Ok(())
}
