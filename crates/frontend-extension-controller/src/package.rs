use super::*;

pub(crate) struct PackageAttempt {
    pub(crate) attempt: u32,
    pub(crate) job: Job,
}

pub(crate) async fn list_package_jobs_for_artifact_key(
    job_api: &Api<Job>,
    namespace: &str,
    fe: &FrontendExtension,
    artifact_key: &str,
) -> Result<Vec<PackageAttempt>, Error> {
    let fe_name = fe.name_any();
    let selector = package_job_selector(fe, artifact_key);
    let jobs = job_api
        .list(&ListParams::default().labels(&selector))
        .await
        .with_context(|_| ListPackageJobsForArtifactKeySnafu {
            namespace: namespace.to_string(),
            fe_name: fe_name.clone(),
            artifact_key: artifact_key.to_string(),
        })?;
    Ok(jobs
        .items
        .into_iter()
        .filter_map(|job| {
            package_attempt_from_job(&job).map(|attempt| PackageAttempt { attempt, job })
        })
        .collect())
}

pub(crate) fn package_job_selector(fe: &FrontendExtension, artifact_key: &str) -> String {
    format!(
        "{}={},{}={},{}={},{}={}",
        LABEL_FE_NAME,
        fe.name_any(),
        LABEL_FE_UID,
        frontend_extension_uid_label(fe),
        LABEL_ARTIFACT_KEY_SHORT,
        hash_label_value(artifact_key),
        LABEL_PACKAGE_KIND,
        PACKAGE_KIND_VALUE
    )
}

pub(crate) fn package_attempt_from_job(job: &Job) -> Option<u32> {
    let name = job.name_any();
    let (_, attempt) = name.rsplit_once("-a")?;
    let attempt = attempt.parse::<u32>().ok()?;
    (attempt > 0).then_some(attempt)
}

pub(crate) fn latest_package_attempt(attempts: Vec<PackageAttempt>) -> Option<PackageAttempt> {
    attempts.into_iter().max_by(|a, b| {
        a.attempt
            .cmp(&b.attempt)
            .then_with(|| {
                a.job
                    .metadata
                    .creation_timestamp
                    .cmp(&b.job.metadata.creation_timestamp)
            })
            .then_with(|| a.job.name_any().cmp(&b.job.name_any()))
    })
}

pub(crate) fn frontend_extension_rebuild_token(fe: &FrontendExtension) -> String {
    fe.metadata
        .annotations
        .as_ref()
        .and_then(|annos| annos.get(ANNO_REBUILD_TOKEN))
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty())
        .unwrap_or_default()
}

pub(crate) fn frontend_extension_uid_label(fe: &FrontendExtension) -> String {
    fe.meta().uid.clone().unwrap_or_default()
}

