use frontend_forge_api::{
    FrontendIntegrationSpec, IframePageSpec, LastBuildError, MenuNodeType, MenuPlacement, PageSpec,
    PageType, PrimaryMenuSpec,
};
use k8s_openapi::api::batch::v1::JobStatus;
use kube::core::ObjectMeta;

use super::*;

fn fi(name: &str, status: Option<FrontendIntegrationStatus>) -> FrontendIntegration {
    FrontendIntegration {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some("default".to_string()),
            generation: Some(3),
            ..Default::default()
        },
        spec: FrontendIntegrationSpec {
            display_name: None,
            locales: BTreeMap::new(),
            enabled: Some(true),
            menus: vec![PrimaryMenuSpec {
                display_name: "demo".to_string(),
                key: "demo".to_string(),
                icon: None,
                placement: MenuPlacement::Global,
                type_: MenuNodeType::Page,
                children: vec![],
            }],
            pages: vec![PageSpec {
                key: "demo".to_string(),
                type_: PageType::Iframe,
                crd_table: None,
                iframe: Some(IframePageSpec {
                    src: "http://example.test".to_string(),
                }),
            }],
            builder: None,
        },
        status,
    }
}

fn spec_hash(fi: &FrontendIntegration) -> Result<String, CommonError> {
    build_spec_hash(fi)
}

fn bundle_for_hash(name: &str, spec_hash: &str) -> JSBundle {
    JSBundle {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            labels: Some(BTreeMap::from([(
                LABEL_SPEC_HASH.to_string(),
                hash_label_value(spec_hash),
            )])),
            ..Default::default()
        },
        spec: frontend_forge_api::JsBundleSpec {
            raw: None,
            raw_from: None,
        },
        status: None,
    }
}

#[test]
fn build_hash_ignores_enabled() -> Result<(), CommonError> {
    let mut enabled_fi = fi("demo", None);
    enabled_fi.spec.enabled = Some(true);

    let mut disabled_fi = fi("demo", None);
    disabled_fi.spec.enabled = Some(false);

    assert_eq!(spec_hash(&enabled_fi)?, spec_hash(&disabled_fi)?);
    Ok(())
}

#[test]
fn needs_build_when_hash_changes() {
    let mut fi = fi(
        "demo",
        Some(FrontendIntegrationStatus {
            observed_spec_hash: Some("sha256:old".to_string()),
            phase: FrontendIntegrationPhase::Succeeded,
            ..Default::default()
        }),
    );
    fi.spec.enabled = Some(true);
    assert!(needs_new_build(&fi, "sha256:new", None));
}

#[test]
fn does_not_build_when_observed_hash_matches() -> Result<(), CommonError> {
    let mut fi = fi("demo", None);
    fi.spec.enabled = Some(true);
    let hash = spec_hash(&fi)?;
    let bundle = bundle_for_hash("fi-demo", &hash);
    fi.status = Some(FrontendIntegrationStatus {
        observed_spec_hash: Some(hash.clone()),
        phase: FrontendIntegrationPhase::Succeeded,
        ..Default::default()
    });

    assert!(!needs_new_build(&fi, &hash, Some(&bundle)));
    Ok(())
}

#[test]
fn does_not_auto_retry_failed_build_when_hash_is_unchanged() -> Result<(), CommonError> {
    let mut fi = fi("demo", None);
    fi.spec.enabled = Some(true);
    let hash = spec_hash(&fi)?;
    fi.status = Some(FrontendIntegrationStatus {
        observed_spec_hash: Some(hash.clone()),
        phase: FrontendIntegrationPhase::Failed,
        ..Default::default()
    });

    assert!(!needs_new_build(&fi, &hash, None));
    Ok(())
}

#[test]
fn builds_when_matching_bundle_is_missing_after_reenable() -> Result<(), CommonError> {
    let mut fi = fi("demo", None);
    fi.spec.enabled = Some(true);
    let hash = spec_hash(&fi)?;
    fi.status = Some(FrontendIntegrationStatus {
        observed_spec_hash: Some(hash.clone()),
        phase: FrontendIntegrationPhase::Pending,
        message: Some("Disabled".to_string()),
        ..Default::default()
    });

    assert!(needs_new_build(&fi, &hash, None));
    Ok(())
}

