use frontend_forge_api::{
    ArtifactStorageStatus, ExtensionDownloadStatus, FrontendExtensionStatus, PublishPolicyMode,
    PublishPolicySpec,
};
use kube::core::ObjectMeta;

use super::*;

fn ready_fe() -> FrontendExtension {
    serde_yaml::from_str(
        r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendExtension
metadata:
  name: inspecttask
spec:
  package:
    version: 0.1.0
    displayName:
      en: Inspect Task
    description:
      en: InspectTask extension package
  source:
    type: Inline
    inline:
      schemaVersion: v1
      frontend: {}
"#,
    )
    .unwrap()
}

#[test]
fn ready_artifact_requires_matching_source_hash() {
    let mut fe = ready_fe();
    fe.metadata = ObjectMeta {
        name: Some("inspecttask".to_string()),
        ..Default::default()
    };
    fe.status = Some(FrontendExtensionStatus {
        phase: FrontendExtensionPhase::Ready,
        observed_source_hash: Some("sha256:new".to_string()),
        artifact: Some(ExtensionArtifactStatus {
            storage: ArtifactStorageStatus {
                kind: ArtifactStorageKind::ConfigMap,
                ref_: NamespacedResourceRef {
                    namespace: "extension-frontend-forge".to_string(),
                    name: "fe-inspecttask-a1b2c3d4".to_string(),
                    uid: None,
                },
                key: "package.tgz".to_string(),
            },
            digest: "sha256:artifact".to_string(),
            size_bytes: 1,
            media_type: "application/gzip".to_string(),
            filename: "inspecttask-0.1.0.tgz".to_string(),
            generated_at: chrono::Utc::now(),
            source_hash: "sha256:old".to_string(),
            artifact_key: Some("sha256:artifact-key".to_string()),
        }),
        download: Some(ExtensionDownloadStatus {
            ready: true,
            filename: "inspecttask-0.1.0.tgz".to_string(),
            media_type: "application/gzip".to_string(),
        }),
        ..Default::default()
    });

    assert!(ready_artifact(&fe).is_err());
}

#[test]
fn ready_artifact_requires_current_source_hash() {
    let mut fe = ready_fe();
    fe.status = Some(FrontendExtensionStatus {
        phase: FrontendExtensionPhase::Ready,
        observed_source_hash: Some("sha256:old".to_string()),
        artifact: Some(ExtensionArtifactStatus {
            storage: ArtifactStorageStatus {
                kind: ArtifactStorageKind::ConfigMap,
                ref_: NamespacedResourceRef {
                    namespace: "extension-frontend-forge".to_string(),
                    name: "fe-inspecttask-a1b2c3d4".to_string(),
                    uid: None,
                },
                key: "package.tgz".to_string(),
            },
            digest: "sha256:artifact".to_string(),
            size_bytes: 1,
            media_type: "application/gzip".to_string(),
            filename: "inspecttask-0.1.0.tgz".to_string(),
            generated_at: chrono::Utc::now(),
            source_hash: "sha256:old".to_string(),
            artifact_key: Some("sha256:artifact-key".to_string()),
        }),
        download: Some(ExtensionDownloadStatus {
            ready: true,
            filename: "inspecttask-0.1.0.tgz".to_string(),
            media_type: "application/gzip".to_string(),
        }),
        ..Default::default()
    });

    let err = ready_artifact(&fe).unwrap_err();

    assert_eq!(err.status, StatusCode::CONFLICT);
    assert_eq!(
        err.message,
        "FrontendExtension artifact does not match current source hash"
    );
    assert!(publish_can_wait_for_artifact(&fe));
}

#[test]
fn list_query_passes_label_selector_to_kubernetes_list_params() {
    let query = FrontendExtensionListQuery {
        label_selector: Some(
            " frontend-forge.kubesphere.io/package-state=ready,frontend-forge.kubesphere.io/\
             publish-state=not-published "
                .to_string(),
        ),
    };

    let params = query.list_params();

    assert_eq!(
        params.label_selector.as_deref(),
        Some(
            "frontend-forge.kubesphere.io/package-state=ready,frontend-forge.kubesphere.io/\
             publish-state=not-published"
        )
    );
}

