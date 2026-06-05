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
    extension_display_name: String,
    fe_cr: &'a Value,
    top_menu_phrase: String,
    all_menu_entry_phrase: String,
    integrations: Vec<ReadmeIntegration>,
}

#[derive(Clone, Debug, Serialize)]
struct ReadmeIntegration {
    key: String,
    title: String,
    kind: &'static str,
    kind_label: &'static str,
    placement_phrase: String,
    menu_entry_phrase: String,
    quick_start_text: String,
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
        extension_display_name: extension_display_name(fe_cr, locale)
            .unwrap_or_else(|| package_name.to_string()),
        fe_cr,
        top_menu_phrase: top_menu_phrase(fe_cr, locale),
        all_menu_entry_phrase: all_menu_entry_phrase(pages, locale),
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
        .map(|source| {
            let quick_start_text = quick_start_text(&source, locale);
            ReadmeIntegration {
                key: source.key,
                title: source.title,
                kind: readme_integration_kind(source.type_),
                kind_label: readme_integration_kind_label(source.type_, locale),
                placement_phrase: placement_phrase(&source.placements, locale),
                menu_entry_phrase: menu_entry_phrase(&source.placements, locale),
                quick_start_text,
                crd_resource: source.crd_resource,
                iframe_src: source.iframe_src,
            }
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

fn quick_start_text(source: &ReadmeIntegrationSource, locale: Locale) -> String {
    match (source.type_, locale) {
        (PageType::CrdTable, Locale::En) => {
            let resource = source
                .crd_resource
                .as_ref()
                .map(|resource| resource.plural.as_str())
                .unwrap_or("the custom resource");
            format!("Open \"{}\" to view `{resource}` resources.", source.title)
        }
        (PageType::CrdTable, Locale::Zh) => {
            let resource = source
                .crd_resource
                .as_ref()
                .map(|resource| resource.plural.as_str())
                .unwrap_or("自定义");
            format!("进入「{}」，查看 `{resource}` 资源。", source.title)
        }
        (PageType::Iframe, Locale::En) => {
            format!(
                "Open \"{}\" to access the embedded third-party page.",
                source.title
            )
        }
        (PageType::Iframe, Locale::Zh) => {
            format!("进入「{}」，访问嵌入的第三方页面。", source.title)
        }
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

pub(crate) fn placement_phrase(placements: &[MenuPlacement], locale: Locale) -> String {
    let phrases: Vec<&'static str> = ordered_placements(placements)
        .into_iter()
        .map(|placement| formatted_placement_label(placement, locale))
        .collect();

    match locale {
        Locale::En if phrases.is_empty() => "**Unknown**".to_string(),
        Locale::Zh if phrases.is_empty() => "**未知**".to_string(),
        Locale::En => join_phrases(phrases, ", ", " and "),
        Locale::Zh => join_phrases(phrases, "、", " 和 "),
    }
}

fn menu_entry_phrase(placements: &[MenuPlacement], locale: Locale) -> String {
    let phrases: Vec<&'static str> = ordered_placements(placements)
        .into_iter()
        .map(|placement| plain_placement_label(placement, locale))
        .collect();

    match locale {
        Locale::En if phrases.is_empty() => "Unknown".to_string(),
        Locale::Zh if phrases.is_empty() => "未知".to_string(),
        Locale::En => join_phrases(phrases, ", ", " and "),
        Locale::Zh => join_phrases(phrases, "、", "、"),
    }
}

fn all_menu_entry_phrase(pages: &[ResolvedFrontendPage], locale: Locale) -> String {
    let mut placements = Vec::new();
    for page in pages {
        push_unique_placement(&mut placements, page.placement);
    }
    menu_entry_phrase(&placements, locale)
}

fn top_menu_phrase(fe_cr: &Value, locale: Locale) -> String {
    let Some(menus) = fe_cr
        .pointer("/spec/source/inline/frontend/menus")
        .and_then(Value::as_array)
    else {
        return match locale {
            Locale::En => "Unknown".to_string(),
            Locale::Zh => "未知".to_string(),
        };
    };
    let mut titles = Vec::new();
    for menu in menus {
        if let Some(title) = menu.get("displayName").and_then(Value::as_str)
            && !titles.contains(&title)
        {
            titles.push(title);
        }
    }

    match locale {
        Locale::En => quote_phrase(titles, "\"", "\"", ", ", " and "),
        Locale::Zh => quote_phrase(titles, "「", "」", "、", "、"),
    }
}

fn extension_display_name(fe_cr: &Value, locale: Locale) -> Option<String> {
    localized_map_text(fe_cr.pointer("/spec/package/displayName"), locale)
        .or_else(|| {
            fe_cr
                .pointer("/spec/package/name")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .or_else(|| {
            fe_cr
                .pointer("/metadata/name")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}

fn localized_map_text(value: Option<&Value>, locale: Locale) -> Option<String> {
    let value = value?;
    let (preferred, fallback) = match locale {
        Locale::En => ("en", "zh"),
        Locale::Zh => ("zh", "en"),
    };
    value
        .get(preferred)
        .or_else(|| value.get(fallback))
        .or_else(|| value.as_object().and_then(|values| values.values().next()))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn formatted_placement_label(placement: MenuPlacement, locale: Locale) -> &'static str {
    match locale {
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
    }
}

fn plain_placement_label(placement: MenuPlacement, locale: Locale) -> &'static str {
    match locale {
        Locale::En => match placement {
            MenuPlacement::Cluster => "Cluster",
            MenuPlacement::Workspace => "Workspace",
            MenuPlacement::Global => "Global",
        },
        Locale::Zh => match placement {
            MenuPlacement::Cluster => "集群",
            MenuPlacement::Workspace => "企业空间",
            MenuPlacement::Global => "全局",
        },
    }
}

fn join_phrases(phrases: Vec<&'static str>, separator: &str, conjunction: &str) -> String {
    match phrases.as_slice() {
        [] => String::new(),
        [only] => (*only).to_string(),
        [head @ .., tail] => format!("{}{conjunction}{tail}", head.join(separator)),
    }
}

fn quote_phrase(
    phrases: Vec<&str>,
    open_quote: &str,
    close_quote: &str,
    separator: &str,
    conjunction: &str,
) -> String {
    if phrases.is_empty() {
        return String::new();
    }
    let quoted = phrases
        .into_iter()
        .map(|phrase| format!("{open_quote}{phrase}{close_quote}"))
        .collect::<Vec<_>>();
    match quoted.as_slice() {
        [only] => only.clone(),
        [head @ .., tail] => format!("{}{conjunction}{tail}", head.join(separator)),
        [] => String::new(),
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
