use frontend_forge_api::{
    BuilderSpec, FrontendIntegrationSpec, IframePageSpec, MenuNodeType, MenuPlacement, PageSpec,
    PageType, PrimaryMenuSpec,
};
use kube::core::ObjectMeta;

use super::*;

fn cfg() -> MigratorConfig {
    MigratorConfig {
        package_version: "0.1.0".to_string(),
        schema_version: "v1".to_string(),
        ready_timeout: Duration::from_secs(1),
        poll_interval: Duration::from_millis(1),
        publish_target_kind: PublishTargetKind::ConfigMap,
        publish_target_namespace: "extension-frontend-forge".to_string(),
        publish_target_name: "ksbuilder-publish-config".to_string(),
    }
}

fn fi(name: &str) -> FrontendIntegration {
    FrontendIntegration {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            annotations: Some(BTreeMap::from([
                (
                    "kubesphere.io/description".to_string(),
                    "description from annotation".to_string(),
                ),
                (ANNO_CREATOR.to_string(), "creator-user".to_string()),
            ])),
            uid: Some("fi-uid".to_string()),
            ..Default::default()
        },
        spec: FrontendIntegrationSpec {
            display_name: Some("Display Name".to_string()),
            locales: BTreeMap::from([(
                "en".to_string(),
                BTreeMap::from([("title".to_string(), "Title".to_string())]),
            )]),
            enabled: Some(true),
            menus: vec![PrimaryMenuSpec {
                display_name: "Menu".to_string(),
                key: "menu".to_string(),
                icon: None,
                placement: MenuPlacement::Global,
                type_: MenuNodeType::Page,
                children: vec![],
            }],
            pages: vec![PageSpec {
                key: "menu".to_string(),
                type_: PageType::Iframe,
                crd_table: None,
                iframe: Some(IframePageSpec {
                    src: "https://example.test".to_string(),
                }),
            }],
            builder: Some(BuilderSpec {
                engine_version: Some("v1alpha1".to_string()),
            }),
        },
        status: None,
    }
}

#[test]
fn migrated_fe_name_always_prefixes_fi() {
    assert_eq!(migrated_fe_name("foo"), "fi-foo");
    assert_eq!(migrated_fe_name("fi-foo"), "fi-fi-foo");
}

#[test]
fn migrated_fe_name_uses_slice_hash_when_too_long() {
    let name = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz";
    let migrated = migrated_fe_name(name);
    assert_eq!(migrated.len(), 63);
    assert!(migrated.starts_with("fi-"));
    assert_ne!(migrated, format!("fi-{name}"));
    assert!(migrated.ends_with(&sha256_hex(name.as_bytes())[..12]));
}

#[test]
fn frontend_extension_from_fi_copies_source_fields_and_defaults_package() {
    let fi = fi("demo");
    let fe = frontend_extension_from_fi(&fi, "fi-demo", &cfg());

    assert_eq!(
        fe.metadata.labels.unwrap()[LABEL_MANAGED_BY],
        MANAGED_BY_VALUE
    );
    let annotations = fe.metadata.annotations.unwrap();
    assert_eq!(annotations[ANNO_SOURCE_FI_NAME], "demo");
    assert_eq!(annotations[ANNO_SOURCE_FI_UID], "fi-uid");
    assert_eq!(fe.spec.package.name.as_deref(), Some("fi-demo"));
    assert_eq!(fe.spec.package.version, "0.1.0");
    assert_eq!(fe.spec.package.display_name["en"], "Display Name");
    assert_eq!(
        fe.spec.package.description["en"],
        "description from annotation"
    );
    assert_eq!(fe.spec.package.icon.as_deref(), Some(DEFAULT_PACKAGE_ICON));
    assert_eq!(
        fe.spec.package.category.as_deref(),
        Some(DEFAULT_PACKAGE_CATEGORY)
    );
    assert_eq!(fe.spec.package.provider["en"].name, "creator-user");
    assert_eq!(fe.spec.package.provider["zh"].name, "creator-user");
    assert_eq!(fe.spec.source.inline.schema_version, "v1alpha1");
    assert_eq!(
        fe.spec.source.inline.frontend.display_name.as_deref(),
        Some("Display Name")
    );
    assert_eq!(
        fe.spec.source.inline.frontend.locales["en"]["title"],
        "Title"
    );
    assert_eq!(fe.spec.source.inline.frontend.menus[0].key, "menu");
    assert_eq!(
        fe.spec.source.inline.frontend.menus[0].page_key.as_deref(),
        Some("menu")
    );
    assert_eq!(
        fe.spec.source.inline.frontend.menus[0].placements,
        vec![MenuPlacement::Global]
    );
    assert_eq!(fe.spec.source.inline.frontend.pages[0].key, "menu");
    assert_eq!(
        fe.spec.source.inline.frontend.pages[0].placements,
        vec![MenuPlacement::Global]
    );
}

