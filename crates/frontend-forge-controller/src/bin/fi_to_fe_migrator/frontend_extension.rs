use super::*;

pub(crate) fn frontend_extension_from_fi(
    fi: &FrontendIntegration,
    fe_name: &str,
    cfg: &MigratorConfig,
) -> FrontendExtension {
    let display_name = fi_display_name(fi);
    let description = fi_description(fi, &display_name);
    let schema_version = fi
        .spec
        .engine_version()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| cfg.schema_version.clone());
    let mut fe = FrontendExtension::new(
        fe_name,
        FrontendExtensionSpec {
            package: FrontendExtensionPackageSpec {
                name: Some(fe_name.to_string()),
                version: cfg.package_version.clone(),
                display_name: localized_map(display_name),
                description: localized_map(description),
                category: Some(DEFAULT_PACKAGE_CATEGORY.to_string()),
                keywords: Vec::new(),
                sources: Vec::new(),
                kube_version: None,
                ks_version: None,
                maintainers: Vec::new(),
                home: None,
                provider: default_provider(fi),
                icon: Some(DEFAULT_PACKAGE_ICON.to_string()),
                static_file_directory: None,
                dependencies: None,
                installation_mode: None,
                images: Vec::new(),
                charts: None,
            },
            source: FrontendExtensionSourceSpec {
                type_: FrontendExtensionSourceType::Inline,
                inline: InlineFrontendExtensionSourceSpec {
                    schema_version,
                    frontend: FrontendExtensionFrontendSpec {
                        display_name: fi.spec.display_name.clone(),
                        locales: fi.spec.locales.clone(),
                        menus: fi
                            .spec
                            .menus
                            .iter()
                            .map(FrontendExtensionPrimaryMenuSpec::from)
                            .collect(),
                        pages: frontend_pages_from_fi(fi),
                    },
                    extension_resources: None,
                },
            },
            publish_policy: Some(PublishPolicySpec {
                mode: PublishPolicyMode::Manual,
                default_target_kind: Some(cfg.publish_target_kind.clone()),
                default_target_ref: Some(NamespacedResourceRef {
                    namespace: cfg.publish_target_namespace.clone(),
                    name: cfg.publish_target_name.clone(),
                    uid: None,
                }),
            }),
        },
    );
    fe.metadata.labels = Some(migrator_labels());
    fe.metadata.annotations = Some(migrator_annotations(fi));
    fe
}

pub(crate) fn frontend_pages_from_fi(fi: &FrontendIntegration) -> Vec<FrontendExtensionPageSpec> {
    let mut pages = BTreeMap::new();

    for menu in &fi.spec.menus {
        match menu.type_ {
            MenuNodeType::Page => {
                insert_frontend_page(&mut pages, fi, &menu.key);
            }
            MenuNodeType::Organization => {
                for child in &menu.children {
                    insert_frontend_page(&mut pages, fi, &child.key);
                }
            }
        }
    }

    pages.into_values().collect()
}

fn insert_frontend_page(
    pages: &mut BTreeMap<String, FrontendExtensionPageSpec>,
    fi: &FrontendIntegration,
    page_key: &str,
) {
    let Some(page) = fi.spec.pages.iter().find(|page| page.key == page_key) else {
        return;
    };
    pages
        .entry(page_key.to_string())
        .or_insert_with(|| FrontendExtensionPageSpec::from_fi_page(page));
}

pub(crate) fn migrator_labels() -> BTreeMap<String, String> {
    BTreeMap::from([(LABEL_MANAGED_BY.to_string(), MANAGED_BY_VALUE.to_string())])
}

pub(crate) fn migrator_annotations(fi: &FrontendIntegration) -> BTreeMap<String, String> {
    let mut annotations = BTreeMap::from([(ANNO_SOURCE_FI_NAME.to_string(), fi.name_any())]);
    if let Some(uid) = fi.meta().uid.as_ref() {
        annotations.insert(ANNO_SOURCE_FI_UID.to_string(), uid.clone());
    }
    annotations
}
pub(crate) fn fi_display_name(fi: &FrontendIntegration) -> String {
    fi.spec
        .display_name
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| fi.name_any())
}

pub(crate) fn fi_description(fi: &FrontendIntegration, display_name: &str) -> String {
    fi.metadata
        .annotations
        .as_ref()
        .and_then(|annos| annos.get("kubesphere.io/description"))
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| display_name.to_string())
}

pub(crate) fn default_provider(
    fi: &FrontendIntegration,
) -> BTreeMap<String, ExtensionProviderSpec> {
    let name = fi_creator(fi);
    BTreeMap::from([
        (
            "en".to_string(),
            ExtensionProviderSpec {
                name: name.clone(),
                email: None,
                url: None,
            },
        ),
        (
            "zh".to_string(),
            ExtensionProviderSpec {
                name,
                email: None,
                url: None,
            },
        ),
    ])
}

pub(crate) fn fi_creator(fi: &FrontendIntegration) -> String {
    fi.metadata
        .annotations
        .as_ref()
        .and_then(|annos| annos.get(ANNO_CREATOR))
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| DEFAULT_PROVIDER_NAME.to_string())
}

pub(crate) fn localized_map(value: String) -> BTreeMap<String, String> {
    BTreeMap::from([("en".to_string(), value)])
}
