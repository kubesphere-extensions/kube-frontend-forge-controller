use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

use k8s_openapi::{
    api::batch::v1::{Job, JobStatus},
    apimachinery::pkg::apis::meta::v1::OwnerReference,
};
use kube::{Api, Resource, api::PostParams};
use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use snafu::{ResultExt, Snafu};

pub const MANAGED_BY_VALUE: &str = "frontend-forge-builder-controller";
pub const LABEL_MANAGED_BY: &str = "frontend-forge.io/managed-by";
pub const LABEL_FI_NAME: &str = "frontend-forge.io/fi-name";
pub const LABEL_FE_MANAGED_BY: &str = "frontend-forge.kubesphere.io/managed-by";
pub const LABEL_FE_NAME: &str = "frontend-forge.kubesphere.io/fe-name";
pub const LABEL_FE_UID: &str = "frontend-forge.kubesphere.io/fe-uid";
pub const LABEL_ENABLED: &str = "frontend-forge.io/enabled";
pub const LABEL_SPEC_HASH: &str = "frontend-forge.io/spec-hash";
pub const LABEL_SOURCE_HASH: &str = "frontend-forge.kubesphere.io/source-hash";
pub const LABEL_SOURCE_HASH_SHORT: &str = "frontend-forge.kubesphere.io/source-hash-short";
pub const LABEL_ARTIFACT_KEY_SHORT: &str = "frontend-forge.kubesphere.io/artifact-key-short";
pub const LABEL_MANIFEST_HASH: &str = "frontend-forge.io/manifest-hash";
pub const LABEL_BUILD_KIND: &str = "frontend-forge.io/build-kind";
pub const LABEL_FE_BUILD_KIND: &str = "frontend-forge.kubesphere.io/build-kind";
pub const LABEL_PACKAGE_KIND: &str = "frontend-forge.kubesphere.io/package-kind";
pub const LABEL_PUBLISH_KIND: &str = "frontend-forge.kubesphere.io/publish-kind";
pub const LABEL_UNPUBLISH_KIND: &str = "frontend-forge.kubesphere.io/unpublish-kind";
pub const LABEL_PUBLISH_REQUEST_HASH: &str = "frontend-forge.kubesphere.io/publish-request-hash";
pub const LABEL_UNPUBLISH_REQUEST_HASH: &str =
    "frontend-forge.kubesphere.io/unpublish-request-hash";
pub const LABEL_FE_PACKAGE_STATUS: &str = "frontend-forge.kubesphere.io/package-state";
pub const LABEL_FE_PUBLISH_STATUS: &str = "frontend-forge.kubesphere.io/publish-state";
pub const LABEL_FE_PUBLISH_FRESH: &str = "frontend-forge.kubesphere.io/publish-fresh";
pub const DEPRECATED_LABEL_FE_PACKAGE_STATUS: &str = "Package";
pub const DEPRECATED_LABEL_FE_PUBLISH_STATUS: &str = "Publish";
pub const ANNO_BUILD_JOB: &str = "frontend-forge.io/build-job";
pub const ANNO_MANIFEST_HASH: &str = "frontend-forge.io/manifest-hash";
pub const ANNO_MANIFEST_CONTENT: &str = "frontend-forge.io/manifest-content";
pub const ANNO_OBSERVED_GENERATION: &str = "frontend-forge.io/observed-generation";
pub const ANNO_FE_OBSERVED_GENERATION: &str = "frontend-forge.kubesphere.io/observed-generation";
pub const ANNO_SOURCE_SPEC: &str = "frontend-forge.io/source-spec";
pub const ANNO_SOURCE_SPEC_HASH: &str = "frontend-forge.io/source-spec-hash";
pub const ANNO_SOURCE_GENERATION: &str = "frontend-forge.io/source-generation";
pub const ANNO_REBUILD_TOKEN: &str = "frontend-forge.kubesphere.io/rebuild-token";
pub const ANNO_SOURCE_HASH: &str = "frontend-forge.kubesphere.io/source-hash";
pub const ANNO_ARTIFACT_KEY: &str = "frontend-forge.kubesphere.io/artifact-key";
pub const ANNO_ARTIFACT_DIGEST: &str = "frontend-forge.kubesphere.io/artifact-digest";
pub const ANNO_ARTIFACT_FILENAME: &str = "frontend-forge.kubesphere.io/artifact-filename";
pub const ANNO_PUBLISH_REQUEST_ID: &str = "frontend-forge.kubesphere.io/publish-request-id";
pub const ANNO_PUBLISH_REQUEST_GENERATION: &str =
    "frontend-forge.kubesphere.io/publish-request-generation";
