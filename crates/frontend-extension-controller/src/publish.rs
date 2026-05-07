use super::*;

pub(crate) struct PublishRequest {
    pub(crate) request_id: String,
    pub(crate) artifact_digest: String,
    pub(crate) target_ref: NamespacedResourceRef,
    pub(crate) target_kind: String,
}

#[derive(Clone, Debug)]
pub(crate) struct PublishSync {
    pub(crate) status: Option<PublishStatus>,
    pub(crate) should_requeue: bool,
}

pub(crate) async fn sync_publish(
    fe: &FrontendExtension,
    job_api: &Api<Job>,
    namespace: &str,
    config: &ControllerConfig,
    artifact_key: &str,
    artifact: &PackageArtifactMetadata,
    artifact_cm: &ConfigMap,
) -> Result<PublishSync, Error> {
    let Some(request_id) = publish_request_id(fe) else {
        return Ok(PublishSync {
            status: Some(current_publish_for_artifact(
                fe,
                &artifact.digest,
                artifact_key,
            )),
            should_requeue: false,
        });
    };

    let request = match publish_request(fe, request_id, &artifact.digest) {
        Ok(request) => request,
        Err(status) => {
            return Ok(PublishSync {
                status: Some(*status),
                should_requeue: false,
            });
        }
    };
    let job_name = publish_job_name(&fe.name_any(), &request.request_id);
    let job = if let Some(job) =
        job_api
            .get_opt(&job_name)
            .await
            .with_context(|_| GetPublishJobSnafu {
                namespace: namespace.to_string(),
                name: job_name.clone(),
            })? {
        job
    } else {
        if publish_already_finished(fe, &request, artifact_key) {
            return Ok(PublishSync {
                status: fe.status.as_ref().and_then(|status| status.publish.clone()),
                should_requeue: false,
            });
        }
        let desired_job = make_publish_job(fe, config, &job_name, &request, artifact, artifact_cm);
        create_or_get_job(job_api, namespace, desired_job, &job_name).await?
    };

    let status = publish_status_from_job(&request, &job);
    let should_requeue = matches!(status.phase, PublishPhase::Pending | PublishPhase::Running);

    Ok(PublishSync {
        status: Some(status),
        should_requeue,
    })
}

pub(crate) fn publish_request_id(fe: &FrontendExtension) -> Option<&str> {
    fe.metadata
        .annotations
        .as_ref()
        .and_then(|annos| annos.get(ANNO_PUBLISH_REQUEST_ID))
        .map(String::as_str)
        .filter(|request_id| !request_id.is_empty())
}

