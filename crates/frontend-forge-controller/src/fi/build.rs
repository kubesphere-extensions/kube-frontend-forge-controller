use super::*;

pub(crate) fn build_spec_hash(fi: &FrontendIntegration) -> Result<String, CommonError> {
    serializable_hash(&fi.spec.without_enabled())
}

pub(crate) fn needs_new_build(
    fi: &FrontendIntegration,
    spec_hash: &str,
    bundle: Option<&JSBundle>,
) -> bool {
    let status = fi.status.as_ref();
    let observed_hash = status
        .and_then(|s| s.observed_spec_hash.as_deref())
        .or_else(|| status.and_then(|s| s.observed_manifest_hash.as_deref()));
    let phase = status.map(|s| s.phase.clone());

    let hash_changed = observed_hash != Some(spec_hash);
    let pending_initial = status.is_none();
    let missing_matching_bundle = observed_hash == Some(spec_hash)
        && !matches!(
            phase,
            Some(FrontendIntegrationPhase::Building | FrontendIntegrationPhase::Failed)
        )
        && !bundle.is_some_and(|bundle| bundle_matches_spec_hash(bundle, spec_hash));

    hash_changed || pending_initial || missing_matching_bundle
}

pub(crate) fn should_reuse_build_job(
    fi: &FrontendIntegration,
    job: &Job,
    bundle: Option<&JSBundle>,
    spec_hash: &str,
) -> bool {
    match observed_job_phase(job.status.as_ref()) {
        ObservedJobPhase::Pending | ObservedJobPhase::Running => true,
        ObservedJobPhase::Succeeded => {
            let bundle_ready =
                bundle.is_some_and(|bundle| bundle_matches_spec_hash(bundle, spec_hash));
            bundle_ready
                && !matches!(
                    fi.status.as_ref().map(|s| s.phase.clone()),
                    Some(FrontendIntegrationPhase::Failed)
                )
        }
        ObservedJobPhase::Failed => false,
    }
}
pub(crate) async fn find_job_for_hash(
    job_api: &Api<Job>,
    namespace: &str,
    fi_name: &str,
    spec_hash: &str,
) -> Result<Option<Job>, Error> {
    let selector = format!(
        "{}={},{}={}",
        LABEL_FI_NAME,
        fi_name,
        LABEL_SPEC_HASH,
        hash_label_value(spec_hash)
    );
    let jobs = job_api
        .list(&ListParams::default().labels(&selector))
        .await
        .with_context(|_| ListJobsForHashSnafu {
            namespace: namespace.to_string(),
            fi_name: fi_name.to_string(),
            spec_hash: spec_hash.to_string(),
        })?;
    let mut items = jobs.items;
    items.sort_by_key(|j| j.metadata.creation_timestamp.clone());
    let latest_job = items.pop();
    if !items.is_empty()
        && let Some(job) = latest_job.as_ref()
    {
        let job_name = job.name_any();
        warn!(
            fi = %fi_name,
            job = %job_name,
            "multiple jobs found for same spec_hash, using latest"
        );
    }
    Ok(latest_job)
}

pub(crate) fn extract_job_error(job: &Job) -> Option<LastBuildError> {
    let status = job.status.as_ref()?;
    let cond = status
        .conditions
        .as_ref()?
        .iter()
        .find(|c| c.status == "True" && c.type_ == "Failed")?;
    let message = cond.message.clone().or_else(|| cond.reason.clone())?;

    Some(LastBuildError {
        source: "job".to_string(),
        message,
        reason: cond.reason.clone(),
        occurred_at: Some(Utc::now()),
    })
}

pub(crate) fn failure_error_for_status(
    fi: &FrontendIntegration,
    spec_hash: &str,
    job: &Job,
) -> LastBuildError {
    if let Some(last_error) = current_last_error(fi, spec_hash) {
        return last_error;
    }

    extract_job_error(job).unwrap_or_else(|| LastBuildError {
        source: "job".to_string(),
        message: extract_job_message(job).unwrap_or_else(|| "Build job failed".to_string()),
        reason: Some("JobFailed".to_string()),
        occurred_at: Some(Utc::now()),
    })
}

