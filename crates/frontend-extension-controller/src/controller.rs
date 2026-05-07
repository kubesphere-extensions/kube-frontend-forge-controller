use super::*;

pub(crate) async fn run(ctx: Arc<ContextData>) -> Result<(), Error> {
    let client = ctx.client.clone();
    let fe_api = Api::<FrontendExtension>::all(client.clone());
    let job_api = Api::<Job>::namespaced(client.clone(), &ctx.config.work_namespace);
    let artifact_api =
        Api::<ConfigMap>::namespaced(client.clone(), &ctx.config.artifact_configmap_namespace);

    Controller::new(fe_api, watcher::Config::default())
        .owns(job_api, watcher::Config::default())
        .owns(artifact_api, watcher::Config::default())
        .shutdown_on_signal()
        .run(reconcile, error_policy, ctx)
        .for_each(|result| async move {
            match result {
                Ok((obj_ref, action)) => info!(?obj_ref, ?action, "reconciled"),
                Err(err) => error!(error = %err, "frontend extension reconcile stream error"),
            }
        })
        .await;

    Ok(())
}

pub(crate) fn error_policy(
    _fe: Arc<FrontendExtension>,
    err: &Error,
    _ctx: Arc<ContextData>,
) -> Action {
    warn!(error = %err, "frontend extension reconcile failed; requeueing");
    Action::requeue(Duration::from_secs(10))
}

