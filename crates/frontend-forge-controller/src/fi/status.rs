use super::*;

pub(crate) fn disabled_status(
    fi: &FrontendIntegration,
    bundle: Option<&JSBundle>,
) -> FrontendIntegrationStatus {
    FrontendIntegrationStatus {
        phase: FrontendIntegrationPhase::Pending,
        observed_spec_hash: fi
            .status
            .as_ref()
            .and_then(|s| s.observed_spec_hash.clone()),
        observed_manifest_hash: fi
            .status
            .as_ref()
            .and_then(|s| s.observed_manifest_hash.clone()),
        observed_generation: Some(fi.metadata.generation.unwrap_or_default()),
        last_build: None,
        bundle_ref: bundle.map(resource_ref),
        last_error: None,
        message: Some("Disabled".to_string()),
        conditions: vec![],
    }
}

pub(crate) fn building_status(
    fi: &FrontendIntegration,
    spec_hash: &str,
    bundle_name: &str,
    job: &Job,
    message: &str,
) -> FrontendIntegrationStatus {
    let started_at = existing_build_started_at(fi, spec_hash, job).unwrap_or_else(Utc::now);

    FrontendIntegrationStatus {
        phase: FrontendIntegrationPhase::Building,
        observed_spec_hash: Some(spec_hash.to_string()),
        observed_manifest_hash: fi
            .status
            .as_ref()
            .and_then(|s| s.observed_manifest_hash.clone()),
        observed_generation: Some(fi.metadata.generation.unwrap_or_default()),
        last_build: Some(LastBuildStatus {
            job_ref: Some(resource_ref(job)),
            started_at: Some(started_at),
        }),
        bundle_ref: Some(ResourceRef {
            name: bundle_name.to_string(),
            namespace: None,
            uid: None,
        }),
        last_error: current_last_error(fi, spec_hash),
        message: Some(message.to_string()),
        conditions: vec![],
    }
}

pub(crate) fn succeeded_status(
    fi: &FrontendIntegration,
    spec_hash: &str,
    bundle: &JSBundle,
    job: &Job,
) -> FrontendIntegrationStatus {
    FrontendIntegrationStatus {
        phase: FrontendIntegrationPhase::Succeeded,
        observed_spec_hash: Some(spec_hash.to_string()),
        observed_manifest_hash: bundle_manifest_hash(bundle),
        observed_generation: Some(fi.metadata.generation.unwrap_or_default()),
        last_build: Some(LastBuildStatus {
            job_ref: Some(resource_ref(job)),
            started_at: fi
                .status
                .as_ref()
                .and_then(|s| s.last_build.clone())
                .and_then(|b| b.started_at),
        }),
        bundle_ref: Some(resource_ref(bundle)),
        last_error: None,
        message: Some("Build succeeded".to_string()),
        conditions: vec![],
    }
}

pub(crate) fn bundle_manifest_hash(bundle: &JSBundle) -> Option<String> {
    if let Some(v) = bundle
        .metadata
        .annotations
        .as_ref()
        .and_then(|annos| annos.get(ANNO_MANIFEST_HASH))
        .cloned()
    {
        return Some(v);
    }

    bundle
        .metadata
        .labels
        .as_ref()
        .and_then(|labels| labels.get(LABEL_MANIFEST_HASH))
        .map(|v| {
            if v.starts_with("sha256:") {
                v.clone()
            } else {
                format!("sha256:{v}")
            }
        })
}

pub(crate) fn failed_status(
    fi: &FrontendIntegration,
    spec_hash: &str,
    last_error: LastBuildError,
) -> FrontendIntegrationStatus {
    FrontendIntegrationStatus {
        phase: FrontendIntegrationPhase::Failed,
        observed_spec_hash: Some(spec_hash.to_string()),
        observed_manifest_hash: fi
            .status
            .as_ref()
            .and_then(|s| s.observed_manifest_hash.clone()),
        observed_generation: Some(fi.metadata.generation.unwrap_or_default()),
        last_build: fi.status.as_ref().and_then(|s| s.last_build.clone()),
        bundle_ref: fi.status.as_ref().and_then(|s| s.bundle_ref.clone()),
        message: Some(last_error.message.clone()),
        last_error: Some(last_error),
        conditions: vec![],
    }
}

pub(crate) fn existing_build_started_at(
    fi: &FrontendIntegration,
    spec_hash: &str,
    job: &Job,
) -> Option<chrono::DateTime<Utc>> {
    let status = fi.status.as_ref()?;
    let observed_hash = status
        .observed_spec_hash
        .as_deref()
        .or(status.observed_manifest_hash.as_deref());
    if observed_hash != Some(spec_hash) {
        return None;
    }

    let last_build = status.last_build.as_ref()?;
    let current_job_name = last_build.job_ref.as_ref()?.name.as_str();
    if current_job_name != job.name_any() {
        return None;
    }

    last_build.started_at
}

pub(crate) fn fi_status_needs_patch(
    fi: &FrontendIntegration,
    desired_status: &FrontendIntegrationStatus,
) -> bool {
    fi.status.as_ref() != Some(desired_status)
}

pub(crate) async fn patch_fi_status(
    fi_api: &Api<FrontendIntegration>,
    fi: &FrontendIntegration,
    status: FrontendIntegrationStatus,
) -> Result<(), Error> {
    if !fi_status_needs_patch(fi, &status) {
        return Ok(());
    }

    let fi_name = fi.name_any();
    let namespace = fi.namespace().unwrap_or_else(|| "<cluster>".to_string());
    let patch = frontend_integration_status_patch(&status, &namespace, &fi_name)?;

    fi_api
        .patch_status(&fi_name, &PatchParams::default(), &Patch::Merge(&patch))
        .await
        .with_context(|_| PatchFrontendIntegrationStatusSnafu {
            namespace,
            name: fi_name.clone(),
        })?;

    Ok(())
}

pub(crate) fn frontend_integration_status_patch(
    status: &FrontendIntegrationStatus,
    namespace: &str,
    name: &str,
) -> Result<serde_json::Value, Error> {
    let mut status_value = serde_json::to_value(status).with_context(|_| {
        SerializeFrontendIntegrationStatusPatchSnafu {
            namespace: namespace.to_string(),
            name: name.to_string(),
        }
    })?;
    let status_object = status_value.as_object_mut().ok_or_else(|| {
        Error::InvalidFrontendIntegrationStatusPatchShape {
            namespace: namespace.to_string(),
            name: name.to_string(),
        }
    })?;

    if status.last_build.is_none() {
        status_object.insert("last_build".to_string(), serde_json::Value::Null);
    }
    if status.bundle_ref.is_none() {
        status_object.insert("bundle_ref".to_string(), serde_json::Value::Null);
    }
    if status.last_error.is_none() {
        status_object.insert("last_error".to_string(), serde_json::Value::Null);
    }

    Ok(json!({
        "status": status_value,
    }))
}