pub(crate) fn package_attempts_exceeded_message(
    artifact_key: &str,
    latest_attempt: u32,
    max_attempts: u32,
    latest_message: &str,
) -> String {
    format!(
        "Package attempts exceeded for artifactKey short {}: latest attempt {}, max attempts {}. \
         Latest job failure: {}",
        hash_label_value(artifact_key),
        latest_attempt,
        max_attempts,
        latest_message
    )
}
pub(crate) fn make_package_job(
    fe: &FrontendExtension,
    config: &ControllerConfig,
    job_name: &str,
    source_hash: &str,
    artifact_key: &str,
    rebuild_token: &str,
    artifact_configmap_name: &str,
) -> Job {
    let fe_name = fe.name_any();
    let fe_uid = frontend_extension_uid_label(fe);
    let mut labels = BTreeMap::from([
        (
            LABEL_FE_MANAGED_BY.to_string(),
            MANAGED_BY_VALUE.to_string(),
        ),
        (LABEL_FE_NAME.to_string(), fe_name.clone()),
        (LABEL_FE_UID.to_string(), fe_uid.clone()),
        (
            LABEL_SOURCE_HASH_SHORT.to_string(),
            hash_label_value(source_hash),
        ),
        (
            LABEL_ARTIFACT_KEY_SHORT.to_string(),
            hash_label_value(artifact_key),
        ),
        (
            LABEL_PACKAGE_KIND.to_string(),
            PACKAGE_KIND_VALUE.to_string(),
        ),
    ]);
    labels.insert(
        LABEL_FE_BUILD_KIND.to_string(),
        "frontend-extension-package".to_string(),
    );

    let mut annotations = BTreeMap::new();
    annotations.insert(ANNO_SOURCE_HASH.to_string(), source_hash.to_string());
    annotations.insert(ANNO_ARTIFACT_KEY.to_string(), artifact_key.to_string());
    if let Some(generation) = fe.metadata.generation {
        annotations.insert(
            ANNO_FE_OBSERVED_GENERATION.to_string(),
            generation.to_string(),
        );
    }

    let env = vec![
        EnvVar {
            name: "FE_NAME".to_string(),
            value: Some(fe_name),
            ..Default::default()
        },
        EnvVar {
            name: "FE_UID".to_string(),
            value: Some(fe_uid),
            ..Default::default()
        },
        EnvVar {
            name: "SOURCE_HASH".to_string(),
            value: Some(source_hash.to_string()),
            ..Default::default()
        },
        EnvVar {
            name: "ARTIFACT_KEY".to_string(),
            value: Some(artifact_key.to_string()),
            ..Default::default()
        },
        EnvVar {
            name: "REBUILD_TOKEN".to_string(),
            value: Some(rebuild_token.to_string()),
            ..Default::default()
        },
        EnvVar {
            name: "ARTIFACT_CONFIGMAP_NAMESPACE".to_string(),
            value: Some(config.artifact_configmap_namespace.clone()),
            ..Default::default()
        },
        EnvVar {
            name: "ARTIFACT_CONFIGMAP_NAME".to_string(),
            value: Some(artifact_configmap_name.to_string()),
            ..Default::default()
        },
        EnvVar {
            name: "BUILD_SERVICE_BASE_URL".to_string(),
            value: Some(config.build_service_base_url.clone()),
            ..Default::default()
        },
        EnvVar {
            name: "BUILD_SERVICE_TIMEOUT_SECONDS".to_string(),
            value: Some(config.build_service_timeout_seconds.to_string()),
            ..Default::default()
        },
        EnvVar {
            name: "JSBUNDLE_CONFIG_KEY".to_string(),
            value: Some(config.jsbundle_config_key.clone()),
            ..Default::default()
        },
    ];

    let container = Container {
        name: "packager".to_string(),
        image: Some(config.packager_image.clone()),
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
                        "frontend-forge-extension-packager".to_string(),
                    )])),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    restart_policy: Some("Never".to_string()),
                    service_account_name: config.packager_service_account.clone(),
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

pub(crate) fn package_job_status(job: &Job) -> PackageJobStatus {
    let phase = match observed_job_phase(job.status.as_ref()) {
        ObservedJobPhase::Pending => PackageJobPhase::Pending,
        ObservedJobPhase::Running => PackageJobPhase::Running,
        ObservedJobPhase::Succeeded => PackageJobPhase::Succeeded,
        ObservedJobPhase::Failed => PackageJobPhase::Failed,
    };

    PackageJobStatus {
        namespace: job.namespace().unwrap_or_default(),
        name: job.name_any(),
        uid: job.meta().uid.clone(),
        phase,
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
        message: extract_job_message(job),
    }
}

pub(crate) fn k8s_time_to_chrono(time: &Time) -> Option<chrono::DateTime<Utc>> {
    let nanos = time.0.subsec_nanosecond();
    let nanos = u32::try_from(nanos).ok()?;
    chrono::DateTime::from_timestamp(time.0.as_second(), nanos)
}

pub(crate) fn existing_package_job(fe: &FrontendExtension) -> Option<PackageJobStatus> {
    fe.status
        .as_ref()
        .and_then(|status| status.package_job.clone())
}
