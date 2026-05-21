use super::*;

pub(crate) async fn wait_for_prerequisites(client: &Client, cfg: &MigratorConfig) -> Result<()> {
    let deadline = Instant::now() + cfg.ready_timeout;
    loop {
        let fi = frontend_integration_crd_ready(client).await;
        let fe = frontend_extension_crd_ready(client).await;

        match (&fi, &fe) {
            (Ok(()), Ok(())) => {
                info!("migration prerequisites are ready");
                return Ok(());
            }
            _ if Instant::now() >= deadline => {
                return Err(Error::Message {
                    message: format!(
                        "timed out waiting for migration prerequisites: fi={:?}; fe={:?}",
                        fi.err(),
                        fe.err(),
                    ),
                });
            }
            _ => {
                warn!(
                    fi_ready = fi.is_ok(),
                    fe_ready = fe.is_ok(),
                    "waiting for migration prerequisites"
                );
                sleep(cfg.poll_interval).await;
            }
        }
    }
}

pub(crate) async fn frontend_integration_crd_ready(client: &Client) -> Result<()> {
    crd_ready(
        client,
        "frontendintegrations.frontend-forge.kubesphere.io",
        false,
    )
    .await
}

pub(crate) async fn frontend_extension_crd_ready(client: &Client) -> Result<()> {
    crd_ready(
        client,
        "frontendextensions.frontend-forge.kubesphere.io",
        true,
    )
    .await
}

pub(crate) async fn crd_ready(
    client: &Client,
    name: &str,
    require_status_subresource: bool,
) -> Result<()> {
    let api = Api::<CustomResourceDefinition>::all(client.clone());
    let crd = api.get(name).await.map_err(|source| Error::Kube {
        action: format!("getting CRD {name}"),
        source: Box::new(source),
    })?;
    let value = serde_json::to_value(&crd).map_err(|source| Error::Message {
        message: format!("failed to serialize CRD {name}: {source}"),
    })?;

    if !crd_established(&value) {
        return Err(Error::Message {
            message: format!("CRD {name} is not Established"),
        });
    }
    if require_status_subresource && !crd_has_v1alpha1_status_subresource(&value) {
        return Err(Error::Message {
            message: format!("CRD {name} does not serve v1alpha1 status subresource"),
        });
    }
    Ok(())
}
pub(crate) fn crd_established(value: &Value) -> bool {
    value
        .pointer("/status/conditions")
        .and_then(Value::as_array)
        .is_some_and(|conditions| {
            conditions.iter().any(|condition| {
                condition.get("type").and_then(Value::as_str) == Some("Established")
                    && condition.get("status").and_then(Value::as_str) == Some("True")
            })
        })
}

pub(crate) fn crd_has_v1alpha1_status_subresource(value: &Value) -> bool {
    value
        .pointer("/spec/versions")
        .and_then(Value::as_array)
        .is_some_and(|versions| {
            versions.iter().any(|version| {
                version.get("name").and_then(Value::as_str) == Some("v1alpha1")
                    && version.get("served").and_then(Value::as_bool) == Some(true)
                    && version.pointer("/subresources/status").is_some()
            })
        })
}
