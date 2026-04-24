use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
};

use chrono::{DateTime, Utc};
use flate2::{Compression, GzBuilder};
use frontend_forge_api::{
    CrdScope, ExtensionChartsSpec, ExtensionDependencySpec, ExtensionMaintainerSpec,
    ExtensionProviderSpec, FrontendExtension, FrontendExtensionFrontendSpec,
    FrontendExtensionSourceType, MenuPlacement, PageType,
};
use frontend_forge_common::{CommonError, serializable_hash, sha256_hex};
use frontend_forge_manifest::{
    ManifestRenderError, ResolvedFrontendPage, resolve_frontend_extension_pages,
};
use include_dir::{Dir, include_dir};
use kube::ResourceExt;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use snafu::{OptionExt, ResultExt, Snafu};
use tar::{Builder as TarBuilder, Header};

pub const PACKAGE_KEY: &str = "package.tgz";
pub const PACKAGE_MEDIA_TYPE: &str = "application/gzip";
pub const ARTIFACT_METADATA_KEY: &str = "artifact.json";
pub const FILES_METADATA_KEY: &str = "files.json";

static PACKAGE_TEMPLATE_DIR: Dir<'_> =
    include_dir!("$CARGO_MANIFEST_DIR/../../template/test-fe-demo");

#[derive(Debug, Snafu)]
pub enum ExtensionPackageError {
    #[snafu(display("failed to hash FrontendExtension source identity: {source}"))]
    SourceHash { source: CommonError },
    #[snafu(display("failed to render frontend manifest: {source}"))]
    RenderManifest { source: ManifestRenderError },
    #[snafu(display("failed to serialize {name}: {source}"))]
    Serialize {
        name: &'static str,
        source: serde_yaml::Error,
    },
    #[snafu(display("failed to serialize {name}: {source}"))]
    SerializeJson {
        name: &'static str,
        source: serde_json::Error,
    },
    #[snafu(display("failed to build package archive: {source}"))]
    Archive { source: std::io::Error },
    #[snafu(display("package template file {path} is missing"))]
    TemplateMissing { path: &'static str },
    #[snafu(display("package template file {path} is not valid UTF-8"))]
    TemplateUtf8 { path: &'static str },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageFile {
    pub path: String,
    pub content: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageFileMeta {
    pub path: String,
    #[serde(rename = "sizeBytes")]
    pub size_bytes: usize,
    pub digest: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageArtifactMetadata {
    pub name: String,
    pub version: String,
    pub filename: String,
    #[serde(rename = "mediaType")]
    pub media_type: String,
    pub digest: String,
    #[serde(rename = "sizeBytes")]
    pub size_bytes: usize,
    #[serde(rename = "sourceHash")]
    pub source_hash: String,
    #[serde(rename = "generatedAt")]
    pub generated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigMapArtifactPayload {
    pub data: BTreeMap<String, String>,
    pub binary_data: BTreeMap<String, Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionPackageArtifact {
    pub filename: String,
    pub media_type: String,
    pub digest: String,
    pub size_bytes: usize,
    pub source_hash: String,
    pub generated_at: DateTime<Utc>,
    pub files: Vec<PackageFile>,
    pub payload: ConfigMapArtifactPayload,
}

#[derive(Serialize)]
struct SourceIdentity<'a> {
    package: NormalizedPackageIdentity<'a>,
    source: NormalizedSourceIdentity<'a>,
}

#[derive(Serialize)]
struct NormalizedPackageIdentity<'a> {
    name: &'a str,
    version: &'a str,
    #[serde(rename = "displayName")]
    display_name: &'a BTreeMap<String, String>,
    description: &'a BTreeMap<String, String>,
    category: &'a Option<String>,
    keywords: &'a Vec<String>,
    sources: &'a Vec<String>,
    #[serde(rename = "kubeVersion")]
    kube_version: &'a Option<String>,
    #[serde(rename = "ksVersion")]
    ks_version: &'a Option<String>,
    maintainers: &'a Vec<ExtensionMaintainerSpec>,
    home: &'a Option<String>,
    provider: &'a BTreeMap<String, ExtensionProviderSpec>,
    icon: &'a Option<String>,
    #[serde(rename = "installationMode")]
    installation_mode: &'a Option<String>,
    images: &'a Vec<String>,
    charts: &'a Option<ExtensionChartsSpec>,
}

#[derive(Serialize)]
struct NormalizedSourceIdentity<'a> {
    #[serde(rename = "type")]
    type_: &'a FrontendExtensionSourceType,
    inline: NormalizedInlineSourceIdentity<'a>,
}

#[derive(Serialize)]
struct NormalizedInlineSourceIdentity<'a> {
    #[serde(rename = "schemaVersion")]
    schema_version: &'a str,
    frontend: &'a FrontendExtensionFrontendSpec,
}

/// Build a deterministic ksbuilder extension package artifact.
///
/// # Errors
///
/// Returns an error when source hashing, frontend source validation,
/// `RoleTemplate` rendering, YAML/JSON serialization, or archive generation
/// fails.
pub fn build_extension_package(
    fe: &FrontendExtension,
    generated_at: DateTime<Utc>,
    index_js_content: &str,
) -> Result<ExtensionPackageArtifact, ExtensionPackageError> {
    let source_hash = frontend_extension_source_hash(fe)?;

    let mut files = package_files(fe, index_js_content)?;
    files.sort_by(|a, b| a.path.cmp(&b.path));

    let file_meta = package_file_meta(&files);
    let package_bytes = gzip_bytes(&tar_bytes(&files)?)?;
    let digest = format!("sha256:{}", sha256_hex(&package_bytes));
    let package_name = frontend_extension_package_name(fe);
    let filename = format!("{}-{}.tgz", package_name, fe.spec.package.version);

    let metadata = PackageArtifactMetadata {
        name: package_name,
        version: fe.spec.package.version.clone(),
        filename: filename.clone(),
        media_type: PACKAGE_MEDIA_TYPE.to_string(),
        digest: digest.clone(),
        size_bytes: package_bytes.len(),
        source_hash: source_hash.clone(),
        generated_at,
    };

    let artifact_json = serde_json::to_string_pretty(&metadata).context(SerializeJsonSnafu {
        name: ARTIFACT_METADATA_KEY,
    })?;
    let files_json = serde_json::to_string_pretty(&file_meta).context(SerializeJsonSnafu {
        name: FILES_METADATA_KEY,
    })?;

    Ok(ExtensionPackageArtifact {
        filename,
        media_type: PACKAGE_MEDIA_TYPE.to_string(),
        digest,
        size_bytes: package_bytes.len(),
        source_hash,
        generated_at,
        files,
        payload: ConfigMapArtifactPayload {
            data: BTreeMap::from([
                (ARTIFACT_METADATA_KEY.to_string(), artifact_json),
                (FILES_METADATA_KEY.to_string(), files_json),
            ]),
            binary_data: BTreeMap::from([(PACKAGE_KEY.to_string(), package_bytes)]),
        },
    })
}

fn package_files(
    fe: &FrontendExtension,
    index_js_content: &str,
) -> Result<Vec<PackageFile>, ExtensionPackageError> {
    let package_metadata = package_metadata(fe);
    let package_name = frontend_extension_package_name(fe);
    let helper_chart_name = helper_chart_name(&package_name);
    let pages = resolve_frontend_extension_pages(fe).context(RenderManifestSnafu)?;

    Ok(vec![
        yaml_file("extension.yaml", &package_metadata)?,
        template_text_file("permissions.yaml", "permissions.yaml")?,
        yaml_file("values.yaml", &root_values(fe, &helper_chart_name))?,
        template_binary_file("static/favicon.svg", "static/favicon.svg")?,
        yaml_file(
            "charts/frontend/Chart.yaml",
            &frontend_chart(fe, &package_name),
        )?,
        template_text_file("charts/frontend/values.yaml", "charts/frontend/values.yaml")?,
        frontend_script_file(index_js_content),
        template_text_file(
            "charts/frontend/templates/configmap.yaml",
            "charts/frontend/templates/configmap.yaml",
        )?,
        template_text_file(
            "charts/frontend/templates/extensions.yaml",
            "charts/frontend/templates/extensions.yaml",
        )?,
        template_text_file(
            "charts/frontend/templates/helps.tpl",
            "charts/frontend/templates/helps.tpl",
        )?,
        yaml_file(
            &format!("charts/{helper_chart_name}/Chart.yaml"),
            &helper_chart(fe, &helper_chart_name),
        )?,
        template_text_file(
            "charts/fe-demo-helper/values.yaml",
            format!("charts/{helper_chart_name}/values.yaml"),
        )?,
        text_file(
            &format!("charts/{helper_chart_name}/templates/roleTemplate.yaml"),
            &role_template_template(&package_name, &pages)?,
        ),
    ])
}

#[derive(Serialize)]
struct KsbuilderExtensionYaml {
    #[serde(rename = "apiVersion")]
    api_version: String,
    name: String,
    version: String,
    #[serde(rename = "displayName")]
    display_name: BTreeMap<String, String>,
    description: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    category: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    keywords: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    sources: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "kubeVersion")]
    kube_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "ksVersion")]
    ks_version: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    maintainers: Vec<ExtensionMaintainerSpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    home: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    provider: BTreeMap<String, ExtensionProviderSpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    dependencies: Vec<ExtensionDependencySpec>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "installationMode")]
    installation_mode: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    images: Vec<String>,
}