pub(crate) fn publish_request(
    fe: &FrontendExtension,
    request_id: &str,
    current_artifact_digest: &str,
) -> Result<PublishRequest, Box<PublishStatus>> {
    let annos = fe.metadata.annotations.as_ref();
    let requested_digest = annos
        .and_then(|annos| annos.get(ANNO_PUBLISH_ARTIFACT_DIGEST))
        .filter(|digest| !digest.is_empty())
        .cloned()
        .ok_or_else(|| {
            Box::new(failed_publish_status(
                request_id,
                None,
                "publish artifact digest annotation is required",
            ))
        })?;

    if requested_digest != current_artifact_digest {
        return Err(Box::new(failed_publish_status(
            request_id,
            Some(requested_digest),
            "publish artifact digest does not match current ready artifact",
        )));
    }

    let target_ref = publish_target_ref(fe).ok_or_else(|| {
        Box::new(failed_publish_status(
            request_id,
            Some(current_artifact_digest.to_string()),
            "publish targetRef is required",
        ))
    })?;
    let target_kind = publish_target_kind(fe);
    if !matches!(target_kind.as_str(), "ConfigMap" | "Secret") {
        return Err(Box::new(failed_publish_status(
            request_id,
            Some(current_artifact_digest.to_string()),
            "publish targetKind must be ConfigMap or Secret",
        )));
    }

    Ok(PublishRequest {
        request_id: request_id.to_string(),
        artifact_digest: current_artifact_digest.to_string(),
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

pub(crate) fn failed_publish_status(
    request_id: &str,
    artifact_digest: Option<String>,
    message: &str,
) -> PublishStatus {
    PublishStatus {
        phase: PublishPhase::Failed,
        request_id: Some(request_id.to_string()),
        artifact_digest,
        last_error: Some(message.to_string()),
        ..Default::default()
    }
}

pub(crate) fn current_publish_for_artifact(
    fe: &FrontendExtension,
    artifact_digest: &str,
    artifact_key: &str,
) -> PublishStatus {
    let publish = fe.status.as_ref().and_then(|status| status.publish.clone());
    match publish {
        Some(status)
            if status.artifact_digest.as_deref() == Some(artifact_digest)
                && current_status_artifact_key(fe) == Some(artifact_key) =>
        {
            status
        }
        _ => PublishStatus::default(),
    }
}

pub(crate) fn publish_already_finished(
    fe: &FrontendExtension,
    request: &PublishRequest,
    artifact_key: &str,
) -> bool {
    fe.status
        .as_ref()
        .and_then(|status| status.publish.as_ref())
        .is_some_and(|publish| {
            publish.request_id.as_deref() == Some(request.request_id.as_str())
                && publish.artifact_digest.as_deref() == Some(request.artifact_digest.as_str())
                && current_status_artifact_key(fe) == Some(artifact_key)
                && matches!(
                    publish.phase,
                    PublishPhase::Succeeded | PublishPhase::Failed
                )
        })
}

pub(crate) fn current_status_artifact_key(fe: &FrontendExtension) -> Option<&str> {
    fe.status
        .as_ref()
        .and_then(|status| status.artifact.as_ref())
        .and_then(|artifact| artifact.artifact_key.as_deref())
}

pub(crate) fn make_publish_job(
    fe: &FrontendExtension,
    config: &ControllerConfig,
    job_name: &str,
    request: &PublishRequest,
    artifact: &PackageArtifactMetadata,
    artifact_cm: &ConfigMap,
) -> Job {
    let fe_name = fe.name_any();
    let request_hash = format!("sha256:{}", sha256_hex(request.request_id.as_bytes()));
    let labels = BTreeMap::from([
        (LABEL_MANAGED_BY.to_string(), MANAGED_BY_VALUE.to_string()),
        (LABEL_FE_NAME.to_string(), fe_name.clone()),
        (
            LABEL_PUBLISH_KIND.to_string(),
            PUBLISH_KIND_VALUE.to_string(),
        ),
        (
            LABEL_PUBLISH_REQUEST_HASH.to_string(),
            hash_label_value(&request_hash),
        ),
    ]);

    let mut annotations = BTreeMap::from([
        (
            ANNO_PUBLISH_REQUEST_ID.to_string(),
            request.request_id.clone(),
        ),
        (
            ANNO_PUBLISH_ARTIFACT_DIGEST.to_string(),
            request.artifact_digest.clone(),
        ),
        (
            ANNO_ARTIFACT_DIGEST.to_string(),
            request.artifact_digest.clone(),
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
            name: "PUBLISH_REQUEST_ID".to_string(),
            value: Some(request.request_id.clone()),
            ..Default::default()
        },
        EnvVar {
            name: "ARTIFACT_DIGEST".to_string(),
            value: Some(request.artifact_digest.clone()),
            ..Default::default()
        },
        EnvVar {
            name: "ARTIFACT_CONFIGMAP_NAMESPACE".to_string(),
            value: Some(artifact_cm.namespace().unwrap_or_default()),
            ..Default::default()
        },
        EnvVar {
            name: "ARTIFACT_CONFIGMAP_NAME".to_string(),
            value: Some(artifact_cm.name_any()),
            ..Default::default()
        },
        EnvVar {
            name: "ARTIFACT_CONFIGMAP_KEY".to_string(),
            value: Some(PACKAGE_KEY.to_string()),
            ..Default::default()
        },
        EnvVar {
            name: "ARTIFACT_FILENAME".to_string(),
            value: Some(artifact.filename.clone()),
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

pub(crate) fn publish_status_from_job(request: &PublishRequest, job: &Job) -> PublishStatus {
    let phase = match observed_job_phase(job.status.as_ref()) {
        ObservedJobPhase::Pending => PublishPhase::Pending,
        ObservedJobPhase::Running => PublishPhase::Running,
        ObservedJobPhase::Succeeded => PublishPhase::Succeeded,
        ObservedJobPhase::Failed => PublishPhase::Failed,
    };
    let last_error = if matches!(phase, PublishPhase::Failed) {
        extract_job_message(job).or_else(|| Some("Publish job failed".to_string()))
    } else {
        None
    };
    let active = matches!(phase, PublishPhase::Succeeded);

    PublishStatus {
        phase,
        active,
        request_id: Some(request.request_id.clone()),
        artifact_digest: Some(request.artifact_digest.clone()),
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

pub(crate) fn apply_publish_sync(status: &mut FrontendExtensionStatus, publish: &PublishSync) {
    status.publish.clone_from(&publish.status);
    let generation = status.observed_generation;
    status
        .conditions
        .retain(|condition| condition.type_ != "PublishSucceeded");
    status.conditions.push(fe_publish_condition_from_status(
        status.publish.as_ref(),
        generation,
    ));
}
