use super::*;

#[derive(Clone, Debug)]
pub(crate) struct UnpublishRequest {
    pub(crate) request_id: String,
    pub(crate) extension_name: String,
    pub(crate) target_ref: NamespacedResourceRef,
    pub(crate) target_kind: String,
}

#[derive(Clone, Debug)]
pub(crate) struct UnpublishSync {
    pub(crate) status: Option<UnpublishStatus>,
    pub(crate) should_requeue: bool,
}

pub(crate) async fn sync_unpublish(
    fe: &FrontendExtension,
    job_api: &Api<Job>,
    namespace: &str,
    config: &ControllerConfig,
) -> Result<UnpublishSync, Error> {
    let Some(request_id) = unpublish_request_id(fe) else {
        return Ok(UnpublishSync {
            status: fe
                .status
                .as_ref()
                .and_then(|status| status.unpublish.clone()),
            should_requeue: false,
        });
    };

    let request = match unpublish_request(fe, request_id) {
        Ok(request) => request,
        Err(status) => {
            return Ok(UnpublishSync {
                status: Some(*status),
                should_requeue: false,
            });
        }
    };
    let job_name = unpublish_job_name(&fe.name_any(), &request.request_id);
    let job = if let Some(job) =
        job_api
            .get_opt(&job_name)
            .await
            .with_context(|_| GetUnpublishJobSnafu {
                namespace: namespace.to_string(),
                name: job_name.clone(),
            })? {
        job
    } else {
        if unpublish_already_finished(fe, &request) {
            return Ok(UnpublishSync {
                status: fe
                    .status
                    .as_ref()
                    .and_then(|status| status.unpublish.clone()),
                should_requeue: false,
            });
        }
        let desired_job = make_unpublish_job(fe, config, &job_name, &request);
        create_or_get_job(job_api, namespace, desired_job, &job_name).await?
    };

    let status = unpublish_status_from_job(&request, &job);
    let should_requeue = matches!(
        status.phase,
        UnpublishPhase::Pending | UnpublishPhase::Running
    );

    Ok(UnpublishSync {
        status: Some(status),
        should_requeue,
    })
}

pub(crate) fn unpublish_request_id(fe: &FrontendExtension) -> Option<&str> {
    fe.metadata
        .annotations
        .as_ref()
        .and_then(|annos| annos.get(ANNO_UNPUBLISH_REQUEST_ID))
        .map(String::as_str)
        .filter(|request_id| !request_id.is_empty())
}

pub(crate) fn unpublish_request(
    fe: &FrontendExtension,
    request_id: &str,
) -> Result<UnpublishRequest, Box<UnpublishStatus>> {
    let annos = fe.metadata.annotations.as_ref();
    let extension_name = annos
        .and_then(|annos| annos.get(ANNO_UNPUBLISH_EXTENSION_NAME))
        .filter(|name| !name.is_empty())
        .cloned()
        .unwrap_or_else(|| frontend_extension_package_name(fe));
    let target_ref = publish_target_ref(fe).ok_or_else(|| {
        Box::new(failed_unpublish_status(
            request_id,
            Some(extension_name.clone()),
            "publish targetRef is required",
        ))
    })?;
    let target_kind = publish_target_kind(fe);
    if !matches!(target_kind.as_str(), "ConfigMap" | "Secret") {
        return Err(Box::new(failed_unpublish_status(
            request_id,
            Some(extension_name.clone()),
            "publish targetKind must be ConfigMap or Secret",
        )));
    }

    Ok(UnpublishRequest {
        request_id: request_id.to_string(),
        extension_name,
        target_ref,
        target_kind,
    })
}

pub(crate) fn failed_unpublish_status(
    request_id: &str,
    extension_name: Option<String>,
    message: &str,
) -> UnpublishStatus {
    UnpublishStatus {
        phase: UnpublishPhase::Failed,
        request_id: Some(request_id.to_string()),
        extension_name,
        last_error: Some(message.to_string()),
        ..Default::default()
    }
}

pub(crate) fn unpublish_already_finished(
    fe: &FrontendExtension,
    request: &UnpublishRequest,
) -> bool {
    fe.status
        .as_ref()
        .and_then(|status| status.unpublish.as_ref())
        .is_some_and(|unpublish| {
            unpublish.request_id.as_deref() == Some(request.request_id.as_str())
                && unpublish.extension_name.as_deref() == Some(request.extension_name.as_str())
                && matches!(
                    unpublish.phase,
                    UnpublishPhase::Succeeded | UnpublishPhase::Failed
                )
        })
}

