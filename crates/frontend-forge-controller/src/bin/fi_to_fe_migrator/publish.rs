use super::*;

pub(crate) async fn patch_publish_intent(
    fe_api: &Api<FrontendExtension>,
    cfg: &MigratorConfig,
    fe: &FrontendExtension,
) -> Result<()> {
    let fe_name = fe.name_any();
    let source_hash = frontend_extension_source_hash(fe).map_err(|err| Error::Message {
        message: format!("failed to hash FrontendExtension {fe_name} source: {err}"),
    })?;
    let generation = fe.metadata.generation.ok_or_else(|| Error::Message {
        message: format!("FrontendExtension {fe_name} has no metadata.generation after upsert"),
    })?;
    let request_id = publish_request_id(&fe_name, &source_hash);
    let patch = publish_intent_patch(cfg, generation, &source_hash, &request_id);

    fe_api
        .patch(&fe_name, &PatchParams::default(), &Patch::Merge(&patch))
        .await
        .map_err(|source| Error::Kube {
            action: format!("patching FrontendExtension {fe_name} publish intent"),
            source: Box::new(source),
        })?;

    info!(
        fe = %fe_name,
        %request_id,
        generation,
        source_hash = %source_hash,
        "patched FE publish intent"
    );
    Ok(())
}

pub(crate) fn publish_intent_patch(
    cfg: &MigratorConfig,
    generation: i64,
    source_hash: &str,
    request_id: &str,
) -> serde_json::Value {
    json!({
        "metadata": {
            "annotations": {
                ANNO_PUBLISH_REQUEST_ID: request_id,
                ANNO_PUBLISH_REQUEST_GENERATION: generation.to_string(),
                ANNO_PUBLISH_REQUEST_SOURCE_HASH: source_hash,
                ANNO_PUBLISH_ARTIFACT_DIGEST: serde_json::Value::Null,
                ANNO_PUBLISH_TARGET_KIND: publish_target_kind_value(&cfg.publish_target_kind),
                ANNO_PUBLISH_TARGET_NAMESPACE: cfg.publish_target_namespace,
                ANNO_PUBLISH_TARGET_NAME: cfg.publish_target_name,
            }
        }
    })
}

pub(crate) fn publish_target_kind_value(kind: &PublishTargetKind) -> &'static str {
    match kind {
        PublishTargetKind::ConfigMap => "ConfigMap",
        PublishTargetKind::Secret => "Secret",
    }
}

pub(crate) fn publish_request_id(fe_name: &str, source_hash: &str) -> String {
    let source = source_hash
        .strip_prefix("sha256:")
        .unwrap_or(source_hash)
        .chars()
        .take(12)
        .collect::<String>();
    format!("fi-migration-{fe_name}-{source}")
}
