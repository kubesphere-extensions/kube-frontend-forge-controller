use super::*;

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

pub(crate) fn package_files(
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
        readme_file(&package_name),
        readme_zh_file(&package_name),
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