pub const ANNO_PUBLISH_REQUEST_SOURCE_HASH: &str =
    "frontend-forge.kubesphere.io/publish-request-source-hash";
pub const ANNO_PUBLISH_ARTIFACT_DIGEST: &str =
    "frontend-forge.kubesphere.io/publish-artifact-digest";
pub const ANNO_PUBLISH_TARGET_KIND: &str = "frontend-forge.kubesphere.io/publish-target-kind";
pub const ANNO_PUBLISH_TARGET_NAMESPACE: &str =
    "frontend-forge.kubesphere.io/publish-target-namespace";
pub const ANNO_PUBLISH_TARGET_NAME: &str = "frontend-forge.kubesphere.io/publish-target-name";
pub const ANNO_UNPUBLISH_REQUEST_ID: &str = "frontend-forge.kubesphere.io/unpublish-request-id";
pub const ANNO_UNPUBLISH_EXTENSION_NAME: &str =
    "frontend-forge.kubesphere.io/unpublish-extension-name";
pub const ANNO_DELETE_AFTER_UNPUBLISH_REQUEST_ID: &str =
    "frontend-forge.kubesphere.io/delete-after-unpublish-request-id";
pub const BUILD_KIND_VALUE: &str = "frontend-forge";
pub const PACKAGE_KIND_VALUE: &str = "frontend-extension-package";
pub const PUBLISH_KIND_VALUE: &str = "frontend-extension-publish";
pub const UNPUBLISH_KIND_VALUE: &str = "frontend-extension-unpublish";
pub const FE_PACKAGE_STATUS_PACKAGING: &str = "packaging";
pub const FE_PACKAGE_STATUS_READY: &str = "ready";
pub const FE_PACKAGE_STATUS_FAILED: &str = "failed";
pub const FE_PUBLISH_STATUS_NOT_PUBLISHED: &str = "not-published";
pub const FE_PUBLISH_STATUS_PUBLISHING: &str = "publishing";
pub const FE_PUBLISH_STATUS_PUBLISHED: &str = "published";
pub const FE_PUBLISH_STATUS_FAILED: &str = "failed";
pub const FE_PUBLISH_FRESH_TRUE: &str = "true";
pub const FE_PUBLISH_FRESH_FALSE: &str = "false";
pub const DEFAULT_MANIFEST_FILENAME: &str = "manifest.json";
pub const DEFAULT_MANIFEST_MOUNT_PATH: &str = "/work/manifest/manifest.json";
pub const MAX_SECRET_PAYLOAD_BYTES: usize = 1_000_000;

#[derive(Debug, Snafu)]
pub enum CommonError {
    #[snafu(display("manifest serialization failed: {source}"))]
    Serialize { source: serde_json::Error },
}

