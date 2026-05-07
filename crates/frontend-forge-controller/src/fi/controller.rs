use super::*;

pub(crate) async fn run(ctx: Arc<ContextData>) -> Result<(), Error> {
    let client = ctx.client.clone();
    let fi_api = Api::<FrontendIntegration>::all(client.clone());
    let job_api = Api::<Job>::namespaced(client.clone(), &ctx.config.work_namespace);
    Controller::new(fi_api, watcher::Config::default())
        .owns(job_api, watcher::Config::default())
        .shutdown_on_signal()
        .run(reconcile, error_policy, ctx)
        .for_each(|result| async move {
            match result {
                Ok((obj_ref, action)) => info!(?obj_ref, ?action, "reconciled"),
                Err(err) => error!(error = %err, "controller reconcile stream error"),
            }
        })
        .await;

    Ok(())
}

pub(crate) fn error_policy(
    _fi: Arc<FrontendIntegration>,
    err: &Error,
    _ctx: Arc<ContextData>,
) -> Action {
    warn!(error = %err, "reconcile failed; requeueing");
    Action::requeue(Duration::from_secs(10))
}

pub(crate) async fn reconcile(
    fi: Arc<FrontendIntegration>,
    ctx: Arc<ContextData>,
) -> Result<Action, Error> {
    let fi_name = fi.name_any();
    let client = ctx.client.clone();
    let work_ns = ctx.config.work_namespace.clone();

    let fi_api = Api::<FrontendIntegration>::all(client.clone());
    let job_api = Api::<Job>::namespaced(client.clone(), &work_ns);
    let bundle_api = Api::<JSBundle>::all(client.clone());

    if fi.meta().deletion_timestamp.is_some() {
        return Ok(Action::await_change());
    }

    patch_fi_enabled_label_if_needed(&fi_api, &fi).await?;

    let spec_hash = build_spec_hash(&fi).context(CommonSnafu)?;
    info!(
        fi = %fi_name,
        spec_hash,
        phase = ?fi.status.as_ref().map(|s| &s.phase),
        "reconcile started"
    );
    let desired_bundle_name = default_bundle_name(&fi_name);

    let current_bundle = get_bundle_opt(&bundle_api, &desired_bundle_name).await?;

    if !fi.spec.enabled() {
        if let Some(bundle) = current_bundle.as_ref() {
            sync_jsbundle_enabled_state(&bundle_api, &fi, bundle, false).await?;
        }
        patch_fi_status(&fi_api, &fi, disabled_status(&fi, current_bundle.as_ref())).await?;
        return Ok(Action::await_change());
    }

    let needs_build = needs_new_build(&fi, &spec_hash, current_bundle.as_ref());
    if needs_build {
        let existing_job = find_job_for_hash(&job_api, &work_ns, &fi_name, &spec_hash).await?;
        let chosen_job = if let Some(job) = existing_job
            .filter(|j| should_reuse_build_job(&fi, j, current_bundle.as_ref(), &spec_hash))
        {
            job
        } else {
            let job_name = job_name(&fi_name, &spec_hash);
            let desired_job = make_build_job(
                &fi,
                &ctx.config,
                &job_name,
                &desired_bundle_name,
                &spec_hash,
            );
            create_or_get_job(&job_api, &work_ns, desired_job, &job_name).await?
        };

        let status = building_status(
            &fi,
            &spec_hash,
            &desired_bundle_name,
            &chosen_job,
            "Build in progress",
        );
        patch_fi_status(&fi_api, &fi, status).await?;
        return Ok(Action::requeue(Duration::from_secs(
            ctx.config.reconcile_requeue_seconds,
        )));
    }

    let action = sync_status_from_children(ChildSync {
        fi: &fi,
        fi_api: &fi_api,
        job_api: &job_api,
        bundle_api: &bundle_api,
        namespace: &work_ns,
        bundle_name: &desired_bundle_name,
        spec_hash: &spec_hash,
        requeue_seconds: ctx.config.reconcile_requeue_seconds,
    })
    .await?;

    Ok(action)
}

