use super::*;

#[derive(Serialize)]
pub(crate) struct KsbuilderExtensionYaml {
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
    #[serde(
        skip_serializing_if = "Option::is_none",
        rename = "staticFileDirectory"
    )]
    static_file_directory: Option<String>,
    dependencies: Vec<ExtensionDependencySpec>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "installationMode")]
    installation_mode: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    images: Vec<String>,
}

pub(crate) fn package_metadata(fe: &FrontendExtension) -> KsbuilderExtensionYaml {
    let package = &fe.spec.package;
    let package_name = frontend_extension_package_name(fe);
    let dependencies = package_dependencies(&package_name, package.dependencies.as_ref());
    KsbuilderExtensionYaml {
        api_version: "kubesphere.io/v1alpha1".to_string(),
        name: package_name,
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
        static_file_directory: package.static_file_directory.clone(),
        dependencies,
        installation_mode: package.installation_mode.clone(),
        images: package.images.clone(),
    }
}

pub(crate) fn package_dependencies(
    package_name: &str,
    dependencies: Option<&Vec<ExtensionDependencySpec>>,
) -> Vec<ExtensionDependencySpec> {
    dependencies
        .cloned()
        .unwrap_or_else(|| default_package_dependencies(package_name))
}

pub(crate) fn default_package_dependencies(package_name: &str) -> Vec<ExtensionDependencySpec> {
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
pub(crate) struct HelmChartYaml {
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

pub(crate) fn frontend_chart(fe: &FrontendExtension, package_name: &str) -> HelmChartYaml {
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

pub(crate) fn helper_chart(fe: &FrontendExtension, helper_chart_name: &str) -> HelmChartYaml {
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

pub(crate) fn root_values(
    fe: &FrontendExtension,
    helper_chart_name: &str,
) -> BTreeMap<String, Value> {
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

pub(crate) fn helper_chart_name(package_name: &str) -> String {
    format!("{package_name}-helper")
}