#[derive(Debug, Snafu)]
pub enum JobError {
    #[snafu(display("failed to create Job {namespace}/{name}: {source}"))]
    Create {
        namespace: String,
        name: String,
        #[snafu(source(from(kube::Error, Box::new)))]
        source: Box<kube::Error>,
    },
    #[snafu(display("failed to get existing Job after conflict {namespace}/{name}: {source}"))]
    GetAfterConflict {
        namespace: String,
        name: String,
        #[snafu(source(from(kube::Error, Box::new)))]
        source: Box<kube::Error>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservedJobPhase {
    Pending,
    Running,
    Succeeded,
    Failed,
}

pub fn canonicalize_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let sorted: BTreeMap<String, Value> = map
                .iter()
                .map(|(k, v)| (k.clone(), canonicalize_json(v)))
                .collect();
            let mut out = Map::new();
            for (k, v) in sorted {
                out.insert(k, v);
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(canonicalize_json).collect()),
        _ => value.clone(),
    }
}

pub fn canonical_json_string(value: &Value) -> Result<String, CommonError> {
    serde_json::to_string(&canonicalize_json(value)).context(SerializeSnafu)
}

#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

#[must_use]
pub fn manifest_hash_from_content(content: &str) -> String {
    let hash = sha256_hex(content.as_bytes());
    format!("sha256:{hash}")
}

pub fn manifest_content_and_hash(source: &Value) -> Result<(String, String), CommonError> {
    let content = canonical_json_string(source)?;
    let hash = manifest_hash_from_content(&content);
    Ok((content, hash))
}

pub fn serializable_content_and_hash<T>(source: &T) -> Result<(String, String), CommonError>
where
    T: Serialize,
{
    let value = serde_json::to_value(source).context(SerializeSnafu)?;
    manifest_content_and_hash(&value)
}

pub fn serializable_hash<T>(source: &T) -> Result<String, CommonError>
where
    T: Serialize,
{
    let (_, hash) = serializable_content_and_hash(source)?;
    Ok(hash)
}

#[derive(Serialize)]
struct ArtifactKeyInput<'a> {
    #[serde(rename = "keyVersion")]
    key_version: &'static str,
    #[serde(rename = "sourceHash")]
    source_hash: &'a str,
    #[serde(rename = "rebuildToken")]
    rebuild_token: &'a str,
}

pub fn artifact_key(source_hash: &str, rebuild_token: &str) -> Result<String, CommonError> {
    serializable_hash(&ArtifactKeyInput {
        key_version: "v1",
        source_hash,
        rebuild_token,
    })
}

#[must_use]
pub fn observed_job_phase(status: Option<&JobStatus>) -> ObservedJobPhase {
    let Some(status) = status else {
        return ObservedJobPhase::Pending;
    };

    if let Some(conditions) = &status.conditions {
        for cond in conditions {
            if cond.status == "True" && cond.type_ == "Complete" {
                return ObservedJobPhase::Succeeded;
            }
        }
        for cond in conditions {
            if cond.status == "True" && cond.type_ == "Failed" {
                return ObservedJobPhase::Failed;
            }
        }
    }

    if status.active.unwrap_or(0) > 0 {
        return ObservedJobPhase::Running;
    }
    if status.succeeded.unwrap_or(0) > 0 {
        return ObservedJobPhase::Succeeded;
    }
    if status.failed.unwrap_or(0) > 0 {
        return ObservedJobPhase::Failed;
    }

    ObservedJobPhase::Pending
}

#[must_use]
pub fn extract_job_message(job: &Job) -> Option<String> {
    let status = job.status.as_ref()?;
    if let Some(conditions) = &status.conditions
        && let Some(cond) = conditions
            .iter()
            .find(|c| c.status == "True" && c.type_ == "Failed")
    {
        return cond.message.clone().or_else(|| cond.reason.clone());
    }
    None
}

#[must_use]
pub fn base_owner_ref<T>(obj: &T) -> Option<OwnerReference>
where
    T: Resource<DynamicType = ()>,
{
    obj.controller_owner_ref(&())
}