fn package_metadata(fe: &FrontendExtension) -> KsbuilderExtensionYaml {
    let package = &fe.spec.package;
    let package_name = frontend_extension_package_name(fe);
    KsbuilderExtensionYaml {
        api_version: "kubesphere.io/v1alpha1".to_string(),
        name: package_name.clone(),
        version: package.version.clone(),
        display_name: package.display_name.clone(),
        description: package.description.clone(),
        category: package.category.clone(),
        keywords: package.keywords.clone(),
        sources: package.sources.clone(),
        kube_version: package.kube_version.clone(),
        ks_version: package.ks_version.clone(),
        maintainers: package.maintainers.clone(),
        home: package.home.clone(),
        provider: package.provider.clone(),
        icon: package.icon.clone(),
        dependencies: package_dependencies(&package_name),
        installation_mode: package.installation_mode.clone(),
        images: package.images.clone(),
    }
}

fn package_dependencies(package_name: &str) -> Vec<ExtensionDependencySpec> {
    vec![
        ExtensionDependencySpec {
            name: helper_chart_name(package_name),
            tags: vec!["agent".to_string()],
        },
        ExtensionDependencySpec {
            name: "frontend".to_string(),
            tags: vec!["extension".to_string()],
        },
    ]
}

#[derive(Serialize)]
struct HelmChartYaml {
    #[serde(rename = "apiVersion")]
    api_version: String,
    name: String,
    description: String,
    #[serde(rename = "type")]
    type_: String,
    version: String,
    #[serde(rename = "appVersion")]
    app_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    home: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    sources: Vec<String>,
}

