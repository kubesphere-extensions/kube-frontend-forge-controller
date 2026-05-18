use super::*;

pub(crate) async fn run(client: Client, cfg: MigratorConfig) -> Result<()> {
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
        if let Err(err) = migrate_one(&client, &cfg, fi).await {
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
    cfg: &MigratorConfig,
    fi: FrontendIntegration,
) -> Result<()> {
    let fi_name = fi.name_any();
    let fe_name = migrated_fe_name(&fi_name);
    let was_enabled = fi.spec.enabled();
    info!(fi = %fi_name, fe = %fe_name, was_enabled, "migrating FI");

    let fe_api = Api::<FrontendExtension>::all(client.clone());
    let fe = upsert_managed_fe(&fe_api, cfg, &fi, &fe_name).await?;
    delete_fi_and_wait(client, &fi_name, cfg).await?;

    if was_enabled {
        patch_publish_intent(&fe_api, cfg, &fe).await?;
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
) -> Result<FrontendExtension> {
    let desired = frontend_extension_from_fi(fi, fe_name, cfg);
    match fe_api
        .get_opt(fe_name)
        .await
        .map_err(|source| Error::Kube {
            action: format!("getting FrontendExtension {fe_name}"),
            source: Box::new(source),
        })? {
        None => {
            let fe = fe_api
                .create(&PostParams::default(), &desired)
                .await
                .map_err(|source| Error::Kube {
                    action: format!("creating FrontendExtension {fe_name}"),
                    source: Box::new(source),
                })?;
            info!(fe = %fe_name, "created migrated FrontendExtension");
            Ok(fe)
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
            let fe = fe_api
                .patch(fe_name, &PatchParams::default(), &Patch::Merge(&patch))
                .await
                .map_err(|source| Error::Kube {
                    action: format!("patching FrontendExtension {fe_name}"),
                    source: Box::new(source),
                })?;
            info!(fe = %fe_name, "patched migrated FrontendExtension");
            Ok(fe)
        }
    }
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