/// Create a Kubernetes Job, or return the existing Job when creation races.
///
/// # Errors
///
/// Returns an error when Job creation fails for reasons other than conflict, or
/// when the follow-up read after a conflict fails.
pub async fn create_or_get_job(
    job_api: &Api<Job>,
    namespace: &str,
    job: Job,
    name: &str,
) -> Result<Job, JobError> {
    match job_api.create(&PostParams::default(), &job).await {
        Ok(created) => Ok(created),
        Err(kube::Error::Api(ae)) if ae.code == 409 => {
            job_api
                .get(name)
                .await
                .with_context(|_| GetAfterConflictSnafu {
                    namespace: namespace.to_string(),
                    name: name.to_string(),
                })
        }
        Err(err) => Err(JobError::Create {
            namespace: namespace.to_string(),
            name: name.to_string(),
            source: Box::new(err),
        }),
    }
}

#[must_use]
pub fn hash_short(hash: &str) -> String {
    let trimmed = hash.strip_prefix("sha256:").unwrap_or(hash);
    trimmed.chars().take(8).collect()
}

#[must_use]
pub fn hash_label_value(hash: &str) -> String {
    let trimmed = hash.strip_prefix("sha256:").unwrap_or(hash);
    if trimmed.is_empty() {
        return "0".to_string();
    }
    trimmed.chars().take(63).collect()
}

#[must_use]
pub fn hash_name_suffix(hash: &str) -> String {
    let trimmed = hash.strip_prefix("sha256:").unwrap_or(hash);
    if trimmed.is_empty() {
        return "0".to_string();
    }
    trimmed.chars().take(12).collect()
}

#[must_use]
pub fn default_bundle_name(fi_name: &str) -> String {
    bounded_name(&format!("fi-{fi_name}"), 63)
}

#[must_use]
pub fn default_cluster_bundle_name(fi_namespace: &str, fi_name: &str) -> String {
    bounded_name(&format!("fi-{fi_namespace}-{fi_name}"), 63)
}

#[must_use]
pub fn job_name(fi_name: &str, manifest_hash: &str) -> String {
    bounded_name(
        &format!("fi-{fi_name}-build-{}", hash_short(manifest_hash)),
        63,
    )
}

#[must_use]
pub fn package_job_name(fe_name: &str, artifact_key: &str, attempt: u32) -> String {
    let suffix = format!("-{}-a{attempt}", hash_name_suffix(artifact_key));
    bounded_name_preserving_suffix(&format!("fe-{fe_name}-package"), &suffix, 63)
}

#[must_use]
pub fn publish_job_name(fe_name: &str, request_id: &str) -> String {
    let request_hash = format!("sha256:{}", sha256_hex(request_id.as_bytes()));
    bounded_name(
        &format!("fe-{fe_name}-publish-{}", hash_short(&request_hash)),
        63,
    )
}

#[must_use]
pub fn unpublish_job_name(fe_name: &str, request_id: &str) -> String {
    let request_hash = format!("sha256:{}", sha256_hex(request_id.as_bytes()));
    bounded_name(
        &format!("fe-{fe_name}-unpublish-{}", hash_short(&request_hash)),
        63,
    )
}

#[must_use]
pub fn artifact_configmap_name(package_name: &str, artifact_key: &str) -> String {
    let suffix = format!("-{}", hash_name_suffix(artifact_key));
    bounded_name_preserving_suffix(&format!("fe-{package_name}"), &suffix, 63)
}

#[must_use]
pub fn secret_name(fi_name: &str, manifest_hash: &str, nonce: &str) -> String {
    bounded_name(
        &format!("fi-{fi_name}-mf-{}-{nonce}", hash_short(manifest_hash)),
        63,
    )
}