fn frontend_chart(fe: &FrontendExtension, package_name: &str) -> HelmChartYaml {
    HelmChartYaml {
        api_version: "v2".to_string(),
        name: "frontend".to_string(),
        description: format!("Frontend of {package_name} extension."),
        type_: "application".to_string(),
        version: fe.spec.package.version.clone(),
        app_version: fe.spec.package.version.clone(),
        home: fe.spec.package.home.clone(),
        sources: fe.spec.package.sources.clone(),
    }
}

fn helper_chart(fe: &FrontendExtension, helper_chart_name: &str) -> HelmChartYaml {
    HelmChartYaml {
        api_version: "v2".to_string(),
        name: helper_chart_name.to_string(),
        description: format!("Helper resources for {helper_chart_name}."),
        type_: "application".to_string(),
        version: fe.spec.package.version.clone(),
        app_version: fe.spec.package.version.clone(),
        home: None,
        sources: Vec::new(),
    }
}

fn root_values(fe: &FrontendExtension, helper_chart_name: &str) -> BTreeMap<String, Value> {
    let mut values = fe
        .spec
        .package
        .charts
        .as_ref()
        .map(|charts| charts.values.clone())
        .unwrap_or_default();

    let helper = values
        .entry(helper_chart_name.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if let Value::Object(helper) = helper {
        let role_template = helper
            .entry("roleTemplate".to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if let Value::Object(role_template) = role_template {
            role_template
                .entry("enabled".to_string())
                .or_insert(Value::Bool(true));
        }
    }

    values
}

fn helper_chart_name(package_name: &str) -> String {
    format!("{package_name}-helper")
}

#[must_use]
pub fn frontend_extension_package_name(fe: &FrontendExtension) -> String {
    fe.spec
        .package
        .name
        .clone()
        .unwrap_or_else(|| fe.name_any())
}

/// Calculate the normalized source hash used by package jobs and artifacts.
///
/// # Errors
///
/// Returns an error if the source identity cannot be serialized into canonical
/// JSON for hashing.
pub fn frontend_extension_source_hash(
    fe: &FrontendExtension,
) -> Result<String, ExtensionPackageError> {
    let inline = &fe.spec.source.inline;
    let package = &fe.spec.package;
    let package_name = frontend_extension_package_name(fe);
    serializable_hash(&SourceIdentity {
        package: NormalizedPackageIdentity {
            name: &package_name,
            version: &package.version,
            display_name: &package.display_name,
            description: &package.description,
            category: &package.category,
            keywords: &package.keywords,
            sources: &package.sources,
            kube_version: &package.kube_version,
            ks_version: &package.ks_version,
            maintainers: &package.maintainers,
            home: &package.home,
            provider: &package.provider,
            icon: &package.icon,
            installation_mode: &package.installation_mode,
            images: &package.images,
            charts: &package.charts,
        },
        source: NormalizedSourceIdentity {
            type_: &fe.spec.source.type_,
            inline: NormalizedInlineSourceIdentity {
                schema_version: &inline.schema_version,
                frontend: &inline.frontend,
            },
        },
    })
    .context(SourceHashSnafu)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum RoleScope {
    Cluster,
    Namespace,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum RoleAction {
    View,
    Manage,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct GeneratedRule {
    api_group: String,
    resource: String,
    verbs: Vec<String>,
}

#[derive(Clone, Debug, Default)]
struct RoleTemplateAggregate {
    action_keys: BTreeSet<String>,
    rules: BTreeSet<GeneratedRule>,
}

#[derive(Clone, Debug, Default)]
struct RoleTemplateAggregates {
    cluster_view: RoleTemplateAggregate,
    cluster_manage: RoleTemplateAggregate,
    namespace_view: RoleTemplateAggregate,
    namespace_manage: RoleTemplateAggregate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RoleTemplateContribution {
    scope: RoleScope,
    action: RoleAction,
    action_key: String,
    rule: Option<GeneratedRule>,
}

fn role_template_template(
    package_name: &str,
    pages: &[ResolvedFrontendPage],
) -> Result<String, ExtensionPackageError> {
    let aggregates = role_template_aggregates(pages);
    let mut out = String::from("{{- if .Values.roleTemplate.enabled }}\n");

    if aggregates.has_scope(RoleScope::Cluster) {
        out.push_str(&category_template(
            RoleScope::Cluster,
            "cluster-fe-management",
            "Quick Integration",
            "快速集成",
        ));
    }
    append_role_template(
        &mut out,
        package_name,
        RoleScope::Cluster,
        RoleAction::View,
        &aggregates.cluster_view,
    )?;
    append_role_template(
        &mut out,
        package_name,
        RoleScope::Cluster,
        RoleAction::Manage,
        &aggregates.cluster_manage,
    )?;

    if aggregates.has_scope(RoleScope::Namespace) {
        out.push_str(&category_template(
            RoleScope::Namespace,
            "namespace-fe-management",
            "Quick Integration",
            "快速集成",
        ));
    }
    append_role_template(
        &mut out,
        package_name,
        RoleScope::Namespace,
        RoleAction::View,
        &aggregates.namespace_view,
    )?;
    append_role_template(
        &mut out,
        package_name,
        RoleScope::Namespace,
        RoleAction::Manage,
        &aggregates.namespace_manage,
    )?;

    out.push_str("{{- end }}\n");
    Ok(out)
}

impl RoleTemplateAggregates {
    const fn aggregate_mut(
        &mut self,
        scope: RoleScope,
        action: RoleAction,
    ) -> &mut RoleTemplateAggregate {
        match (scope, action) {
            (RoleScope::Cluster, RoleAction::View) => &mut self.cluster_view,
            (RoleScope::Cluster, RoleAction::Manage) => &mut self.cluster_manage,
            (RoleScope::Namespace, RoleAction::View) => &mut self.namespace_view,
            (RoleScope::Namespace, RoleAction::Manage) => &mut self.namespace_manage,
        }
    }

    fn has_scope(&self, scope: RoleScope) -> bool {
        match scope {
            RoleScope::Cluster => !self.cluster_view.is_empty() || !self.cluster_manage.is_empty(),
            RoleScope::Namespace => {
                !self.namespace_view.is_empty() || !self.namespace_manage.is_empty()
            }
        }
    }
}

impl RoleTemplateAggregate {
    fn is_empty(&self) -> bool {
        self.action_keys.is_empty()
    }
}

fn role_template_aggregates(pages: &[ResolvedFrontendPage]) -> RoleTemplateAggregates {
    let mut aggregates = RoleTemplateAggregates::default();

    for page in pages {
        for contribution in role_template_contributions(page) {
            add_role_rule(&mut aggregates, contribution);
        }
    }

    aggregates
}

fn role_template_contributions(page: &ResolvedFrontendPage) -> Vec<RoleTemplateContribution> {
    match page.page.type_ {
        PageType::CrdTable => crd_table_role_template_contributions(page),
        PageType::Iframe => iframe_role_template_contribution(page)
            .into_iter()
            .collect(),
    }
}

fn crd_table_role_template_contributions(
    page: &ResolvedFrontendPage,
) -> Vec<RoleTemplateContribution> {
    let Some(crd) = page.page.crd_table.as_ref() else {
        return Vec::new();
    };
    let scope = crd_role_scope(page.placement, crd.scope);
    let action_key = crd
        .auth_key
        .clone()
        .unwrap_or_else(|| page.action_key.clone());

    vec![
        RoleTemplateContribution {
            scope,
            action: RoleAction::View,
            action_key: action_key.clone(),
            rule: Some(GeneratedRule {
                api_group: crd.group.clone(),
                resource: crd.names.plural.clone(),
                verbs: view_verbs(),
            }),
        },
        RoleTemplateContribution {
            scope,
            action: RoleAction::Manage,
            action_key,
            rule: Some(GeneratedRule {
                api_group: crd.group.clone(),
                resource: crd.names.plural.clone(),
                verbs: manage_verbs(),
            }),
        },
    ]
}

fn iframe_role_template_contribution(
    page: &ResolvedFrontendPage,
) -> Option<RoleTemplateContribution> {
    let scope = iframe_role_scope(page.placement)?;
    Some(RoleTemplateContribution {
        scope,
        action: RoleAction::View,
        action_key: page.action_key.clone(),
        rule: None,
    })
}

fn add_role_rule(aggregates: &mut RoleTemplateAggregates, contribution: RoleTemplateContribution) {
    let aggregate = aggregates.aggregate_mut(contribution.scope, contribution.action);
    aggregate.action_keys.insert(contribution.action_key);
    if let Some(rule) = contribution.rule {
        aggregate.rules.insert(rule);
    }
}

fn view_verbs() -> Vec<String> {
    ["get", "list", "watch"]
        .into_iter()
        .map(str::to_string)
        .collect()
}

fn manage_verbs() -> Vec<String> {
    vec!["*".to_string()]
}

const fn crd_role_scope(placement: MenuPlacement, crd_scope: CrdScope) -> RoleScope {
    match placement {
        MenuPlacement::Cluster => RoleScope::Cluster,
        MenuPlacement::Workspace => RoleScope::Namespace,
        MenuPlacement::Global => match crd_scope {
            CrdScope::Cluster => RoleScope::Cluster,
            CrdScope::Namespaced => RoleScope::Namespace,
        },
    }
}

const fn iframe_role_scope(placement: MenuPlacement) -> Option<RoleScope> {
    match placement {
        MenuPlacement::Cluster => Some(RoleScope::Cluster),
        MenuPlacement::Workspace => Some(RoleScope::Namespace),
        MenuPlacement::Global => None,
    }
}

fn category_template(
    scope: RoleScope,
    category_name: &str,
    display_name_en: &str,
    display_name_zh: &str,
) -> String {
    let scope_name = role_scope_name(scope);
    format!(
        r#"---
{{{{- $existing := lookup "iam.kubesphere.io/v1beta1" "Category" "" "{category_name}" -}}}}

{{{{- if not $existing }}}}
apiVersion: iam.kubesphere.io/v1beta1
kind: Category
metadata:
  name: {category_name}
  annotations:
    "helm.sh/resource-policy": keep
  labels:
    iam.kubesphere.io/scope: {scope_name}
    kubesphere.io/managed: "true"
spec:
  displayName:
    en: {display_name_en}
    zh: {display_name_zh}
{{{{- end }}}}
"#
    )
}

#[allow(clippy::format_push_string)]
fn append_role_template(
    out: &mut String,
    package_name: &str,
    scope: RoleScope,
    action: RoleAction,
    aggregate: &RoleTemplateAggregate,
) -> Result<(), ExtensionPackageError> {
    if aggregate.is_empty() {
        return Ok(());
    }

    let scope_name = role_scope_name(scope);
    let action_name = role_action_name(action);
    let role_name = format!("{scope_name}-{action_name}-{package_name}");
    let category = format!("{scope_name}-fe-management");
    let annotation = role_action_annotation(&aggregate.action_keys, action)?;
    let dependency = if action == RoleAction::Manage {
        Some(format!("{scope_name}-view-{package_name}"))
    } else {
        None
    };

    out.push_str("---\n");
    out.push_str("apiVersion: iam.kubesphere.io/v1beta1\n");
    out.push_str("kind: RoleTemplate\n");
    out.push_str("metadata:\n");
    out.push_str("  annotations:\n");
    if let Some(dependency) = dependency {
        out.push_str(&format!(
            "    iam.kubesphere.io/dependencies: '[\"{dependency}\"]'\n"
        ));
    }
    out.push_str(&format!(
        "    iam.kubesphere.io/role-template-rules: '{}'\n",
        annotation.replace('\'', "''")
    ));
    out.push_str("  labels:\n");
    append_role_labels(out, scope, action, &category);
    out.push_str(&format!("  name: {role_name}\n"));
    out.push_str("spec:\n");
    append_role_description(out, package_name, scope, action);
    append_role_display_name(out, package_name, action);
    append_rules(out, &aggregate.rules);
    Ok(())
}

fn role_action_annotation(
    action_keys: &BTreeSet<String>,
    action: RoleAction,
) -> Result<String, ExtensionPackageError> {
    let values = action_keys
        .iter()
        .map(|key| (key.clone(), role_action_name(action).to_string()))
        .collect::<BTreeMap<_, _>>();
    serde_json::to_string(&values).context(SerializeJsonSnafu {
        name: "role-template-rules annotation",
    })
}

#[allow(clippy::format_push_string)]
fn append_role_labels(out: &mut String, scope: RoleScope, action: RoleAction, category: &str) {
    match (scope, action) {
        (RoleScope::Cluster, RoleAction::View) => {
            out.push_str("    iam.kubesphere.io/aggregate-to-cluster-viewer: \"\"\n");
        }
        (RoleScope::Namespace, RoleAction::View) => {
            out.push_str("    iam.kubesphere.io/aggregate-to-viewer: \"\"\n");
            out.push_str("    iam.kubesphere.io/aggregate-to-operator: \"\"\n");
        }
        (RoleScope::Namespace, RoleAction::Manage) => {
            out.push_str("    iam.kubesphere.io/aggregate-to-operator: \"\"\n");
        }
        (RoleScope::Cluster, RoleAction::Manage) => {}
    }
    out.push_str(&format!("    iam.kubesphere.io/category: {category}\n"));
    out.push_str(&format!(
        "    iam.kubesphere.io/scope: {}\n",
        role_scope_name(scope)
    ));
    out.push_str("    kubesphere.io/managed: \"true\"\n");
}

#[allow(clippy::format_push_string)]
fn append_role_description(
    out: &mut String,
    package_name: &str,
    scope: RoleScope,
    action: RoleAction,
) {
    out.push_str("  description:\n");
    match (scope, action) {
        (_, RoleAction::View) => {
            out.push_str(&format!("    en: View {package_name} list.\n"));
            out.push_str(&format!("    zh: 查看 {package_name} 列表。\n"));
        }
        (RoleScope::Cluster, RoleAction::Manage) => {
            out.push_str(&format!("    en: Manage {package_name}.\n"));
            out.push_str(&format!("    zh: 管理 {package_name}。\n"));
        }
        (RoleScope::Namespace, RoleAction::Manage) => {
            out.push_str(&format!("    en: Namespace {package_name} management.\n"));
            out.push_str(&format!("    zh: 项目 {package_name} 管理。\n"));
        }
    }
}

#[allow(clippy::format_push_string)]
fn append_role_display_name(out: &mut String, package_name: &str, action: RoleAction) {
    out.push_str("  displayName:\n");
    match action {
        RoleAction::View => {
            out.push_str(&format!("    en: View {package_name} List\n"));
            out.push_str(&format!("    zh: 查看 {package_name} 列表\n"));
        }
        RoleAction::Manage => {
            out.push_str(&format!("    en: Manage {package_name}\n"));
            out.push_str(&format!("    zh: 管理 {package_name}\n"));
        }
    }
}

#[allow(clippy::format_push_string)]
fn append_rules(out: &mut String, rules: &BTreeSet<GeneratedRule>) {
    if rules.is_empty() {
        out.push_str("  rules: []\n");
        return;
    }

    out.push_str("  rules:\n");
    for rule in rules {
        out.push_str("  - apiGroups:\n");
        out.push_str(&format!("    - '{}'\n", rule.api_group.replace('\'', "''")));
        out.push_str("    resources:\n");
        out.push_str(&format!("    - '{}'\n", rule.resource.replace('\'', "''")));
        out.push_str("    verbs:\n");
        for verb in &rule.verbs {
            out.push_str(&format!("    - {verb}\n"));
        }
    }
}

const fn role_scope_name(scope: RoleScope) -> &'static str {
    match scope {
        RoleScope::Cluster => "cluster",
        RoleScope::Namespace => "namespace",
    }
}

const fn role_action_name(action: RoleAction) -> &'static str {
    match action {
        RoleAction::View => "view",
        RoleAction::Manage => "manage",
    }
}

fn frontend_script_file(index_js_content: &str) -> PackageFile {
    PackageFile {
        path: "charts/frontend/scripts/index.js".to_string(),
        content: index_js_content.as_bytes().to_vec(),
    }
}

fn template_text_file(
    source_path: &'static str,
    output_path: impl Into<String>,
) -> Result<PackageFile, ExtensionPackageError> {
    Ok(PackageFile {
        path: output_path.into(),
        content: template_text(source_path)?.as_bytes().to_vec(),
    })
}

fn template_binary_file(
    source_path: &'static str,
    output_path: impl Into<String>,
) -> Result<PackageFile, ExtensionPackageError> {
    Ok(PackageFile {
        path: output_path.into(),
        content: template_bytes(source_path)?.to_vec(),
    })
}

fn template_text(path: &'static str) -> Result<&'static str, ExtensionPackageError> {
    PACKAGE_TEMPLATE_DIR
        .get_file(path)
        .context(TemplateMissingSnafu { path })?
        .contents_utf8()
        .context(TemplateUtf8Snafu { path })
}

fn template_bytes(path: &'static str) -> Result<&'static [u8], ExtensionPackageError> {
    Ok(PACKAGE_TEMPLATE_DIR
        .get_file(path)
        .context(TemplateMissingSnafu { path })?
        .contents())
}

fn text_file(path: &str, content: &str) -> PackageFile {
    PackageFile {
        path: path.to_string(),
        content: content.as_bytes().to_vec(),
    }
}

fn yaml_file<T>(path: &str, value: &T) -> Result<PackageFile, ExtensionPackageError>
where
    T: Serialize,
{
    let content = serde_yaml::to_string(value).context(SerializeSnafu { name: "yaml file" })?;
    Ok(PackageFile {
        path: path.to_string(),
        content: content.into_bytes(),
    })
}

fn package_file_meta(files: &[PackageFile]) -> Vec<PackageFileMeta> {
    files
        .iter()
        .map(|file| PackageFileMeta {
            path: file.path.clone(),
            size_bytes: file.content.len(),
            digest: format!("sha256:{}", sha256_hex(&file.content)),
        })
        .collect()
}

fn tar_bytes(files: &[PackageFile]) -> Result<Vec<u8>, ExtensionPackageError> {
    let mut builder = TarBuilder::new(Vec::new());

    for file in files {
        append_tar_file(&mut builder, file)?;
    }

    builder.finish().context(ArchiveSnafu)?;
    builder.into_inner().context(ArchiveSnafu)
}

fn append_tar_file(
    builder: &mut TarBuilder<Vec<u8>>,
    file: &PackageFile,
) -> Result<(), ExtensionPackageError> {
    let mut header = Header::new_ustar();
    header.set_size(file.content.len() as u64);
    header.set_mode(0o644);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_cksum();

    builder
        .append_data(&mut header, file.path.as_str(), file.content.as_slice())
        .context(ArchiveSnafu)
}

fn gzip_bytes(input: &[u8]) -> Result<Vec<u8>, ExtensionPackageError> {
    let mut encoder = GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), Compression::default());
    encoder.write_all(input).context(ArchiveSnafu)?;
    encoder.finish().context(ArchiveSnafu)
}

#[cfg(test)]
mod tests {
    use frontend_forge_api::FrontendExtension;

    use super::*;

    fn sample_fe() -> FrontendExtension {
        serde_yaml::from_str(
            r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendExtension
metadata:
  name: fe-inspecttask
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
      values:
        replicaCount: 1
  source:
    type: Inline
    inline:
      schemaVersion: v1
      frontend:
        menus:
          - displayName: Inspect Tasks
            key: inspecttasks
            placement: cluster
            type: page
        pages:
          - key: inspecttasks
            type: iframe
            iframe:
              src: http://example.test
      extensionResources:
        jsBundle:
          name: inspecttask
        roleTemplates:
          - name: inspecttask-view
            displayName: InspectTask Viewer
            rules:
              - apiGroups: ["kubeeye.kubesphere.io"]
                resources: ["inspecttasks"]
                verbs: ["get", "list", "watch"]
"#,
        )
        .unwrap()
    }

    #[test]
    fn builds_extension_package_artifact_payload() {
        let generated_at = DateTime::from_timestamp(1_775_200_000, 0).unwrap();
        let artifact = build_extension_package(
            &sample_fe(),
            generated_at,
            "System.register([], function () {});",
        )
        .unwrap();

        assert_eq!(artifact.filename, "inspecttask-0.1.0.tgz");
        assert_eq!(artifact.media_type, PACKAGE_MEDIA_TYPE);
        assert!(artifact.digest.starts_with("sha256:"));
        assert_eq!(artifact.generated_at, generated_at);
        assert!(artifact.payload.data.contains_key(ARTIFACT_METADATA_KEY));
        assert!(artifact.payload.data.contains_key(FILES_METADATA_KEY));
        let artifact_metadata: Value =
            serde_json::from_str(&artifact.payload.data[ARTIFACT_METADATA_KEY]).unwrap();
        assert!(artifact_metadata.get("files").is_none());
        let file_metadata: Vec<PackageFileMeta> =
            serde_json::from_str(&artifact.payload.data[FILES_METADATA_KEY]).unwrap();
        assert!(!file_metadata.is_empty());
        assert_eq!(
            artifact.payload.binary_data[PACKAGE_KEY][..3],
            [0x1f, 0x8b, 0x08]
        );
        let paths = artifact
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>();
        assert!(paths.contains(&"extension.yaml"));
        assert!(paths.contains(&"permissions.yaml"));
        assert!(paths.contains(&"values.yaml"));
        assert!(paths.contains(&"charts/frontend/Chart.yaml"));
        assert!(paths.contains(&"charts/frontend/values.yaml"));
        assert!(paths.contains(&"charts/frontend/scripts/index.js"));
        assert!(paths.contains(&"charts/frontend/templates/configmap.yaml"));
        assert!(paths.contains(&"charts/frontend/templates/extensions.yaml"));
        assert!(paths.contains(&"charts/frontend/templates/helps.tpl"));
        assert!(paths.contains(&"charts/inspecttask-helper/Chart.yaml"));
        assert!(paths.contains(&"charts/inspecttask-helper/values.yaml"));
        assert!(paths.contains(&"charts/inspecttask-helper/templates/roleTemplate.yaml"));
        assert!(!paths.contains(&"frontend/manifest.json"));
        assert!(!paths.contains(&"frontend/Chart.yaml"));
        assert!(!paths.contains(&"charts/inspecttask/values.yaml"));
        assert!(!paths.contains(&"resources/jsbundle.yaml"));
        assert!(!paths.contains(&"resources/roletemplates/inspecttask-view.yaml"));

        let extension_yaml = artifact
            .files
            .iter()
            .find(|file| file.path == "extension.yaml")
            .unwrap();
        let content = std::str::from_utf8(&extension_yaml.content).unwrap();

        assert!(content.contains("apiVersion: kubesphere.io/v1alpha1"));
        assert!(content.contains("name: inspecttask"));
        assert!(content.contains("displayName:"));
        assert!(content.contains("name: inspecttask-helper"));
        assert!(content.contains("- agent"));
        assert!(content.contains("name: frontend"));
        assert!(content.contains("- extension"));
        assert!(!content.contains("name: frontend-forge\n"));
        assert!(content.contains("installationMode: HostOnly"));
        assert!(content.contains("kubesphere/frontend-forge-controller:v1.0.0"));

        let frontend_chart = artifact
            .files
            .iter()
            .find(|file| file.path == "charts/frontend/Chart.yaml")
            .unwrap();
        let content = std::str::from_utf8(&frontend_chart.content).unwrap();

        assert!(content.contains("apiVersion: v2"));
        assert!(content.contains("type: application"));

        let permissions = artifact
            .files
            .iter()
            .find(|file| file.path == "permissions.yaml")
            .unwrap();
        let content = std::str::from_utf8(&permissions.content).unwrap();

        assert!(content.contains("kind: ClusterRole"));
        assert!(content.contains("kind: Role"));
        assert!(content.contains("- 'categories'"));
        assert!(content.contains("- 'roletemplates'"));
    }

    #[test]
    fn frontend_configmap_loads_index_js_from_chart_file() {
        let generated_at = DateTime::from_timestamp(1_775_200_000, 0).unwrap();
        let artifact = build_extension_package(
            &sample_fe(),
            generated_at,
            "System.register([], function () {\n  console.log('ok');\n});",
        )
        .unwrap();

        let script = artifact
            .files
            .iter()
            .find(|file| file.path == "charts/frontend/scripts/index.js")
            .unwrap();
        let content = std::str::from_utf8(&script.content).unwrap();

        assert!(content.contains("System.register([], function () {"));
        assert!(content.contains("  console.log('ok');"));

        let template = artifact
            .files
            .iter()
            .find(|file| file.path == "charts/frontend/templates/configmap.yaml")
            .unwrap();
        let content = std::str::from_utf8(&template.content).unwrap();

        assert!(content.contains(r#"{{ (.Files.Glob "scripts/index.js").AsConfig | indent 2 }}"#));
        assert!(!content.contains(".Values.indexJsContent"));
    }

    #[test]
    fn role_templates_are_generated_from_pages_and_ignore_explicit_resources() {
        let generated_at = DateTime::from_timestamp(1_775_200_000, 0).unwrap();
        let artifact =
            build_extension_package(&sample_fe(), generated_at, "console.log('ok');").unwrap();
        let template = artifact
            .files
            .iter()
            .find(|file| file.path == "charts/inspecttask-helper/templates/roleTemplate.yaml")
            .unwrap();
        let content = std::str::from_utf8(&template.content).unwrap();

        assert!(content.contains("name: cluster-view-inspecttask"));
        assert!(content.contains("rules: []"));
        assert!(
            content.contains(r#"iam.kubesphere.io/role-template-rules: '{"inspecttasks":"view"}'"#)
        );
        assert!(!content.contains("inspecttask-view"));
    }

    #[test]
    fn source_hash_changes_with_package_content() {
        let generated_at = DateTime::from_timestamp(1_775_200_000, 0).unwrap();
        let a = sample_fe();
        let mut b = sample_fe();
        b.spec.package.version = "0.2.0".to_string();

        let a = build_extension_package(&a, generated_at, "console.log('a');").unwrap();
        let b = build_extension_package(&b, generated_at, "console.log('a');").unwrap();

        assert_ne!(a.source_hash, b.source_hash);
        assert_ne!(a.digest, b.digest);
    }

    #[test]
    fn source_hash_ignores_deprecated_extension_resources() {
        let mut a = sample_fe();
        let mut b = sample_fe();
        let resources = b.spec.source.inline.extension_resources.as_mut().unwrap();
        resources.js_bundle.as_mut().unwrap().name = "changed-jsbundle".to_string();
        resources.role_templates[0].name = "changed-role-template".to_string();
        resources.role_templates[0].rules[0].verbs = vec!["delete".to_string()];

        assert_eq!(
            frontend_extension_source_hash(&a).unwrap(),
            frontend_extension_source_hash(&b).unwrap()
        );

        a.spec.source.inline.extension_resources = None;
        assert_eq!(
            frontend_extension_source_hash(&a).unwrap(),
            frontend_extension_source_hash(&b).unwrap()
        );
    }

    #[test]
    fn source_hash_ignores_package_dependencies() {
        let a = sample_fe();
        let mut b = sample_fe();
        b.spec.package.dependencies = vec![ExtensionDependencySpec {
            name: "legacy-chart".to_string(),
            tags: vec!["extension".to_string()],
        }];

        assert_eq!(
            frontend_extension_source_hash(&a).unwrap(),
            frontend_extension_source_hash(&b).unwrap()
        );
    }

    #[test]
    fn gzip_payload_is_deterministic() {
        let generated_at = DateTime::from_timestamp(1_775_200_000, 0).unwrap();
        let a = build_extension_package(&sample_fe(), generated_at, "console.log('a');").unwrap();
        let b = build_extension_package(&sample_fe(), generated_at, "console.log('a');").unwrap();

        assert_eq!(a.digest, b.digest);
        assert_eq!(
            a.payload.binary_data[PACKAGE_KEY],
            b.payload.binary_data[PACKAGE_KEY]
        );
    }

    #[test]
    fn crd_table_role_templates_are_aggregated_by_scope_and_action() {
        let generated_at = DateTime::from_timestamp(1_775_200_000, 0).unwrap();
        let fe: FrontendExtension = serde_yaml::from_str(
            r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendExtension
metadata:
  name: mixed
spec:
  package:
    name: mixed
    version: 0.1.0
    displayName:
      en: Mixed
    description:
      en: Mixed
  source:
    type: Inline
    inline:
      schemaVersion: v1
      frontend:
        menus:
          - displayName: Cluster Tasks
            key: cluster-tasks
            placement: cluster
            type: page
          - displayName: Workspace Reports
            key: workspace-reports
            placement: workspace
            type: page
          - displayName: Global Items
            key: global-items
            placement: global
            type: page
        pages:
          - key: cluster-tasks
            type: crdTable
            crdTable:
              group: kubeeye.kubesphere.io
              version: v1alpha2
              scope: Cluster
              authKey: inspecttask-auth
              names:
                kind: InspectTask
                plural: inspecttasks
              columns:
                - key: name
                  title: NAME
                  render:
                    type: text
                    path: metadata.name
          - key: workspace-reports
            type: crdTable
            crdTable:
              group: reports.kubesphere.io
              version: v1alpha1
              scope: Namespaced
              names:
                kind: Report
                plural: reports
              columns:
                - key: name
                  title: NAME
                  render:
                    type: text
                    path: metadata.name
          - key: global-items
            type: crdTable
            crdTable:
              group: items.kubesphere.io
              version: v1
              scope: Namespaced
              names:
                kind: Item
                plural: items
              columns:
                - key: name
                  title: NAME
                  render:
                    type: text
                    path: metadata.name
"#,
        )
        .unwrap();
        let artifact = build_extension_package(&fe, generated_at, "console.log('ok');").unwrap();
        let template = artifact
            .files
            .iter()
            .find(|file| file.path == "charts/mixed-helper/templates/roleTemplate.yaml")
            .unwrap();
        let content = std::str::from_utf8(&template.content).unwrap();

        assert!(content.contains("name: cluster-view-mixed"));
        assert!(content.contains("name: cluster-manage-mixed"));
        assert!(content.contains("name: namespace-view-mixed"));
        assert!(content.contains("name: namespace-manage-mixed"));
        assert!(content.contains(r#""inspecttask-auth":"view""#));
        assert!(content.contains(r#""workspace-reports":"view""#));
        assert!(content.contains(r#""global-items":"manage""#));
        assert!(content.contains("- 'inspecttasks'"));
        assert!(content.contains("- 'reports'"));
        assert!(content.contains("- 'items'"));
        assert!(content.contains("iam.kubesphere.io/dependencies: '[\"cluster-view-mixed\"]'"));
        assert!(content.contains("iam.kubesphere.io/dependencies: '[\"namespace-view-mixed\"]'"));
    }

    #[test]
    fn role_template_scope_resolution_matches_page_rules() {
        assert_eq!(
            crd_role_scope(MenuPlacement::Cluster, CrdScope::Namespaced),
            RoleScope::Cluster
        );
        assert_eq!(
            crd_role_scope(MenuPlacement::Workspace, CrdScope::Cluster),
            RoleScope::Namespace
        );
        assert_eq!(
            crd_role_scope(MenuPlacement::Global, CrdScope::Cluster),
            RoleScope::Cluster
        );
        assert_eq!(
            crd_role_scope(MenuPlacement::Global, CrdScope::Namespaced),
            RoleScope::Namespace
        );
        assert_eq!(
            iframe_role_scope(MenuPlacement::Cluster),
            Some(RoleScope::Cluster)
        );
        assert_eq!(
            iframe_role_scope(MenuPlacement::Workspace),
            Some(RoleScope::Namespace)
        );
        assert_eq!(iframe_role_scope(MenuPlacement::Global), None);
    }

    #[test]
    fn iframe_global_pages_do_not_generate_role_templates() {
        let generated_at = DateTime::from_timestamp(1_775_200_000, 0).unwrap();
        let fe: FrontendExtension = serde_yaml::from_str(
            r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendExtension
metadata:
  name: global-frame
spec:
  package:
    version: 0.1.0
    displayName:
      en: Global Frame
    description:
      en: Global Frame
  source:
    type: Inline
    inline:
      schemaVersion: v1
      frontend:
        menus:
          - displayName: Global Frame
            key: global-frame
            placement: global
            type: page
        pages:
          - key: global-frame
            type: iframe
            iframe:
              src: http://example.test
"#,
        )
        .unwrap();
        let artifact = build_extension_package(&fe, generated_at, "console.log('ok');").unwrap();
        let template = artifact
            .files
            .iter()
            .find(|file| file.path == "charts/global-frame-helper/templates/roleTemplate.yaml")
            .unwrap();
        let content = std::str::from_utf8(&template.content).unwrap();

        assert_eq!(
            content.trim(),
            "{{- if .Values.roleTemplate.enabled }}\n{{- end }}"
        );
    }
}