#[test]
fn empty_list_query_omits_label_selector() {
    let query = FrontendExtensionListQuery {
        label_selector: Some(" ".to_string()),
    };

    let params = query.list_params();

    assert_eq!(params.label_selector, None);
}

#[test]
fn resolve_publish_request_uses_fe_publish_policy_and_current_artifact() {
    let mut fe = ready_fe();
    fe.spec.publish_policy = Some(PublishPolicySpec {
        mode: PublishPolicyMode::Manual,
        default_target_kind: Some(PublishTargetKind::Secret),
        default_target_ref: Some(NamespacedResourceRef {
            namespace: "extension-frontend-forge".to_string(),
            name: "ksbuilder-publish-config".to_string(),
            uid: None,
        }),
    });
    let artifact = ExtensionArtifactStatus {
        storage: ArtifactStorageStatus {
            kind: ArtifactStorageKind::ConfigMap,
            ref_: NamespacedResourceRef {
                namespace: "extension-frontend-forge".to_string(),
                name: "fe-inspecttask-a1b2c3d4".to_string(),
                uid: None,
            },
            key: "package.tgz".to_string(),
        },
        digest: "sha256:artifact".to_string(),
        size_bytes: 1,
        media_type: "application/gzip".to_string(),
        filename: "inspecttask-0.1.0.tgz".to_string(),
        generated_at: chrono::Utc::now(),
        source_hash: "sha256:source".to_string(),
        artifact_key: Some("sha256:artifact-key".to_string()),
    };
    let request = PublishRequest {
        request_id: Some(" request-1 ".to_string()),
        expected_artifact_digest: None,
    };

    let resolved = resolve_publish_request(&fe, &request, Some(&artifact)).unwrap();

    assert_eq!(resolved.request_id, "request-1");
    assert_eq!(resolved.artifact_digest.as_deref(), Some("sha256:artifact"));
    assert_eq!(resolved.generation, fe.metadata.generation);
    assert_eq!(
        resolved.source_hash,
        frontend_extension_source_hash(&fe).unwrap()
    );
    assert_eq!(resolved.target_kind, "Secret");
    assert_eq!(resolved.target_ref.name, "ksbuilder-publish-config");
}

#[test]
fn resolve_publish_request_can_wait_for_current_source_without_artifact() {
    let mut fe = ready_fe();
    fe.metadata.generation = Some(3);
    fe.spec.publish_policy = Some(PublishPolicySpec {
        mode: PublishPolicyMode::Manual,
        default_target_kind: Some(PublishTargetKind::ConfigMap),
        default_target_ref: Some(NamespacedResourceRef {
            namespace: "extension-frontend-forge".to_string(),
            name: "ksbuilder-publish-config".to_string(),
            uid: None,
        }),
    });
    let request = PublishRequest {
        request_id: Some(" request-queued ".to_string()),
        expected_artifact_digest: None,
    };

    let resolved = resolve_publish_request(&fe, &request, None).unwrap();

    assert_eq!(resolved.request_id, "request-queued");
    assert_eq!(resolved.artifact_digest, None);
    assert_eq!(resolved.generation, Some(3));
    assert_eq!(
        resolved.source_hash,
        frontend_extension_source_hash(&fe).unwrap()
    );
    assert_eq!(resolved.target_ref.name, "ksbuilder-publish-config");
}

