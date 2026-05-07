use super::*;

pub(crate) async fn run(client: Client, http: reqwest::Client, cfg: MigratorConfig) -> Result<()> {
    wait_for_prerequisites(&client, &cfg).await?;

    let fi_api = Api::<FrontendIntegration>::all(client.clone());
    let items = fi_api
        .list(&Default::default())
        .await
        .map_err(|source| Error::Kube {
            action: "listing FrontendIntegrations".to_string(),
            source: Box::new(source),
        })?
        .items;

    info!(count = items.len(), "starting FI to FE migration");
    let mut failures = Vec::new();

    for fi in items {
        let fi_name = fi.name_any();
        if let Err(err) = migrate_one(&client, &http, &cfg, fi).await {
            error!(fi = %fi_name, error = %err, "FI migration failed");
            failures.push(format!("{fi_name}: {err}"));
        }
    }

    if failures.is_empty() {
        info!("FI to FE migration completed");
        Ok(())
    } else {
        Err(Error::Message {
            message: format!(
                "FI to FE migration completed with {} failure(s): {}",
                failures.len(),
                failures.join("; ")
            ),
        })
    }
}

pub(crate) async fn migrate_one(
    client: &Client,
    http: &reqwest::Client,
    cfg: &MigratorConfig,
    fi: FrontendIntegration,
) -> Result<()> {
    let fi_name = fi.name_any();
    let fe_name = migrated_fe_name(&fi_name);
    let was_enabled = fi.spec.enabled();
    info!(fi = %fi_name, fe = %fe_name, was_enabled, "migrating FI");

    let fe_api = Api::<FrontendExtension>::all(client.clone());
    upsert_managed_fe(&fe_api, cfg, &fi, &fe_name).await?;
    let artifact_digest = wait_for_fe_ready(&fe_api, &fe_name, cfg).await?;
    delete_fi_and_wait(client, &fi_name, cfg).await?;

    if was_enabled {
        publish_fe(http, cfg, &fe_name, &artifact_digest).await?;
    } else {
        info!(fi = %fi_name, fe = %fe_name, "source FI was disabled; skipping publish");
    }

    Ok(())
}
pub(crate) async fn upsert_managed_fe(
    fe_api: &Api<FrontendExtension>,
    cfg: &MigratorConfig,
    fi: &FrontendIntegration,
    fe_name: &str,
) -> Result<()> {
    let desired = frontend_extension_from_fi(fi, fe_name, cfg);
    match fe_api
        .get_opt(fe_name)
        .await
        .map_err(|source| Error::Kube {
            action: format!("getting FrontendExtension {fe_name}"),
            source: Box::new(source),
        })? {
        None => {
            fe_api
                .create(&PostParams::default(), &desired)
                .await
                .map_err(|source| Error::Kube {
                    action: format!("creating FrontendExtension {fe_name}"),
                    source: Box::new(source),
                })?;
            info!(fe = %fe_name, "created migrated FrontendExtension");
        }
        Some(existing) => {
            ensure_existing_fe_is_managed_by_fi(&existing, fi)?;
            let patch = json!({
                "metadata": {
                    "labels": desired.metadata.labels,
                    "annotations": desired.metadata.annotations,
                },
                "spec": desired.spec,
            });
            fe_api
                .patch(fe_name, &PatchParams::default(), &Patch::Merge(&patch))
                .await
                .map_err(|source| Error::Kube {
                    action: format!("patching FrontendExtension {fe_name}"),
                    source: Box::new(source),
                })?;
            info!(fe = %fe_name, "patched migrated FrontendExtension");
        }
    }
    Ok(())
}

pub(crate) fn ensure_existing_fe_is_managed_by_fi(
    fe: &FrontendExtension,
    fi: &FrontendIntegration,
) -> Result<()> {
    let fe_name = fe.name_any();
    let fi_name = fi.name_any();
    let managed_by = fe
        .metadata
        .labels
        .as_ref()
        .and_then(|labels| labels.get(LABEL_MANAGED_BY))
        .map(String::as_str);
    if managed_by != Some(MANAGED_BY_VALUE) {
        return Err(Error::Message {
            message: format!(
                "FrontendExtension {fe_name} already exists and is not migrator-owned"
            ),
        });
    }

    let source_fi_name = fe
        .metadata
        .annotations
        .as_ref()
        .and_then(|annos| annos.get(ANNO_SOURCE_FI_NAME))
        .map(String::as_str);
    if source_fi_name != Some(fi_name.as_str()) {
        return Err(Error::Message {
            message: format!(
                "FrontendExtension {fe_name} is migrator-owned but points to source FI {:?}",
                source_fi_name
            ),
        });
    }
    Ok(())
}

pub(crate) async fn wait_for_fe_ready(
    fe_api: &Api<FrontendExtension>,
    fe_name: &str,
    cfg: &MigratorConfig,
) -> Result<String> {
    let deadline = Instant::now() + cfg.ready_timeout;
    loop {
        let fe = fe_api.get(fe_name).await.map_err(|source| Error::Kube {
            action: format!("getting FrontendExtension {fe_name} while waiting for Ready"),
            source: Box::new(source),
        })?;
        let phase = fe.status.as_ref().map(|status| status.phase.clone());
        let digest = fe
            .status
            .as_ref()
            .and_then(|status| status.artifact.as_ref())
            .map(|artifact| artifact.digest.clone())
            .filter(|digest| !digest.is_empty());
        if phase == Some(FrontendExtensionPhase::Ready)
            && let Some(digest) = digest
        {
            info!(fe = %fe_name, artifact_digest = %digest, "FrontendExtension is Ready");
            return Ok(digest);
        }
        if phase == Some(FrontendExtensionPhase::Failed) {
            return Err(Error::Message {
                message: format!("FrontendExtension {fe_name} reached Failed phase"),
            });
        }
        if Instant::now() >= deadline {
            return Err(Error::Message {
                message: format!(
                    "timed out waiting for FrontendExtension {fe_name} to become Ready"
                ),
            });
        }
        info!(
            fe = %fe_name,
            phase = ?phase,
            "waiting for FrontendExtension package Ready"
        );
        sleep(cfg.poll_interval).await;
    }
}

pub(crate) async fn delete_fi_and_wait(
    client: &Client,
    fi_name: &str,
    cfg: &MigratorConfig,
) -> Result<()> {
    let fi_api = Api::<FrontendIntegration>::all(client.clone());
    match fi_api.delete(fi_name, &DeleteParams::default()).await {
        Ok(_) => {}
        Err(kube::Error::Api(ae)) if ae.code == 404 => {}
        Err(source) => {
            return Err(Error::Kube {
                action: format!("deleting FrontendIntegration {fi_name}"),
                source: Box::new(source),
            });
        }
    }

    let deadline = Instant::now() + cfg.ready_timeout;
    loop {
        match fi_api.get_opt(fi_name).await {
            Ok(None) => {
                info!(fi = %fi_name, "FrontendIntegration deleted");
                return Ok(());
            }
            Ok(Some(_)) if Instant::now() < deadline => {
                sleep(cfg.poll_interval).await;
            }
            Ok(Some(_)) => {
                return Err(Error::Message {
                    message: format!(
                        "timed out waiting for FrontendIntegration {fi_name} deletion"
                    ),
                });
            }
            Err(source) => {
                return Err(Error::Kube {
                    action: format!("checking FrontendIntegration {fi_name} deletion"),
                    source: Box::new(source),
                });
            }
        }
    }
}