#[test]
fn hash_label_value_is_dns_safe() {
    assert_eq!(hash_label_value("sha256:abcd"), "abcd");
    assert_eq!(hash_label_value("abcd"), "abcd");
}

fn job_with_status(active: Option<i32>, succeeded: Option<i32>, failed: Option<i32>) -> Job {
    Job {
        status: Some(JobStatus {
            active,
            succeeded,
            failed,
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn job_with_failed_condition(message: Option<&str>, reason: Option<&str>) -> Job {
    Job {
        status: Some(JobStatus {
            conditions: Some(vec![k8s_openapi::api::batch::v1::JobCondition {
                message: message.map(str::to_string),
                reason: reason.map(str::to_string),
                status: "True".to_string(),
                type_: "Failed".to_string(),
                ..Default::default()
            }]),
            failed: Some(1),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn does_not_reuse_failed_job_when_retrying_failed_phase() {
    let fi = fi(
        "demo",
        Some(FrontendIntegrationStatus {
            phase: FrontendIntegrationPhase::Failed,
            ..Default::default()
        }),
    );
    let failed_job = job_with_status(None, None, Some(1));

    assert!(!should_reuse_build_job(
        &fi,
        &failed_job,
        None,
        "sha256:demo"
    ));
}

#[test]
fn reuses_running_job_when_retrying_failed_phase() {
    let fi = fi(
        "demo",
        Some(FrontendIntegrationStatus {
            phase: FrontendIntegrationPhase::Failed,
            ..Default::default()
        }),
    );
    let running_job = job_with_status(Some(1), None, None);

    assert!(should_reuse_build_job(
        &fi,
        &running_job,
        None,
        "sha256:demo"
    ));
}

#[test]
fn does_not_reuse_succeeded_job_when_matching_bundle_is_missing() {
    let fi = fi(
        "demo",
        Some(FrontendIntegrationStatus {
            phase: FrontendIntegrationPhase::Succeeded,
            ..Default::default()
        }),
    );
    let succeeded_job = job_with_status(None, Some(1), None);

    assert!(!should_reuse_build_job(
        &fi,
        &succeeded_job,
        None,
        "sha256:demo",
    ));
}

#[test]
fn bundle_hash_match_uses_build_hash_label() -> Result<(), CommonError> {
    let mut fi = fi("demo", None);
    fi.spec.enabled = Some(true);
    let hash = spec_hash(&fi)?;
    let bundle = bundle_for_hash("fi-demo", &hash);

    assert!(bundle_matches_spec_hash(&bundle, &hash));
    Ok(())
}

#[test]
fn disabled_status_clears_last_build_and_uses_live_bundle_ref() {
    let fi = fi(
        "demo",
        Some(FrontendIntegrationStatus {
            observed_spec_hash: Some("sha256:demo".to_string()),
            observed_manifest_hash: Some("sha256:manifest".to_string()),
            last_build: Some(LastBuildStatus {
                job_ref: Some(ResourceRef {
                    name: "old-job".to_string(),
                    namespace: Some("default".to_string()),
                    uid: Some("job-uid".to_string()),
                }),
                started_at: Some(Utc::now()),
            }),
            bundle_ref: Some(ResourceRef {
                name: "stale-bundle".to_string(),
                namespace: None,
                uid: Some("stale-uid".to_string()),
            }),
            ..Default::default()
        }),
    );
    let bundle = bundle_for_hash("fi-demo", "sha256:demo");
    let status = disabled_status(&fi, Some(&bundle));

    assert!(status.last_build.is_none());
    assert_eq!(
        status.bundle_ref.map(|bundle_ref| bundle_ref.name),
        Some("fi-demo".to_string())
    );
    assert_eq!(status.message.as_deref(), Some("Disabled"));
}

#[test]
fn building_status_preserves_last_error_for_same_spec_hash() {
    let fi = fi(
        "demo",
        Some(FrontendIntegrationStatus {
            observed_spec_hash: Some("sha256:demo".to_string()),
            last_error: Some(LastBuildError {
                source: "runner".to_string(),
                message: "duplicate page key".to_string(),
                reason: Some("RunnerFailed".to_string()),
                occurred_at: Some(Utc::now()),
            }),
            ..Default::default()
        }),
    );
    let job = job_with_status(Some(1), None, None);

    let status = building_status(&fi, "sha256:demo", "fi-demo", &job, "Build in progress");

    assert_eq!(
        status
            .last_error
            .as_ref()
            .map(|error| error.message.as_str()),
        Some("duplicate page key")
    );
}

#[test]
fn building_status_preserves_started_at_for_same_job() {
    let started_at = Utc::now();
    let fi = fi(
        "demo",
        Some(FrontendIntegrationStatus {
            phase: FrontendIntegrationPhase::Building,
            observed_spec_hash: Some("sha256:demo".to_string()),
            last_build: Some(LastBuildStatus {
                job_ref: Some(ResourceRef {
                    name: "build-job".to_string(),
                    namespace: Some("default".to_string()),
                    uid: Some("job-uid".to_string()),
                }),
                started_at: Some(started_at),
            }),
            ..Default::default()
        }),
    );
    let mut job = job_with_status(Some(1), None, None);
    job.metadata.name = Some("build-job".to_string());

    let status = building_status(&fi, "sha256:demo", "fi-demo", &job, "Build in progress");

    assert_eq!(
        status.last_build.and_then(|build| build.started_at),
        Some(started_at)
    );
}

#[test]
fn status_patch_is_skipped_when_status_is_unchanged() {
    let status = FrontendIntegrationStatus {
        phase: FrontendIntegrationPhase::Building,
        observed_spec_hash: Some("sha256:demo".to_string()),
        observed_generation: Some(3),
        last_build: Some(LastBuildStatus {
            job_ref: Some(ResourceRef {
                name: "build-job".to_string(),
                namespace: Some("default".to_string()),
                uid: Some("job-uid".to_string()),
            }),
            started_at: Some(Utc::now()),
        }),
        bundle_ref: Some(ResourceRef {
            name: "fi-demo".to_string(),
            namespace: None,
            uid: None,
        }),
        message: Some("Build in progress".to_string()),
        ..Default::default()
    };
    let fi = fi("demo", Some(status.clone()));

    assert!(!fi_status_needs_patch(&fi, &status));
}

#[test]
fn failure_error_prefers_existing_runner_error_for_same_spec_hash() {
    let fi = fi(
        "demo",
        Some(FrontendIntegrationStatus {
            observed_spec_hash: Some("sha256:demo".to_string()),
            last_error: Some(LastBuildError {
                source: "runner".to_string(),
                message: "duplicate page key".to_string(),
                reason: Some("RunnerFailed".to_string()),
                occurred_at: Some(Utc::now()),
            }),
            ..Default::default()
        }),
    );
    let job = job_with_failed_condition(
        Some("Job has reached the specified backoff limit"),
        Some("BackoffLimitExceeded"),
    );

    let failure = failure_error_for_status(&fi, "sha256:demo", &job);

    assert_eq!(failure.source, "runner");
    assert_eq!(failure.message, "duplicate page key");
}

#[test]
fn status_patch_sets_null_for_cleared_optional_refs() -> Result<(), Error> {
    let status = FrontendIntegrationStatus {
        phase: FrontendIntegrationPhase::Pending,
        last_build: None,
        bundle_ref: None,
        last_error: None,
        ..Default::default()
    };
    let patch = frontend_integration_status_patch(&status, "default", "demo")?;

    assert_eq!(patch["status"]["last_build"], serde_json::Value::Null);
    assert_eq!(patch["status"]["bundle_ref"], serde_json::Value::Null);
    assert_eq!(patch["status"]["last_error"], serde_json::Value::Null);
    Ok(())
}