pub(crate) struct ChildSync<'a> {
    fi: &'a FrontendIntegration,
    fi_api: &'a Api<FrontendIntegration>,
    job_api: &'a Api<Job>,
    bundle_api: &'a Api<JSBundle>,
    namespace: &'a str,
    bundle_name: &'a str,
    spec_hash: &'a str,
    requeue_seconds: u64,
}

pub(crate) async fn sync_status_from_children(ctx: ChildSync<'_>) -> Result<Action, Error> {
    let ChildSync {
        fi,
        fi_api,
        job_api,
        bundle_api,
        namespace,
        bundle_name,
        spec_hash,
        requeue_seconds,
    } = ctx;
    let fi_name = fi.name_any();
    let current_job = find_job_for_hash(job_api, namespace, &fi_name, spec_hash).await?;

    if let Some(job) = current_job {
        match observed_job_phase(job.status.as_ref()) {
            ObservedJobPhase::Pending | ObservedJobPhase::Running => {
                let live_fi = get_live_fi(fi_api, &fi_name).await?;
                let status =
                    building_status(&live_fi, spec_hash, bundle_name, &job, "Build in progress");
                patch_fi_status(fi_api, &live_fi, status).await?;
                return Ok(Action::requeue(Duration::from_secs(requeue_seconds)));
            }
            ObservedJobPhase::Failed => {
                let live_fi = get_live_fi(fi_api, &fi_name).await?;
                let status = failed_status(
                    &live_fi,
                    spec_hash,
                    failure_error_for_status(&live_fi, spec_hash, &job),
                );
                patch_fi_status(fi_api, &live_fi, status).await?;
                return Ok(Action::await_change());
            }
            ObservedJobPhase::Succeeded => {
                let bundle = get_bundle_opt(bundle_api, bundle_name).await?;
                if let Some(bundle) = bundle {
                    if bundle_matches_spec_hash(&bundle, spec_hash) {
                        sync_jsbundle_enabled_state(bundle_api, fi, &bundle, true).await?;
                        let status = succeeded_status(fi, spec_hash, &bundle, &job);
                        patch_fi_status(fi_api, fi, status).await?;
                        return Ok(Action::await_change());
                    }
                    let status = building_status(
                        fi,
                        spec_hash,
                        bundle_name,
                        &job,
                        "Job succeeded; waiting for JSBundle with matching spec-hash",
                    );
                    patch_fi_status(fi_api, fi, status).await?;
                    return Ok(Action::requeue(Duration::from_secs(requeue_seconds)));
                }

                let status = building_status(
                    fi,
                    spec_hash,
                    bundle_name,
                    &job,
                    "Job succeeded; waiting for JSBundle materialization",
                );
                patch_fi_status(fi_api, fi, status).await?;
                return Ok(Action::requeue(Duration::from_secs(requeue_seconds)));
            }
        }
    }

    if let Some(bundle) = get_bundle_opt(bundle_api, bundle_name).await?
        && bundle_matches_spec_hash(&bundle, spec_hash)
    {
        sync_jsbundle_enabled_state(bundle_api, fi, &bundle, true).await?;
        let status = FrontendIntegrationStatus {
            phase: FrontendIntegrationPhase::Succeeded,
            observed_spec_hash: Some(spec_hash.to_string()),
            observed_manifest_hash: bundle_manifest_hash(&bundle),
            observed_generation: Some(fi.metadata.generation.unwrap_or_default()),
            last_build: fi.status.as_ref().and_then(|s| s.last_build.clone()),
            bundle_ref: Some(resource_ref(&bundle)),
            last_error: None,
            message: Some("JSBundle ready".to_string()),
            conditions: vec![],
        };
        patch_fi_status(fi_api, fi, status).await?;
    }

    Ok(Action::await_change())
}

pub(crate) async fn get_live_fi(
    fi_api: &Api<FrontendIntegration>,
    fi_name: &str,
) -> Result<FrontendIntegration, Error> {
    fi_api
        .get(fi_name)
        .await
        .with_context(|_| GetFrontendIntegrationSnafu {
            namespace: "<cluster>".to_string(),
            name: fi_name.to_string(),
        })
}