#[must_use]
pub fn bounded_name(raw: &str, max_len: usize) -> String {
    let sanitized = raw
        .chars()
        .map(|c| match c {
            'a'..='z' | '0'..='9' | '-' => c,
            'A'..='Z' => c.to_ascii_lowercase(),
            _ => '-',
        })
        .collect::<String>();

    let mut compact = String::with_capacity(sanitized.len());
    let mut last_dash = false;
    for c in sanitized.chars() {
        if c == '-' {
            if !last_dash {
                compact.push(c);
            }
            last_dash = true;
        } else {
            compact.push(c);
            last_dash = false;
        }
    }

    let mut compact = compact.trim_matches('-').to_string();
    if compact.is_empty() {
        compact = "fi".to_string();
    }
    if compact.len() <= max_len {
        return compact;
    }

    let mut truncated = compact[..max_len].trim_end_matches('-').to_string();
    if truncated.is_empty() {
        truncated = compact.chars().take(max_len).collect();
    }
    truncated
}

#[must_use]
pub fn bounded_name_preserving_suffix(raw_prefix: &str, suffix: &str, max_len: usize) -> String {
    let suffix_len = suffix.len();
    if suffix_len >= max_len {
        return bounded_name(suffix, max_len);
    }
    let prefix_max_len = max_len - suffix_len;
    let prefix = bounded_name(raw_prefix, prefix_max_len);
    format!("{prefix}{suffix}")
}

#[must_use]
pub fn time_nonce() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let val = u32::try_from(nanos % (36u128.pow(4))).unwrap_or(0);
    base36_pad4(val)
}

fn base36_pad4(mut n: u32) -> String {
    let mut buf = ['0'; 4];
    for idx in (0..4).rev() {
        let digit = (n % 36) as u8;
        buf[idx] = match digit {
            0..=9 => (b'0' + digit) as char,
            _ => (b'a' + (digit - 10)) as char,
        };
        n /= 36;
    }
    buf.iter().collect()
}

#[cfg(test)]
mod tests {
    use k8s_openapi::api::batch::v1::JobCondition;
    use serde_json::json;

    use super::*;

    #[test]
    fn canonical_hash_is_stable_for_object_key_order() {
        let a = json!({"b": 1, "a": {"z": 1, "m": [3, 2, 1]}});
        let b = json!({"a": {"m": [3, 2, 1], "z": 1}, "b": 1});

        let (a_content, a_hash) = manifest_content_and_hash(&a).unwrap();
        let (b_content, b_hash) = manifest_content_and_hash(&b).unwrap();

        assert_eq!(a_content, b_content);
        assert_eq!(a_hash, b_hash);
    }

