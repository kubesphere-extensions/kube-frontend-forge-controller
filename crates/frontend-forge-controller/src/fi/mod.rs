use std::{collections::BTreeMap, sync::Arc, time::Duration};

use chrono::Utc;
use frontend_forge_api::{
    FrontendIntegration, FrontendIntegrationPhase, FrontendIntegrationStatus, JSBundle,
    LastBuildError, LastBuildStatus, ResourceRef,
};
use frontend_forge_common::{
    ANNO_MANIFEST_HASH, ANNO_OBSERVED_GENERATION, BUILD_KIND_VALUE, CommonError, LABEL_BUILD_KIND,
    LABEL_ENABLED, LABEL_FI_NAME, LABEL_MANAGED_BY, LABEL_MANIFEST_HASH, LABEL_SPEC_HASH,
    MANAGED_BY_VALUE, ObservedJobPhase, base_owner_ref, create_or_get_job, default_bundle_name,
    extract_job_message, hash_label_value, job_name, observed_job_phase, serializable_hash,
};
use futures::StreamExt;
use k8s_openapi::{
    api::{
        batch::v1::{Job, JobSpec},
        core::v1::{Container, EnvVar, PodSpec, PodTemplateSpec},
    },
    apimachinery::pkg::apis::meta::v1::ObjectMeta,
};
use kube::{
    Api, Resource, ResourceExt,
    api::{ListParams, Patch, PatchParams},
};
use kube_runtime::{
    controller::{Action, Controller},
    watcher,
};
use serde_json::json;
use snafu::ResultExt;
use tracing::{error, info, warn};

use super::{
    CommonSnafu, ContextData, ControllerConfig, Error, GetFrontendIntegrationSnafu,
    GetJsBundleSnafu, ListJobsForHashSnafu, PatchFrontendIntegrationMetadataSnafu,
    PatchFrontendIntegrationStatusSnafu, SerializeFrontendIntegrationStatusPatchSnafu,
};

mod build;
mod controller;
mod jsbundle;
mod status;

#[cfg(test)]
mod tests;

pub(crate) use build::*;
pub(crate) use controller::run;
pub(crate) use jsbundle::*;
pub(crate) use status::*;

const JSBUNDLE_STATE_AVAILABLE: &str = "Available";
const JSBUNDLE_STATE_DISABLED: &str = "Disabled";