pub(crate) fn make_unpublish_job(
    fe: &FrontendExtension,
    config: &ControllerConfig,
    job_name: &str,
    request: &UnpublishRequest,
) -> Job {
    let fe_name = fe.name_any();
    let request_hash = format!("sha256:{}", sha256_hex(request.request_id.as_bytes()));
    let labels = BTreeMap::from([
        (LABEL_MANAGED_BY.to_string(), MANAGED_BY_VALUE.to_string()),
        (LABEL_FE_NAME.to_string(), fe_name.clone()),
        (
            LABEL_UNPUBLISH_KIND.to_string(),
            UNPUBLISH_KIND_VALUE.to_string(),
        ),
        (
            LABEL_UNPUBLISH_REQUEST_HASH.to_string(),
            hash_label_value(&request_hash),
        ),
    ]);

    let mut annotations = BTreeMap::from([
        (
            ANNO_UNPUBLISH_REQUEST_ID.to_string(),
            request.request_id.clone(),
        ),
        (
            ANNO_UNPUBLISH_EXTENSION_NAME.to_string(),
            request.extension_name.clone(),
        ),
    ]);
    if let Some(generation) = fe.metadata.generation {
        annotations.insert(ANNO_OBSERVED_GENERATION.to_string(), generation.to_string());
    }

    let env = vec![
        EnvVar {
            name: "FE_NAME".to_string(),
            value: Some(fe_name),
            ..Default::default()
        },
        EnvVar {
            name: "PUBLISH_ACTION".to_string(),
            value: Some("unpublish".to_string()),
            ..Default::default()
        },
        EnvVar {
            name: "UNPUBLISH_REQUEST_ID".to_string(),
            value: Some(request.request_id.clone()),
            ..Default::default()
        },
        EnvVar {
            name: "UNPUBLISH_EXTENSION_NAME".to_string(),
            value: Some(request.extension_name.clone()),
            ..Default::default()
        },
        EnvVar {
            name: "PUBLISH_TARGET_KIND".to_string(),
            value: Some(request.target_kind.clone()),
            ..Default::default()
        },
        EnvVar {
            name: "PUBLISH_TARGET_NAMESPACE".to_string(),
            value: Some(request.target_ref.namespace.clone()),
            ..Default::default()
        },
        EnvVar {
            name: "PUBLISH_TARGET_NAME".to_string(),
            value: Some(request.target_ref.name.clone()),
            ..Default::default()
        },
    ];

    let container = Container {
        name: "publisher".to_string(),
        image: Some(config.publisher_image.clone()),
        env: Some(env),
        ..Default::default()
    };

    Job {
        metadata: ObjectMeta {
            name: Some(job_name.to_string()),
            namespace: Some(config.work_namespace.clone()),
            labels: Some(labels),
            annotations: Some(annotations),
            owner_references: base_owner_ref(fe).map(|o| vec![o]),
            ..Default::default()
        },
        spec: Some(JobSpec {
            active_deadline_seconds: Some(config.job_active_deadline_seconds),
            ttl_seconds_after_finished: config.job_ttl_seconds_after_finished,
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(BTreeMap::from([(
                        "app.kubernetes.io/name".to_string(),
                        "frontend-forge-extension-publisher".to_string(),
                    )])),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    restart_policy: Some("Never".to_string()),
                    service_account_name: config.publisher_service_account.clone(),
                    containers: vec![container],
                    ..Default::default()
                }),
            },
            backoff_limit: Some(0),
            ..Default::default()
        }),
        status: None,
    }
}

pub(crate) fn unpublish_status_from_job(request: &UnpublishRequest, job: &Job) -> UnpublishStatus {
    let phase = match observed_job_phase(job.status.as_ref()) {
        ObservedJobPhase::Pending => UnpublishPhase::Pending,
        ObservedJobPhase::Running => UnpublishPhase::Running,
        ObservedJobPhase::Succeeded => UnpublishPhase::Succeeded,
        ObservedJobPhase::Failed => UnpublishPhase::Failed,
    };
    let last_error = if matches!(phase, UnpublishPhase::Failed) {
        extract_job_message(job).or_else(|| Some("Unpublish job failed".to_string()))
    } else {
        None
    };

    UnpublishStatus {
        phase,
        request_id: Some(request.request_id.clone()),
        extension_name: Some(request.extension_name.clone()),
        job_ref: Some(namespaced_ref(job)),
        started_at: job
            .status
            .as_ref()
            .and_then(|status| status.start_time.as_ref())
            .and_then(k8s_time_to_chrono),
        finished_at: job
            .status
            .as_ref()
            .and_then(|status| status.completion_time.as_ref())
            .and_then(k8s_time_to_chrono),
        last_error,
    }
}

pub(crate) fn apply_unpublish_sync(
    status: &mut FrontendExtensionStatus,
    unpublish: &UnpublishSync,
) {
    status.unpublish.clone_from(&unpublish.status);
    if unpublish
        .status
        .as_ref()
        .is_some_and(|status| matches!(status.phase, UnpublishPhase::Succeeded))
        && let Some(publish) = status.publish.as_mut()
    {
        publish.active = false;
    }
}

pub(crate) fn should_delete_after_unpublish(
    fe: &FrontendExtension,
    unpublish: &UnpublishSync,
) -> bool {
    let Some(delete_after_request_id) = fe
        .metadata
        .annotations
        .as_ref()
        .and_then(|annos| annos.get(ANNO_DELETE_AFTER_UNPUBLISH_REQUEST_ID))
        .filter(|request_id| !request_id.is_empty())
    else {
        return false;
    };
    unpublish.status.as_ref().is_some_and(|status| {
        matches!(status.phase, UnpublishPhase::Succeeded)
            && status.request_id.as_deref() == Some(delete_after_request_id.as_str())
    })
}

pub(crate) async fn delete_frontend_extension(
    fe_api: &Api<FrontendExtension>,
    name: &str,
) -> Result<(), Error> {
    fe_api
        .delete(name, &DeleteParams::default())
        .await
        .with_context(|_| DeleteFrontendExtensionSnafu {
            name: name.to_string(),
        })?;
    Ok(())
}