    #[test]
    fn generated_names_are_dns_compatible_and_bounded() {
        let fi_name = "My__Very.Long_FrontendIntegration.Name";
        let hash = "sha256:0123456789abcdef";
        let job = job_name(fi_name, hash);
        let secret = secret_name(fi_name, hash, "ab12");
        let bundle = default_bundle_name(fi_name);

        for name in [job, secret, bundle] {
            assert!(name.len() <= 63);
            assert!(!name.starts_with('-'));
            assert!(!name.ends_with('-'));
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            );
        }
    }

    #[test]
    fn job_name_is_deterministic_for_same_hash() {
        let fi_name = "demo";
        let hash = "sha256:0123456789abcdef";

        assert_eq!(job_name(fi_name, hash), job_name(fi_name, hash));
    }

    #[test]
    fn package_resource_names_are_deterministic_and_bounded() {
        let artifact_key = "sha256:0123456789abcdef";
        let suffix = hash_name_suffix(artifact_key);
        let job = package_job_name("Very.Long_FrontendExtension.Name", artifact_key, 1);
        let cm = artifact_configmap_name("Very.Long_Package.Name", artifact_key);
        let publish_job = publish_job_name("Very.Long_FrontendExtension.Name", "20260420-100000");

        assert_eq!(
            job,
            package_job_name("Very.Long_FrontendExtension.Name", artifact_key, 1)
        );
        assert_eq!(
            cm,
            artifact_configmap_name("Very.Long_Package.Name", artifact_key)
        );
        assert!(job.ends_with(&format!("-{suffix}-a1")));
        assert!(cm.ends_with(&format!("-{suffix}")));
        assert_eq!(
            publish_job,
            publish_job_name("Very.Long_FrontendExtension.Name", "20260420-100000")
        );
        for name in [job, cm, publish_job] {
            assert!(name.len() <= 63);
            assert!(!name.starts_with('-'));
            assert!(!name.ends_with('-'));
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            );
        }
    }

    #[test]
    fn hash_label_value_is_label_safe() {
        let hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let v = hash_label_value(hash);
        assert_eq!(v.len(), 63);
        assert!(v.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn artifact_key_uses_trimmed_rebuild_token_identity() {
        let source_hash = "sha256:source";
        let missing = artifact_key(source_hash, "").unwrap();
        let whitespace = artifact_key(source_hash, "   ".trim()).unwrap();
        let token = artifact_key(source_hash, "token-1").unwrap();
        let token_with_trim = artifact_key(source_hash, " token-1 ".trim()).unwrap();

        assert_eq!(missing, whitespace);
        assert_eq!(token, token_with_trim);
        assert_ne!(missing, token);
    }

    #[test]
    fn artifact_key_changes_independently_from_source_hash() {
        let source_hash = "sha256:source";

        assert_ne!(
            artifact_key(source_hash, "").unwrap(),
            artifact_key(source_hash, "token-1").unwrap()
        );
        assert_ne!(
            artifact_key(source_hash, "token-1").unwrap(),
            artifact_key("sha256:other", "token-1").unwrap()
        );
    }

    #[test]
    fn observed_job_phase_uses_terminal_conditions_before_counters() {
        let status = JobStatus {
            active: Some(1),
            failed: Some(1),
            conditions: Some(vec![JobCondition {
                type_: "Complete".to_string(),
                status: "True".to_string(),
                ..Default::default()
            }]),
            ..Default::default()
        };

        assert_eq!(
            observed_job_phase(Some(&status)),
            ObservedJobPhase::Succeeded
        );
    }

    #[test]
    fn observed_job_phase_treats_active_retry_as_running() {
        let status = JobStatus {
            active: Some(1),
            failed: Some(1),
            ..Default::default()
        };

        assert_eq!(observed_job_phase(Some(&status)), ObservedJobPhase::Running);
    }

    #[test]
    fn observed_job_phase_allows_success_after_failed_retries() {
        let status = JobStatus {
            succeeded: Some(1),
            failed: Some(1),
            ..Default::default()
        };

        assert_eq!(
            observed_job_phase(Some(&status)),
            ObservedJobPhase::Succeeded
        );
    }

    #[test]
    fn artifact_key_canonical_json_includes_key_version_v1() {
        let expected = manifest_hash_from_content(
            r#"{"keyVersion":"v1","rebuildToken":"token-1","sourceHash":"sha256:source"}"#,
        );

        assert_eq!(artifact_key("sha256:source", "token-1").unwrap(), expected);
    }

    #[test]
    fn hash_name_suffix_is_twelve_characters() {
        let hash = "sha256:0123456789abcdef";

        assert_eq!(hash_name_suffix(hash), "0123456789ab");
        assert_eq!(hash_name_suffix(""), "0");
    }

    #[test]
    fn package_resource_names_preserve_suffix_for_long_names() {
        let artifact_key = "sha256:d46b92fa1234abcdef";
        let suffix = hash_name_suffix(artifact_key);
        let long_name = "very-long-frontend-extension-name-that-would-otherwise-drop-the-suffix";

        let job = package_job_name(long_name, artifact_key, 27);
        let cm = artifact_configmap_name(long_name, artifact_key);

        assert_eq!(job.len(), 63);
        assert_eq!(cm.len(), 63);
        assert!(job.ends_with(&format!("-{suffix}-a27")));
        assert!(cm.ends_with(&format!("-{suffix}")));
    }
}