#[test]
fn frontend_extension_from_fi_falls_back_for_package_metadata() {
    let mut fi = fi("demo");
    fi.metadata.annotations = None;
    fi.spec.display_name = None;
    fi.spec.builder = None;
    let fe = frontend_extension_from_fi(&fi, "fi-demo", &cfg());

    assert_eq!(fe.spec.package.display_name["en"], "demo");
    assert_eq!(fe.spec.package.description["en"], "demo");
    assert_eq!(fe.spec.package.provider["en"].name, DEFAULT_PROVIDER_NAME);
    assert_eq!(fe.spec.package.provider["zh"].name, DEFAULT_PROVIDER_NAME);
    assert_eq!(fe.spec.source.inline.schema_version, "v1");
}

#[test]
fn unmanaged_existing_fe_is_rejected() {
    let fi = fi("demo");
    let fe = FrontendExtension::new(
        "fi-demo",
        frontend_extension_from_fi(&fi, "fi-demo", &cfg()).spec,
    );
    let err = ensure_existing_fe_is_managed_by_fi(&fe, &fi).unwrap_err();
    assert!(err.to_string().contains("not migrator-owned"));
}

#[test]
fn publish_request_id_is_stable_for_fe_and_source_hash() {
    assert_eq!(
        publish_request_id("fi-demo", "sha256:abcdef1234567890"),
        "fi-migration-fi-demo-abcdef123456"
    );
}

#[test]
fn publish_intent_patch_writes_generation_source_and_clears_digest() {
    let patch = publish_intent_patch(
        &cfg(),
        7,
        "sha256:abcdef1234567890",
        "fi-migration-fi-demo-abcdef123456",
    );

    let annotations = &patch["metadata"]["annotations"];
    assert_eq!(
        annotations[ANNO_PUBLISH_REQUEST_ID],
        "fi-migration-fi-demo-abcdef123456"
    );
    assert_eq!(annotations[ANNO_PUBLISH_REQUEST_GENERATION], "7");
    assert_eq!(
        annotations[ANNO_PUBLISH_REQUEST_SOURCE_HASH],
        "sha256:abcdef1234567890"
    );
    assert_eq!(
        annotations[ANNO_PUBLISH_ARTIFACT_DIGEST],
        serde_json::Value::Null
    );
    assert_eq!(annotations[ANNO_PUBLISH_TARGET_KIND], "ConfigMap");
    assert_eq!(
        annotations[ANNO_PUBLISH_TARGET_NAMESPACE],
        "extension-frontend-forge"
    );
    assert_eq!(
        annotations[ANNO_PUBLISH_TARGET_NAME],
        "ksbuilder-publish-config"
    );
}

#[test]
fn crd_status_subresource_detection() {
    let value = json!({
        "status": {
            "conditions": [{ "type": "Established", "status": "True" }]
        },
        "spec": {
            "versions": [{
                "name": "v1alpha1",
                "served": true,
                "subresources": { "status": {} }
            }]
        }
    });

    assert!(crd_established(&value));
    assert!(crd_has_v1alpha1_status_subresource(&value));
}
