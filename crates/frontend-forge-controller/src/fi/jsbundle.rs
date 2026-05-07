use super::*;

pub(crate) async fn get_bundle_opt(
    bundle_api: &Api<JSBundle>,
    name: &str,
) -> Result<Option<JSBundle>, Error> {
    bundle_api
        .get_opt(name)
        .await
        .with_context(|_| GetJsBundleSnafu {
            namespace: "<cluster>".to_string(),
            name: name.to_string(),
        })
}

pub(crate) const fn enabled_label_value(enabled: bool) -> &'static str {
    if enabled { "true" } else { "false" }
}

pub(crate) async fn patch_fi_enabled_label_if_needed(
    fi_api: &Api<FrontendIntegration>,
    fi: &FrontendIntegration,
) -> Result<(), Error> {
    let desired = enabled_label_value(fi.spec.enabled());
    let current = fi
        .metadata
        .labels
        .as_ref()
        .and_then(|labels| labels.get(LABEL_ENABLED))
        .map(String::as_str);
    if current == Some(desired) {
        return Ok(());
    }

    let fi_name = fi.name_any();
    let namespace = fi.namespace().unwrap_or_else(|| "<cluster>".to_string());
    let patch = json!({
        "metadata": {
            "labels": {
                LABEL_ENABLED: desired,
            }
        }
    });
    fi_api
        .patch(&fi_name, &PatchParams::default(), &Patch::Merge(&patch))
        .await
        .with_context(|_| PatchFrontendIntegrationMetadataSnafu {
            namespace,
            name: fi_name.clone(),
        })?;
    Ok(())
}

pub(crate) async fn sync_jsbundle_enabled_state(
    bundle_api: &Api<JSBundle>,
    fi: &FrontendIntegration,
    bundle: &JSBundle,
    enabled: bool,
) -> Result<(), Error> {
    patch_jsbundle_owner_ref_if_needed(bundle_api, fi, bundle).await?;
    patch_jsbundle_enabled_label_if_needed(bundle_api, bundle, enabled).await?;
    let desired_state = if enabled {
        JSBUNDLE_STATE_AVAILABLE
    } else {
        JSBUNDLE_STATE_DISABLED
    };
    patch_jsbundle_state_if_needed(bundle_api, bundle, desired_state).await?;
    Ok(())
}

pub(crate) async fn patch_jsbundle_owner_ref_if_needed(
    bundle_api: &Api<JSBundle>,
    fi: &FrontendIntegration,
    bundle: &JSBundle,
) -> Result<(), Error> {
    let Some(owner_ref) = base_owner_ref(fi) else {
        return Ok(());
    };

    let mut owners = bundle.metadata.owner_references.clone().unwrap_or_default();
    if owners.iter().any(|owner| owner.uid == owner_ref.uid) {
        return Ok(());
    }
    owners.push(owner_ref);

    let name = bundle.name_any();
    let patch = json!({
        "metadata": {
            "ownerReferences": owners,
        }
    });
    match bundle_api
        .patch(&name, &PatchParams::default(), &Patch::Merge(&patch))
        .await
    {
        Ok(_) => Ok(()),
        Err(kube::Error::Api(ae)) if ae.code == 404 => Ok(()),
        Err(source) => Err(Error::PatchJsBundle {
            namespace: "<cluster>".to_string(),
            name,
            source: Box::new(source),
        }),
    }
}

pub(crate) async fn patch_jsbundle_enabled_label_if_needed(
    bundle_api: &Api<JSBundle>,
    bundle: &JSBundle,
    enabled: bool,
) -> Result<(), Error> {
    let desired = enabled_label_value(enabled);
    let current = bundle
        .metadata
        .labels
        .as_ref()
        .and_then(|labels| labels.get(LABEL_ENABLED))
        .map(String::as_str);
    if current == Some(desired) {
        return Ok(());
    }

    let name = bundle.name_any();
    let patch = json!({
        "metadata": {
            "labels": {
                LABEL_ENABLED: desired,
            }
        }
    });
    match bundle_api
        .patch(&name, &PatchParams::default(), &Patch::Merge(&patch))
        .await
    {
        Ok(_) => Ok(()),
        Err(kube::Error::Api(ae)) if ae.code == 404 => Ok(()),
        Err(source) => Err(Error::PatchJsBundle {
            namespace: "<cluster>".to_string(),
            name,
            source: Box::new(source),
        }),
    }
}

pub(crate) async fn patch_jsbundle_state_if_needed(
    bundle_api: &Api<JSBundle>,
    bundle: &JSBundle,
    desired_state: &str,
) -> Result<(), Error> {
    let current = bundle
        .status
        .as_ref()
        .and_then(|status| status.state.as_deref());
    if current == Some(desired_state) {
        return Ok(());
    }

    let name = bundle.name_any();
    let patch = json!({
        "status": {
            "state": desired_state,
        }
    });
    match bundle_api
        .patch_status(&name, &PatchParams::default(), &Patch::Merge(&patch))
        .await
    {
        Ok(_) => Ok(()),
        Err(kube::Error::Api(ae)) if ae.code == 404 => {
            match bundle_api
                .patch(&name, &PatchParams::default(), &Patch::Merge(&patch))
                .await
            {
                Ok(_) => Ok(()),
                Err(kube::Error::Api(ae)) if ae.code == 404 => Ok(()),
                Err(source) => Err(Error::PatchJsBundle {
                    namespace: "<cluster>".to_string(),
                    name,
                    source: Box::new(source),
                }),
            }
        }
        Err(source) => Err(Error::PatchJsBundle {
            namespace: "<cluster>".to_string(),
            name,
            source: Box::new(source),
        }),
    }
}

pub(crate) fn resource_ref<K: ResourceExt>(obj: &K) -> ResourceRef {
    ResourceRef {
        name: obj.name_any(),
        namespace: obj.namespace(),
        uid: obj.meta().uid.clone(),
    }
}
