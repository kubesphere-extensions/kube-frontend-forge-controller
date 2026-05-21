use super::*;

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
    #[serde(rename = "staticFileDirectory")]
    static_file_directory: &'a Option<String>,
    dependencies: &'a Vec<ExtensionDependencySpec>,
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
    let dependencies = package_dependencies(&package_name, package.dependencies.as_ref());
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
            static_file_directory: &package.static_file_directory,
            dependencies: &dependencies,
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
