use super::*;

pub(crate) async fn list_extensions(
    State(state): State<Arc<AppState>>,
) -> Result<Json<FrontendExtensionListResponse>, ApiError> {
    let api = Api::<FrontendExtension>::all(state.client.clone());
    let list = api
        .list(&ListParams::default())
        .await
        .map_err(|err| ApiError::kube("failed to list FrontendExtensions", &err))?;
    Ok(Json(FrontendExtensionListResponse {
        items: list.items.iter().map(extension_summary).collect(),
    }))
}

pub(crate) async fn create_extension(
    State(state): State<Arc<AppState>>,
    Json(extension): Json<FrontendExtension>,
) -> Result<Json<FrontendExtension>, ApiError> {
    let api = Api::<FrontendExtension>::all(state.client.clone());
    let created = api
        .create(&PostParams::default(), &extension)
        .await
        .map_err(|err| ApiError::kube("failed to create FrontendExtension", &err))?;
    Ok(Json(created))
}

pub(crate) async fn get_extension(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<FrontendExtension>, ApiError> {
    let extension = get_fe(&state, &name).await?;
    Ok(Json(extension))
}

pub(crate) async fn get_publish_status(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<PublishStatus>, ApiError> {
    let extension = get_fe(&state, &name).await?;
    Ok(Json(
        extension
            .status
            .and_then(|status| status.publish)
            .unwrap_or_default(),
    ))
}

pub(crate) async fn get_unpublish_status(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<UnpublishStatus>, ApiError> {
    let extension = get_fe(&state, &name).await?;
    Ok(Json(
        extension
            .status
            .and_then(|status| status.unpublish)
            .unwrap_or_default(),
    ))
}

pub(crate) async fn download_extension(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Response<Body>, ApiError> {
    let extension = get_fe(&state, &name).await?;
    let artifact = ready_artifact(&extension)?;
    let bytes = artifact_bytes(&state, artifact).await?;
    verify_artifact_digest(&bytes, &artifact.digest)?;

    let mut response = Response::new(Body::from(bytes));
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_str(&artifact.media_type).map_err(|err| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("invalid artifact media type: {err}"),
            )
        })?,
    );
    response.headers_mut().insert(
        CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{}\"", artifact.filename)).map_err(
            |err| {
                ApiError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("invalid artifact filename: {err}"),
                )
            },
        )?,
    );
    Ok(response)
}

pub(crate) async fn trigger_unpublish(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(request): Json<UnpublishRequest>,
) -> Result<(StatusCode, Json<UnpublishStatus>), ApiError> {
    let extension = get_fe(&state, &name).await?;
    let request = resolve_unpublish_request(&extension, &request)?;

    if let Some(current) = extension
        .status
        .as_ref()
        .and_then(|status| status.unpublish.as_ref())
        && current.request_id.as_deref() == Some(request.request_id.as_str())
        && current.extension_name.as_deref() == Some(request.extension_name.as_str())
    {
        return Ok((StatusCode::ACCEPTED, Json(current.clone())));
    }

    patch_unpublish_request(&state, &name, &request, None).await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(UnpublishStatus {
            phase: UnpublishPhase::Pending,
            request_id: Some(request.request_id),
            extension_name: Some(request.extension_name),
            ..Default::default()
        }),
    ))
}

pub(crate) async fn delete_extension(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(request): Json<DeleteRequest>,
) -> Result<(StatusCode, Json<DeleteResponse>), ApiError> {
    let extension = get_fe(&state, &name).await?;
    if request.unpublish && currently_published(&extension) {
        let unpublish_request =
            resolve_unpublish_request(&extension, &UnpublishRequest { request_id: None })?;
        patch_unpublish_request(
            &state,
            &name,
            &unpublish_request,
            Some(unpublish_request.request_id.as_str()),
        )
        .await?;
        return Ok((
            StatusCode::ACCEPTED,
            Json(DeleteResponse {
                deleted: false,
                unpublish_skipped: false,
                unpublish: Some(UnpublishStatus {
                    phase: UnpublishPhase::Pending,
                    request_id: Some(unpublish_request.request_id),
                    extension_name: Some(unpublish_request.extension_name),
                    ..Default::default()
                }),
            }),
        ));
    }

    delete_fe(&state, &name).await?;
    Ok((
        StatusCode::OK,
        Json(DeleteResponse {
            deleted: true,
            unpublish_skipped: true,
            unpublish: None,
        }),
    ))
}

