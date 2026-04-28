use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::fi::{PageSpec, PrimaryMenuSpec};

#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[kube(
    group = "frontend-forge.kubesphere.io",
    version = "v1alpha1",
    kind = "FrontendExtension",
    plural = "frontendextensions",
    status = "FrontendExtensionStatus",
    shortname = "fe"
)]
pub struct FrontendExtensionSpec {
    pub package: FrontendExtensionPackageSpec,
    pub source: FrontendExtensionSourceSpec,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "publishPolicy"
    )]
    pub publish_policy: Option<PublishPolicySpec>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct FrontendExtensionPackageSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub version: String,
    #[serde(rename = "displayName")]
    pub display_name: BTreeMap<String, String>,
    pub description: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "kubeVersion"
    )]
    pub kube_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "ksVersion")]
    pub ks_version: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub maintainers: Vec<ExtensionMaintainerSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub provider: BTreeMap<String, ExtensionProviderSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "staticFileDirectory"
    )]
    pub static_file_directory: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<Vec<ExtensionDependencySpec>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "installationMode"
    )]
    pub installation_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub charts: Option<ExtensionChartsSpec>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ExtensionMaintainerSpec {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ExtensionProviderSpec {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ExtensionDependencySpec {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
pub struct ExtensionChartsSpec {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub values: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct FrontendExtensionSourceSpec {
    #[serde(rename = "type")]
    pub type_: FrontendExtensionSourceType,
    pub inline: InlineFrontendExtensionSourceSpec,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub enum FrontendExtensionSourceType {
    Inline,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct InlineFrontendExtensionSourceSpec {
    #[serde(rename = "schemaVersion")]
    pub schema_version: String,
    pub frontend: FrontendExtensionFrontendSpec,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "extensionResources"
    )]
    pub extension_resources: Option<ExtensionResourcesSpec>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
pub struct FrontendExtensionFrontendSpec {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "displayName"
    )]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub locales: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub menus: Vec<PrimaryMenuSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pages: Vec<PageSpec>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
pub struct ExtensionResourcesSpec {
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "jsBundle")]
    pub js_bundle: Option<ExtensionJsBundleSpec>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ExtensionJsBundleSpec {
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PublishPolicySpec {
    pub mode: PublishPolicyMode,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "defaultTargetRef"
    )]
    pub default_target_ref: Option<NamespacedResourceRef>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub enum PublishPolicyMode {
    Manual,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
pub struct NamespacedResourceRef {
    pub namespace: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "PascalCase")]
pub enum FrontendExtensionPhase {
    #[default]
    Pending,
    Packaging,
    Ready,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub enum PackageJobPhase {
    Pending,
    Running,
    Succeeded,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "PascalCase")]
pub enum PublishPhase {
    #[default]
    NotRequested,
    Pending,
    Running,
    Succeeded,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
pub struct FrontendExtensionStatus {
    #[serde(default)]
    pub phase: FrontendExtensionPhase,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "observedGeneration"
    )]
    pub observed_generation: Option<i64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "observedSourceHash"
    )]
    pub observed_source_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<ExtensionArtifactStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download: Option<ExtensionDownloadStatus>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "packageJob"
    )]
    pub package_job: Option<PackageJobStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publish: Option<PublishStatus>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<ExtensionCondition>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ExtensionArtifactStatus {
    pub storage: ArtifactStorageStatus,
    pub digest: String,
    #[serde(rename = "sizeBytes")]
    pub size_bytes: i64,
    #[serde(rename = "mediaType")]
    pub media_type: String,
    pub filename: String,
    #[serde(rename = "generatedAt")]
    pub generated_at: DateTime<Utc>,
    #[serde(rename = "sourceHash")]
    pub source_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ArtifactStorageStatus {
    pub kind: ArtifactStorageKind,
    #[serde(rename = "ref")]
    pub ref_: NamespacedResourceRef,
    pub key: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub enum ArtifactStorageKind {
    ConfigMap,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ExtensionDownloadStatus {
    pub ready: bool,
    pub filename: String,
    #[serde(rename = "mediaType")]
    pub media_type: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PackageJobStatus {
    pub namespace: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,
    pub phase: PackageJobPhase,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "startedAt")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "finishedAt"
    )]
    pub finished_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
pub struct PublishStatus {
    #[serde(default)]
    pub phase: PublishPhase,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "requestId")]
    pub request_id: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "artifactDigest"
    )]
    pub artifact_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "jobRef")]
    pub job_ref: Option<NamespacedResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "startedAt")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "finishedAt"
    )]
    pub finished_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "lastError")]
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ExtensionCondition {
    #[serde(rename = "type")]
    pub type_: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "observedGeneration"
    )]
    pub observed_generation: Option<i64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "lastTransitionTime"
    )]
    pub last_transition_time: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend_extension_crd;

    #[test]
    fn deserializes_frontend_extension_inline_source() {
        let fe: FrontendExtension = serde_yaml::from_str(
            r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendExtension
metadata:
  name: inspecttask
spec:
  package:
    name: inspecttask
    version: 0.1.0
    displayName:
      zh: 巡检任务
      en: Inspect Task
    description:
      zh: InspectTask extension package
      en: InspectTask extension package
    category: dev-tools
    keywords:
      - Frontend
    sources:
      - https://github.com/kubesphere-extensions/frontend-forge
    kubeVersion: ">=1.23.0-0"
    ksVersion: ">=4.2.1-0"
    maintainers:
      - name: KubeSphere
        email: kubesphere@yunify.com
    home: https://kubesphere.com.cn/
    provider:
      zh:
        name: 北京青云科技股份有限公司
        email: kubesphere@yunify.com
        url: https://kubesphere.com.cn/
      en:
        name: QingCloud Technologies
        email: kubesphere@yunify.com
        url: https://kubesphere.co/
    icon: ./static/frontend-forge.ico
    dependencies:
      - name: frontend
        tags:
          - extension
      - name: frontend-forge
        tags:
          - extension
    installationMode: HostOnly
    images:
      - kubesphere/frontend-forge-console:v1.0.0
      - kubesphere/frontend-forge-controller:v1.0.0
      - kubesphere/frontend-forge-runner:v1.0.0
    charts:
      values: {}
  source:
    type: Inline
    inline:
      schemaVersion: v1
      frontend:
        locales:
          zh:
            title: 巡检任务
          en:
            title: Inspect Tasks
        menus:
          - displayName: Inspect Tasks
            key: inspecttasks
            placement: cluster
            type: page
        pages:
          - key: inspecttasks
            type: crdTable
            crdTable:
              group: kubeeye.kubesphere.io
              version: v1alpha2
              scope: Cluster
              names:
                kind: InspectTask
                plural: inspecttasks
              columns:
                - key: name
                  title: NAME
                  render:
                    type: text
                    path: metadata.name
      extensionResources:
        jsBundle:
          name: inspecttask
  publishPolicy:
    mode: Manual
    defaultTargetRef:
      namespace: extension-frontend-forge
      name: ksbuilder-publish-config
"#,
        )
        .unwrap();

        assert_eq!(fe.spec.package.version, "0.1.0");
        assert_eq!(fe.spec.package.name.as_deref(), Some("inspecttask"));
        assert_eq!(fe.spec.package.display_name["en"], "Inspect Task");
        assert_eq!(
            fe.spec.package.provider["en"].name,
            "QingCloud Technologies"
        );
        assert_eq!(fe.spec.package.static_file_directory, None);
        assert_eq!(
            fe.spec.package.dependencies.as_ref().unwrap()[0].tags,
            vec!["extension".to_string()]
        );
        assert_eq!(fe.spec.source.type_, FrontendExtensionSourceType::Inline);
        let inline = fe.spec.source.inline;
        assert_eq!(inline.schema_version, "v1");
        assert_eq!(inline.frontend.menus[0].key, "inspecttasks");
        let resources = inline.extension_resources.unwrap();
        assert_eq!(resources.js_bundle.unwrap().name, "inspecttask");
    }

    #[test]
    fn keeps_missing_package_dependencies_unset() {
        let fe: FrontendExtension = serde_yaml::from_str(
            r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendExtension
metadata:
  name: no-dependencies
spec:
  package:
    version: 0.1.0
    displayName:
      en: No Dependencies
    description:
      en: No Dependencies
  source:
    type: Inline
    inline:
      schemaVersion: v1
      frontend: {}
"#,
        )
        .unwrap();

        assert!(fe.spec.package.dependencies.is_none());
        assert_eq!(fe.spec.package.static_file_directory, None);
    }

    #[test]
    fn generated_frontend_extension_crd_uses_publish_shape() {
        let crd = frontend_extension_crd();
        let schema = serde_json::to_value(&crd).unwrap();
        let spec_properties = &schema["spec"]["versions"][0]["schema"]["openAPIV3Schema"]
            ["properties"]["spec"]["properties"];
        let package = &spec_properties["package"];
        let extension_resources = &spec_properties["source"]["properties"]["inline"]["properties"]
            ["extensionResources"]["properties"];
        let status_properties = &schema["spec"]["versions"][0]["schema"]["openAPIV3Schema"]
            ["properties"]["status"]["properties"];

        assert!(spec_properties.get("package").is_some());
        assert!(spec_properties.get("source").is_some());
        assert!(spec_properties.get("publishPolicy").is_some());
        assert_eq!(
            package["properties"]["displayName"]["type"],
            Value::String("object".to_string())
        );
        assert_eq!(
            package["properties"]["description"]["type"],
            Value::String("object".to_string())
        );
        assert!(package["properties"].get("kubeVersion").is_some());
        assert!(package["properties"].get("staticFileDirectory").is_some());
        assert!(
            package["properties"]["staticFileDirectory"]
                .get("default")
                .is_none()
        );
        assert!(package["properties"].get("dependencies").is_some());
        assert!(
            !package["required"]
                .as_array()
                .unwrap()
                .contains(&Value::String("dependencies".to_string()))
        );
        assert!(package["properties"].get("installationMode").is_some());
        assert!(extension_resources.get("jsBundle").is_some());
        assert!(extension_resources.get("roleTemplates").is_none());
        assert!(status_properties.get("observedGeneration").is_some());
        assert!(status_properties.get("observedSourceHash").is_some());
        assert!(status_properties.get("packageJob").is_some());
    }

    #[test]
    fn generated_frontend_extension_crd_requires_inline_source() {
        let crd = frontend_extension_crd();
        let schema = serde_json::to_value(&crd).unwrap();
        let source = &schema["spec"]["versions"][0]["schema"]["openAPIV3Schema"]["properties"]
            ["spec"]["properties"]["source"];
        let required = source["required"].as_array().unwrap();

        assert!(required.contains(&Value::String("type".to_string())));
        assert!(required.contains(&Value::String("inline".to_string())));
    }
}
