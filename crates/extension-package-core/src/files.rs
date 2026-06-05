use super::*;

pub(crate) fn frontend_script_file(index_js_content: &str) -> PackageFile {
    PackageFile {
        path: "charts/frontend/scripts/index.js".to_string(),
        content: index_js_content.as_bytes().to_vec(),
    }
}

pub(crate) fn readme_file(
    package_name: &str,
    fe_cr: &Value,
    pages: &[ResolvedFrontendPage],
) -> Result<PackageFile, ExtensionPackageError> {
    readme_file_with_template(
        "README.md.tpl",
        "README.md",
        package_name,
        fe_cr,
        pages,
        Locale::En,
    )
}

pub(crate) fn readme_zh_file(
    package_name: &str,
    fe_cr: &Value,
    pages: &[ResolvedFrontendPage],
) -> Result<PackageFile, ExtensionPackageError> {
    readme_file_with_template(
        "README_zh.md.tpl",
        "README_zh.md",
        package_name,
        fe_cr,
        pages,
        Locale::Zh,
    )
}

#[derive(Clone, Copy)]
pub(crate) enum Locale {
    En,
    Zh,
}

#[derive(Serialize)]
struct ReadmeTemplate<'a> {
    package_name: &'a str,
    fe_cr: &'a Value,
    integrations: Vec<ReadmeIntegration>,
}

#[derive(Clone, Debug, Serialize)]
struct ReadmeIntegration {
    key: String,
    title: String,
    kind: &'static str,
    kind_label: &'static str,
    placement_phrase: String,
    crd_resource: Option<ReadmeCrdResource>,
    iframe_src: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct ReadmeCrdResource {
    plural: String,
    group: String,
    version: String,
}

#[derive(Clone, Debug)]
struct ReadmeIntegrationSource {
    key: String,
    title: String,
    type_: PageType,
    placements: Vec<MenuPlacement>,
    crd_resource: Option<ReadmeCrdResource>,
    iframe_src: Option<String>,
}

fn readme_file_with_template(
    template_path: &'static str,
    output_path: &str,
    package_name: &str,
    fe_cr: &Value,
    pages: &[ResolvedFrontendPage],
    locale: Locale,
) -> Result<PackageFile, ExtensionPackageError> {
    let content = render_readme_template(template_path, package_name, fe_cr, pages, locale)?;
    Ok(text_file(output_path, &content))
}

pub(crate) fn render_readme_template(
    template_path: &'static str,
    package_name: &str,
    fe_cr: &Value,
    pages: &[ResolvedFrontendPage],
    locale: Locale,
) -> Result<String, ExtensionPackageError> {
    let source = template_text(template_path)?;
    let mut env = minijinja::Environment::new();
    env.add_template(template_path, source)
        .with_context(|_| RenderTemplateSnafu {
            path: template_path,
        })?;
    let template = env
        .get_template(template_path)
        .with_context(|_| RenderTemplateSnafu {
            path: template_path,
        })?;
    let model = ReadmeTemplate {
        package_name,
        fe_cr,
        integrations: readme_integrations(pages, locale),
    };

    template
        .render(model)
        .with_context(|_| RenderTemplateSnafu {
            path: template_path,
        })
}

fn readme_integrations(pages: &[ResolvedFrontendPage], locale: Locale) -> Vec<ReadmeIntegration> {
    readme_integration_sources(pages)
        .into_iter()
        .map(|source| ReadmeIntegration {
            key: source.key,
            title: source.title,
            kind: readme_integration_kind(source.type_),
            kind_label: readme_integration_kind_label(source.type_, locale),
            placement_phrase: placement_phrase(&source.placements, locale),
            crd_resource: source.crd_resource,
            iframe_src: source.iframe_src,
        })
        .collect()
}

fn readme_integration_sources(pages: &[ResolvedFrontendPage]) -> Vec<ReadmeIntegrationSource> {
    let mut integrations: Vec<ReadmeIntegrationSource> = Vec::new();

    for page in pages {
        if let Some(integration) = integrations
            .iter_mut()
            .find(|item| item.key == page.page.key && item.type_ == page.page.type_)
        {
            push_unique_placement(&mut integration.placements, page.placement);
            continue;
        }

        integrations.push(ReadmeIntegrationSource {
            key: page.page.key.clone(),
            title: page.title.clone(),
            type_: page.page.type_,
            placements: vec![page.placement],
            crd_resource: page.page.crd_table.as_ref().map(ReadmeCrdResource::from),
            iframe_src: page.page.iframe.as_ref().map(|iframe| iframe.src.clone()),
        });
    }

    integrations
}

fn readme_integration_kind(type_: PageType) -> &'static str {
    match type_ {
        PageType::CrdTable => "crdTable",
        PageType::Iframe => "iframe",
    }
}

impl From<&CrdTablePageSpec> for ReadmeCrdResource {
    fn from(crd: &CrdTablePageSpec) -> Self {
        Self {
            plural: crd.names.plural.clone(),
            group: crd.group.clone(),
            version: crd.version.clone(),
        }
    }
}

fn readme_integration_kind_label(type_: PageType, locale: Locale) -> &'static str {
    match (type_, locale) {
        (PageType::CrdTable, Locale::En) => "Resource Integration",
        (PageType::CrdTable, Locale::Zh) => "资源集成",
        (PageType::Iframe, Locale::En) => "Page Integration",
        (PageType::Iframe, Locale::Zh) => "页面集成",
    }
}

fn push_unique_placement(placements: &mut Vec<MenuPlacement>, placement: MenuPlacement) {
    if !placements.contains(&placement) {
        placements.push(placement);
    }
}

fn ordered_placements(placements: &[MenuPlacement]) -> Vec<MenuPlacement> {
    [
        MenuPlacement::Cluster,
        MenuPlacement::Workspace,
        MenuPlacement::Global,
    ]
    .into_iter()
    .filter(|placement| placements.contains(placement))
    .collect()
}

fn placement_phrase(placements: &[MenuPlacement], locale: Locale) -> String {
    let phrases = ordered_placements(placements)
        .into_iter()
        .map(|placement| match locale {
            Locale::En => match placement {
                MenuPlacement::Cluster => "**Cluster**",
                MenuPlacement::Workspace => "**Workspace**",
                MenuPlacement::Global => "**Global**",
            },
            Locale::Zh => match placement {
                MenuPlacement::Cluster => "**集群（Cluster）**",
                MenuPlacement::Workspace => "**企业空间（Workspace）**",
                MenuPlacement::Global => "**全局（Global）**",
            },
        })
        .collect();

    match locale {
        Locale::En => join_phrases(phrases, ", ", " and "),
        Locale::Zh => join_phrases(phrases, "、", " 和 "),
    }
}

fn join_phrases(phrases: Vec<&'static str>, separator: &str, conjunction: &str) -> String {
    match phrases.as_slice() {
        [] => String::new(),
        [only] => (*only).to_string(),
        [head @ .., tail] => format!("{}{conjunction}{tail}", head.join(separator)),
    }
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