pub(crate) async fn trigger_publish(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(request): Json<PublishRequest>,
) -> Result<(StatusCode, Json<PublishStatus>), ApiError> {
    let extension = get_fe(&state, &name).await?;
    let artifact = ready_artifact(&extension)?;
    if request
        .expected_artifact_digest
        .as_deref()
        .is_some_and(|expected| expected != artifact.digest)
    {
        return Err(ApiError::conflict(
            "publish expectedArtifactDigest does not match current ready artifact",
        ));
    }
    let request = resolve_publish_request(&extension, &request, artifact)?;

    if let Some(current) = extension
        .status
        .as_ref()
        .and_then(|status| status.publish.as_ref())
        && current.request_id.as_deref() == Some(request.request_id.as_str())
        && current.artifact_digest.as_deref() == Some(request.artifact_digest.as_str())
    {
        return Ok((StatusCode::ACCEPTED, Json(current.clone())));
    }

    patch_publish_request(&state, &name, &request).await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(PublishStatus {
            phase: frontend_forge_api::PublishPhase::Pending,
            request_id: Some(request.request_id),
            artifact_digest: Some(request.artifact_digest),
            ..Default::default()
        }),
    ))
}
pub(crate) async fn get_fe(state: &AppState, name: &str) -> Result<FrontendExtension, ApiError> {
    let api = Api::<FrontendExtension>::all(state.client.clone());
    api.get(name).await.map_err(|err| match err {
        kube::Error::Api(ae) if ae.code == 404 => {
            ApiError::not_found(format!("FrontendExtension {name} not found"))
        }
        err => ApiError::kube("failed to get FrontendExtension", &err),
    })
}

pub(crate) async fn delete_fe(state: &AppState, name: &str) -> Result<(), ApiError> {
    let api = Api::<FrontendExtension>::all(state.client.clone());
    api.delete(name, &DeleteParams::default())
        .await
        .map_err(|err| match err {
            kube::Error::Api(ae) if ae.code == 404 => {
                ApiError::not_found(format!("FrontendExtension {name} not found"))
            }
            err => ApiError::kube("failed to delete FrontendExtension", &err),
        })?;
    Ok(())
}

pub(crate) fn extension_summary(fe: &FrontendExtension) -> FrontendExtensionSummary {
    let status = fe.status.clone().unwrap_or_default();
    FrontendExtensionSummary {
        name: fe.name_any(),
        generation: fe.metadata.generation,
        package: FrontendExtensionPackageSummary {
            version: fe.spec.package.version.clone(),
            display_name: fe.spec.package.display_name.clone(),
        },
        phase: status.phase,
        artifact_digest: status
            .artifact
            .as_ref()
            .map(|artifact| artifact.digest.clone()),
        download: status.download.map(|download| DownloadSummary {
            ready: download.ready,
            filename: download.filename,
        }),
        publish: status.publish.unwrap_or_default(),
    }
}

pub(crate) fn currently_published(fe: &FrontendExtension) -> bool {
    fe.status
        .as_ref()
        .and_then(|status| status.publish.as_ref())
        .is_some_and(|publish| matches!(publish.phase, PublishPhase::Succeeded) && publish.active)
}

pub(crate) fn ready_artifact(fe: &FrontendExtension) -> Result<&ExtensionArtifactStatus, ApiError> {
    let status = fe
        .status
        .as_ref()
        .ok_or_else(|| ApiError::conflict("FrontendExtension has no status yet"))?;
    if !matches!(status.phase, FrontendExtensionPhase::Ready) {
        return Err(ApiError::conflict(
            "FrontendExtension artifact is not ready",
        ));
    }
    let download = status
        .download
        .as_ref()
        .ok_or_else(|| ApiError::conflict("FrontendExtension download status is missing"))?;
    if !download.ready {
        return Err(ApiError::conflict(
            "FrontendExtension artifact is not downloadable",
        ));
    }
    let artifact = status
        .artifact
        .as_ref()
        .ok_or_else(|| ApiError::conflict("FrontendExtension artifact status is missing"))?;
    if status.observed_source_hash.as_deref() != Some(artifact.source_hash.as_str()) {
        return Err(ApiError::conflict(
            "FrontendExtension artifact does not match observed source hash",
        ));
    }
    if !matches!(artifact.storage.kind, ArtifactStorageKind::ConfigMap) {
        return Err(ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "unsupported artifact storage kind",
        ));
    }
    Ok(artifact)
}