pub(crate) fn current_last_error(
    fi: &FrontendIntegration,
    spec_hash: &str,
) -> Option<LastBuildError> {
    let status = fi.status.as_ref()?;
    if status.observed_spec_hash.as_deref() != Some(spec_hash) {
        return None;
    }
    status.last_error.clone()
}

pub(crate) fn bundle_matches_spec_hash(bundle: &JSBundle, spec_hash: &str) -> bool {
    let expected = hash_label_value(spec_hash);
    bundle
        .metadata
        .labels
        .as_ref()
        .and_then(|labels| labels.get(LABEL_SPEC_HASH))
        .is_some_and(|v| v == &expected)
}

pub(crate) fn labels_for(fi_name: &str, spec_hash: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        (LABEL_MANAGED_BY.to_string(), MANAGED_BY_VALUE.to_string()),
        (LABEL_FI_NAME.to_string(), fi_name.to_string()),
        (LABEL_SPEC_HASH.to_string(), hash_label_value(spec_hash)),
    ])
}

pub(crate) fn make_build_job(
    fi: &FrontendIntegration,
    config: &ControllerConfig,
    job_name: &str,
    jsbundle_name: &str,
    spec_hash: &str,
) -> Job {
    let fi_name = fi.name_any();
    let mut labels = labels_for(&fi_name, spec_hash);
    labels.insert(LABEL_BUILD_KIND.to_string(), BUILD_KIND_VALUE.to_string());

    let mut annotations = BTreeMap::new();
    if let Some(generation) = fi.metadata.generation {
        annotations.insert(ANNO_OBSERVED_GENERATION.to_string(), generation.to_string());
    }

    let env = vec![
        EnvVar {
            name: "FI_NAME".to_string(),
            value: Some(fi_name),
            ..Default::default()
        },
        EnvVar {
            name: "SPEC_HASH".to_string(),
            value: Some(spec_hash.to_string()),
            ..Default::default()
        },
        EnvVar {
            name: "JSBUNDLE_NAME".to_string(),
            value: Some(jsbundle_name.to_string()),
            ..Default::default()
        },
        EnvVar {
            name: "BUILD_SERVICE_BASE_URL".to_string(),
            value: Some(config.build_service_base_url.clone()),
            ..Default::default()
        },
        EnvVar {
            name: "JSBUNDLE_CONFIGMAP_NAMESPACE".to_string(),
            value: Some(config.jsbundle_configmap_namespace.clone()),
            ..Default::default()
        },
        EnvVar {
            name: "JSBUNDLE_CONFIG_KEY".to_string(),
            value: Some(config.jsbundle_config_key.clone()),
            ..Default::default()
        },
        EnvVar {
            name: "BUILD_SERVICE_TIMEOUT_SECONDS".to_string(),
            value: Some(config.build_service_timeout_seconds.to_string()),
            ..Default::default()
        },
        EnvVar {
            name: "STALE_CHECK_GRACE_SECONDS".to_string(),
            value: Some(config.stale_check_grace_seconds.to_string()),
            ..Default::default()
        },
    ];

    let container = Container {
        name: "runner".to_string(),
        image: Some(config.runner_image.clone()),
        env: Some(env),
        ..Default::default()
    };

    Job {
        metadata: ObjectMeta {
            name: Some(job_name.to_string()),
            namespace: Some(config.work_namespace.clone()),
            labels: Some(labels),
            annotations: Some(annotations),
            owner_references: base_owner_ref(fi).map(|o| vec![o]),
            ..Default::default()
        },
        spec: Some(JobSpec {
            active_deadline_seconds: Some(config.job_active_deadline_seconds),
            ttl_seconds_after_finished: config.job_ttl_seconds_after_finished,
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(BTreeMap::from([(
                        "app.kubernetes.io/name".to_string(),
                        "frontend-forge-runner".to_string(),
                    )])),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    restart_policy: Some("Never".to_string()),
                    service_account_name: config.runner_service_account.clone(),
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