pub(crate) async fn reconcile(
    fe: Arc<FrontendExtension>,
    ctx: Arc<ContextData>,
) -> Result<Action, Error> {
    let fe_name = fe.name_any();
    let client = ctx.client.clone();
    let work_ns = ctx.config.work_namespace.clone();
    let artifact_ns = ctx.config.artifact_configmap_namespace.clone();

    let fe_api = Api::<FrontendExtension>::all(client.clone());
    let job_api = Api::<Job>::namespaced(client.clone(), &work_ns);
    let artifact_api = Api::<ConfigMap>::namespaced(client.clone(), &artifact_ns);

    if fe.meta().deletion_timestamp.is_some() {
        return Ok(Action::await_change());
    }

    let source_hash =
        frontend_extension_source_hash(&fe).context(FrontendExtensionSourceHashSnafu)?;
    let rebuild_token = frontend_extension_rebuild_token(&fe);
    let artifact_key =
        artifact_key(&source_hash, &rebuild_token).context(FrontendExtensionArtifactKeySnafu)?;
    let package_name = frontend_extension_package_name(&fe);
    let artifact_name = artifact_configmap_name(&package_name, &artifact_key);
    let package_attempts =
        list_package_jobs_for_artifact_key(&job_api, &work_ns, &fe, &artifact_key).await?;
    let latest_attempt = latest_package_attempt(package_attempts);

    info!(
        fe = %fe_name,
        source_hash = %source_hash,
        artifact_key = %artifact_key,
        rebuild_token = %rebuild_token,
        phase = ?fe.status.as_ref().map(|s| &s.phase),
        "frontend extension reconcile started"
    );

    if let Err(err) = validate_frontend_extension(&fe) {
        patch_fe_status(
            &fe_api,
            &fe,
            failed_fe_status(
                &fe,
                &source_hash,
                &rebuild_token,
                &artifact_key,
                None,
                "InvalidSource",
                &err.to_string(),
            ),
        )
        .await?;
        return Ok(Action::await_change());
    }

    if let Some(cm) =
        get_artifact_configmap_opt(&artifact_api, &artifact_ns, &artifact_name).await?
        && let Some(metadata) = artifact_metadata_from_configmap(&cm, &source_hash, &artifact_key)
    {
        let gc_keep_names = artifact_gc_keep_names(&fe, &cm);
        let publish = sync_publish(
            &fe,
            &job_api,
            &work_ns,
            &ctx.config,
            &artifact_key,
            &metadata,
            &cm,
        )
        .await?;
        let unpublish = sync_unpublish(&fe, &job_api, &work_ns, &ctx.config).await?;
        let mut status = ready_fe_status(
            &fe,
            &source_hash,
            &rebuild_token,
            &artifact_key,
            &cm,
            metadata,
            current_or_existing_package_job(latest_attempt.as_ref().map(|a| &a.job), &fe),
        );
        apply_publish_sync(&mut status, &publish);
        apply_unpublish_sync(&mut status, &unpublish);
        patch_fe_status(&fe_api, &fe, status).await?;
        if should_delete_after_unpublish(&fe, &unpublish) {
            delete_frontend_extension(&fe_api, &fe_name).await?;
            return Ok(Action::await_change());
        }
        gc_artifact_configmaps(
            &artifact_api,
            &artifact_ns,
            &fe,
            &gc_keep_names,
            ctx.config.artifact_retain_old_count,
        )
        .await?;
        return Ok(requeue_if_publish_or_unpublish_running(
            &publish,
            &unpublish,
            ctx.config.reconcile_requeue_seconds,
        ));
    }

    if let Some(attempt) = latest_attempt.as_ref() {
        let job = &attempt.job;
        match observed_job_phase(job.status.as_ref()) {
            ObservedJobPhase::Pending | ObservedJobPhase::Running => {
                patch_fe_status(
                    &fe_api,
                    &fe,
                    packaging_fe_status(
                        &fe,
                        &source_hash,
                        &rebuild_token,
                        &artifact_key,
                        job,
                        "Package job in progress",
                    ),
                )
                .await?;
                return Ok(Action::requeue(Duration::from_secs(
                    ctx.config.reconcile_requeue_seconds,
                )));
            }
            ObservedJobPhase::Failed => {
                if attempt.attempt >= ctx.config.package_max_attempts {
                    let latest_message = extract_job_message(job)
                        .unwrap_or_else(|| "Package job failed".to_string());
                    let message = package_attempts_exceeded_message(
                        &artifact_key,
                        attempt.attempt,
                        ctx.config.package_max_attempts,
                        &latest_message,
                    );
                    patch_fe_status(
                        &fe_api,
                        &fe,
                        failed_fe_status(
                            &fe,
                            &source_hash,
                            &rebuild_token,
                            &artifact_key,
                            Some(job),
                            "PackageAttemptsExceeded",
                            &message,
                        ),
                    )
                    .await?;
                    return Ok(Action::await_change());
                }
                let message =
                    extract_job_message(job).unwrap_or_else(|| "Package job failed".to_string());
                info!(
                    fe = %fe_name,
                    attempt = attempt.attempt,
                    max_attempts = ctx.config.package_max_attempts,
                    message,
                    "package job failed; creating next attempt"
                );
            }
            ObservedJobPhase::Succeeded => {
                if let Some(cm) =
                    get_artifact_configmap_opt(&artifact_api, &artifact_ns, &artifact_name).await?
                    && let Some(metadata) =
                        artifact_metadata_from_configmap(&cm, &source_hash, &artifact_key)
                {
                    let gc_keep_names = artifact_gc_keep_names(&fe, &cm);
                    let publish = sync_publish(
                        &fe,
                        &job_api,
                        &work_ns,
                        &ctx.config,
                        &artifact_key,
                        &metadata,
                        &cm,
                    )
                    .await?;
                    let unpublish = sync_unpublish(&fe, &job_api, &work_ns, &ctx.config).await?;
                    let mut status = ready_fe_status(
                        &fe,
                        &source_hash,
                        &rebuild_token,
                        &artifact_key,
                        &cm,
                        metadata,
                        Some(package_job_status(job)),
                    );
                    apply_publish_sync(&mut status, &publish);
                    apply_unpublish_sync(&mut status, &unpublish);
                    patch_fe_status(&fe_api, &fe, status).await?;
                    if should_delete_after_unpublish(&fe, &unpublish) {
                        delete_frontend_extension(&fe_api, &fe_name).await?;
                        return Ok(Action::await_change());
                    }
                    gc_artifact_configmaps(
                        &artifact_api,
                        &artifact_ns,
                        &fe,
                        &gc_keep_names,
                        ctx.config.artifact_retain_old_count,
                    )
                    .await?;
                    return Ok(requeue_if_publish_or_unpublish_running(
                        &publish,
                        &unpublish,
                        ctx.config.reconcile_requeue_seconds,
                    ));
                }

                if attempt.attempt >= ctx.config.package_max_attempts {
                    let latest_message =
                        "Package job succeeded but artifact ConfigMap is missing or mismatched";
                    let message = package_attempts_exceeded_message(
                        &artifact_key,
                        attempt.attempt,
                        ctx.config.package_max_attempts,
                        latest_message,
                    );
                    patch_fe_status(
                        &fe_api,
                        &fe,
                        failed_fe_status(
                            &fe,
                            &source_hash,
                            &rebuild_token,
                            &artifact_key,
                            Some(job),
                            "PackageAttemptsExceeded",
                            &message,
                        ),
                    )
                    .await?;
                    return Ok(Action::await_change());
                }
                info!(
                    fe = %fe_name,
                    attempt = attempt.attempt,
                    max_attempts = ctx.config.package_max_attempts,
                    "package job succeeded without matching artifact; creating next attempt"
                );
            }
        }
    }

    let next_attempt = latest_attempt
        .as_ref()
        .map_or(1, |attempt| attempt.attempt.saturating_add(1));
    if next_attempt > ctx.config.package_max_attempts {
        let latest_message = latest_attempt
            .as_ref()
            .and_then(|attempt| extract_job_message(&attempt.job))
            .unwrap_or_else(|| "No package attempts can be created".to_string());
        let message = package_attempts_exceeded_message(
            &artifact_key,
            next_attempt.saturating_sub(1),
            ctx.config.package_max_attempts,
            &latest_message,
        );
        patch_fe_status(
            &fe_api,
            &fe,
            failed_fe_status(
                &fe,
                &source_hash,
                &rebuild_token,
                &artifact_key,
                latest_attempt.as_ref().map(|attempt| &attempt.job),
                "PackageAttemptsExceeded",
                &message,
            ),
        )
        .await?;
        return Ok(Action::await_change());
    }

    let job_name = package_job_name(&fe_name, &artifact_key, next_attempt);
    let desired_job = make_package_job(
        &fe,
        &ctx.config,
        &job_name,
        &source_hash,
        &artifact_key,
        &rebuild_token,
        &artifact_name,
    );
    let job = create_or_get_job(&job_api, &work_ns, desired_job, &job_name).await?;
    patch_fe_status(
        &fe_api,
        &fe,
        packaging_fe_status(
            &fe,
            &source_hash,
            &rebuild_token,
            &artifact_key,
            &job,
            "Package job created",
        ),
    )
    .await?;
    Ok(Action::requeue(Duration::from_secs(
        ctx.config.reconcile_requeue_seconds,
    )))
}
