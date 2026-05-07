use super::*;

pub(crate) async fn get_artifact_configmap_opt(
    cm_api: &Api<ConfigMap>,
    namespace: &str,
    name: &str,
) -> Result<Option<ConfigMap>, Error> {
    cm_api
        .get_opt(name)
        .await
        .with_context(|_| GetArtifactConfigMapSnafu {
            namespace: namespace.to_string(),
            name: name.to_string(),
        })
}

pub(crate) async fn gc_artifact_configmaps(
    cm_api: &Api<ConfigMap>,
    namespace: &str,
    fe: &FrontendExtension,
    keep_names: &BTreeSet<String>,
    retain_old_count: usize,
) -> Result<(), Error> {
    let selector = format!(
        "{}={},{}={}",
        LABEL_FE_NAME,
        fe.name_any(),
        LABEL_PACKAGE_KIND,
        PACKAGE_KIND_VALUE
    );
    let configmaps = cm_api
        .list(&ListParams::default().labels(&selector))
        .await
        .with_context(|_| ListArtifactConfigMapsForGcSnafu {
            namespace: namespace.to_string(),
        })?;

    let delete_names =
        artifact_configmap_gc_candidates(configmaps.items, fe, keep_names, retain_old_count);
    for name in delete_names {
        match cm_api.delete(&name, &DeleteParams::default()).await {
            Ok(_) => {
                info!(
                    fe = %fe.name_any(),
                    namespace,
                    configmap = %name,
                    "deleted stale FrontendExtension artifact ConfigMap"
                );
            }
            Err(kube::Error::Api(ae)) if ae.code == 404 => {}
            Err(source) => {
                return Err(Error::DeleteArtifactConfigMap {
                    namespace: namespace.to_string(),
                    name,
                    source: Box::new(source),
                });
            }
        }
    }

    Ok(())
}

pub(crate) fn artifact_gc_keep_names(
    fe: &FrontendExtension,
    current_cm: &ConfigMap,
) -> BTreeSet<String> {
    let mut keep_names = BTreeSet::from([current_cm.name_any()]);
    if let Some(name) = status_artifact_configmap_name(fe) {
        keep_names.insert(name.to_string());
    }
    keep_names
}

pub(crate) fn status_artifact_configmap_name(fe: &FrontendExtension) -> Option<&str> {
    let artifact = fe.status.as_ref()?.artifact.as_ref()?;
    if artifact.storage.kind != ArtifactStorageKind::ConfigMap {
        return None;
    }
    Some(artifact.storage.ref_.name.as_str())
}

pub(crate) fn artifact_configmap_gc_candidates(
    configmaps: Vec<ConfigMap>,
    fe: &FrontendExtension,
    keep_names: &BTreeSet<String>,
    retain_old_count: usize,
) -> Vec<String> {
    let mut candidates = configmaps
        .into_iter()
        .filter(|cm| artifact_configmap_is_owned_by(cm, fe))
        .filter(|cm| !keep_names.contains(&cm.name_any()))
        .collect::<Vec<_>>();

    candidates.sort_by(|a, b| {
        b.metadata
            .creation_timestamp
            .cmp(&a.metadata.creation_timestamp)
            .then_with(|| b.name_any().cmp(&a.name_any()))
    });

    candidates
        .into_iter()
        .skip(retain_old_count)
        .map(|cm| cm.name_any())
        .collect()
}

pub(crate) fn artifact_configmap_is_owned_by(cm: &ConfigMap, fe: &FrontendExtension) -> bool {
    let Some(fe_uid) = fe.meta().uid.as_deref() else {
        return false;
    };
    cm.metadata
        .owner_references
        .as_ref()
        .is_some_and(|owners| owners.iter().any(|owner| owner.uid == fe_uid))
}

pub(crate) fn artifact_metadata_from_configmap(
    cm: &ConfigMap,
    source_hash: &str,
    artifact_key: &str,
) -> Option<PackageArtifactMetadata> {
    let annotations = cm.metadata.annotations.as_ref()?;
    if annotations.get(ANNO_SOURCE_HASH).map(String::as_str) != Some(source_hash) {
        return None;
    }
    if annotations.get(ANNO_ARTIFACT_KEY).map(String::as_str) != Some(artifact_key) {
        return None;
    }

    let metadata_content = cm
        .data
        .as_ref()
        .and_then(|data| data.get(ARTIFACT_METADATA_KEY))?;
    let metadata: PackageArtifactMetadata = serde_json::from_str(metadata_content).ok()?;
    if metadata.source_hash != source_hash {
        return None;
    }

    let bytes = cm
        .binary_data
        .as_ref()
        .and_then(|binary_data| binary_data.get(PACKAGE_KEY))?;
    let observed_digest = format!("sha256:{}", sha256_hex(&bytes.0));
    if observed_digest != metadata.digest {
        return None;
    }
    if annotations.get(ANNO_ARTIFACT_DIGEST).map(String::as_str) != Some(metadata.digest.as_str()) {
        return None;
    }

    Some(metadata)
}