#[test]
fn publish_request_idempotency_matches_absent_artifact_digest() {
    let current = PublishStatus {
        phase: PublishPhase::Pending,
        request_id: Some("request-queued".to_string()),
        artifact_digest: None,
        ..Default::default()
    };
    let request = ResolvedPublishRequest {
        request_id: "request-queued".to_string(),
        artifact_digest: None,
        generation: Some(3),
        source_hash: "sha256:source".to_string(),
        target_ref: NamespacedResourceRef {
            namespace: "extension-frontend-forge".to_string(),
            name: "ksbuilder-publish-config".to_string(),
            uid: None,
        },
        target_kind: "ConfigMap".to_string(),
    };

    assert!(publish_request_matches_status(&current, &request));
}

#[test]
fn publish_request_idempotency_rejects_artifact_digest_mismatch() {
    let current = PublishStatus {
        phase: PublishPhase::Pending,
        request_id: Some("request-queued".to_string()),
        artifact_digest: None,
        ..Default::default()
    };
    let request = ResolvedPublishRequest {
        request_id: "request-queued".to_string(),
        artifact_digest: Some("sha256:artifact".to_string()),
        generation: Some(3),
        source_hash: "sha256:source".to_string(),
        target_ref: NamespacedResourceRef {
            namespace: "extension-frontend-forge".to_string(),
            name: "ksbuilder-publish-config".to_string(),
            uid: None,
        },
        target_kind: "ConfigMap".to_string(),
    };

    assert!(!publish_request_matches_status(&current, &request));
}

#[test]
fn currently_published_requires_succeeded_and_active() {
    let mut fe = ready_fe();
    fe.status = Some(FrontendExtensionStatus {
        publish: Some(PublishStatus {
            phase: PublishPhase::Succeeded,
            active: true,
            ..Default::default()
        }),
        ..Default::default()
    });
    assert!(currently_published(&fe));

    fe.status = Some(FrontendExtensionStatus {
        publish: Some(PublishStatus {
            phase: PublishPhase::Succeeded,
            active: false,
            ..Default::default()
        }),
        ..Default::default()
    });
    assert!(!currently_published(&fe));
}

#[test]
fn ready_artifact_requires_ready_phase() {
    let mut fe = ready_fe();
    fe.status = Some(FrontendExtensionStatus {
        phase: FrontendExtensionPhase::Packaging,
        ..Default::default()
    });

    let err = ready_artifact(&fe).unwrap_err();

    assert_eq!(err.status, StatusCode::CONFLICT);
    assert_eq!(err.message, "FrontendExtension artifact is not ready");
}

#[test]
fn ready_artifact_requires_download_ready() {
    let mut fe = ready_fe();
    fe.status = Some(FrontendExtensionStatus {
        phase: FrontendExtensionPhase::Ready,
        observed_source_hash: Some("sha256:source".to_string()),
        artifact: Some(ExtensionArtifactStatus {
            storage: ArtifactStorageStatus {
                kind: ArtifactStorageKind::ConfigMap,
                ref_: NamespacedResourceRef {
                    namespace: "extension-frontend-forge".to_string(),
                    name: "fe-inspecttask-a1b2c3d4".to_string(),
                    uid: None,
                },
                key: "package.tgz".to_string(),
            },
            digest: "sha256:artifact".to_string(),
            size_bytes: 1,
            media_type: "application/gzip".to_string(),
            filename: "inspecttask-0.1.0.tgz".to_string(),
            generated_at: chrono::Utc::now(),
            source_hash: "sha256:source".to_string(),
            artifact_key: Some("sha256:artifact-key".to_string()),
        }),
        download: Some(ExtensionDownloadStatus {
            ready: false,
            filename: "inspecttask-0.1.0.tgz".to_string(),
            media_type: "application/gzip".to_string(),
        }),
        ..Default::default()
    });

    let err = ready_artifact(&fe).unwrap_err();

    assert_eq!(err.status, StatusCode::CONFLICT);
    assert_eq!(
        err.message,
        "FrontendExtension artifact is not downloadable"
    );
}

#[test]
fn verify_artifact_digest_rejects_mismatch() {
    let err = verify_artifact_digest(b"package", "sha256:mismatch").unwrap_err();

    assert_eq!(err.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(err.message, "artifact digest mismatch");
}
