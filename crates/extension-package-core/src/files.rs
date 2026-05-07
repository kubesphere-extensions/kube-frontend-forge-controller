use super::*;

pub(crate) fn frontend_script_file(index_js_content: &str) -> PackageFile {
    PackageFile {
        path: "charts/frontend/scripts/index.js".to_string(),
        content: index_js_content.as_bytes().to_vec(),
    }
}

pub(crate) fn readme_file(package_name: &str) -> PackageFile {
    text_file(
        "README.md",
        &format!(
            "This is a {package_name} extension, which is shown in more detail here, and you can \
             write it here using Markdown syntax.\n"
        ),
    )
}

pub(crate) fn readme_zh_file(package_name: &str) -> PackageFile {
    text_file(
        "README_zh.md",
        &format!(
            "这是一个{package_name}扩展组件，这里展示了他的详细介绍，你可以在这里使用 Markdown \
             语法编写内容。\n"
        ),
    )
}

pub(crate) fn template_text_file(
    source_path: &'static str,
    output_path: impl Into<String>,
) -> Result<PackageFile, ExtensionPackageError> {
    Ok(PackageFile {
        path: output_path.into(),
        content: template_text(source_path)?.as_bytes().to_vec(),
    })
}

pub(crate) fn template_binary_file(
    source_path: &'static str,
    output_path: impl Into<String>,
) -> Result<PackageFile, ExtensionPackageError> {
    Ok(PackageFile {
        path: output_path.into(),
        content: template_bytes(source_path)?.to_vec(),
    })
}
