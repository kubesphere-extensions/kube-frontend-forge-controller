use std::collections::HashSet;

use super::*;

pub fn render_v1_manifest(input: &FrontendRenderInput) -> Result<Value, ManifestRenderError> {
    let fi_name = input.name.clone();
    let display_name = input
        .display_name
        .clone()
        .unwrap_or_else(|| fi_name.clone());
    let resolved_menus = resolve_spec(input, &fi_name, &input.route_namespace)?;

    let mut routes = Vec::new();
    let mut menus = Vec::new();
    let mut pages = Vec::new();
    let mut rendered_page_ids = HashSet::new();

    for menu in resolved_menus {
        match menu {
            ResolvedTopMenu::Page(page) => {
                menus.push(render_leaf_menu(&page));
                routes.push(render_route(&input.route_namespace, &fi_name, &page));
                push_rendered_page(&fi_name, &page, &mut pages, &mut rendered_page_ids)?;
            }
            ResolvedTopMenu::Organization { menu, children } => {
                menus.push(render_organization_menu(&menu));
                for child in children {
                    menus.push(render_leaf_menu(&child));
                    routes.push(render_route(&input.route_namespace, &fi_name, &child));
                    push_rendered_page(&fi_name, &child, &mut pages, &mut rendered_page_ids)?;
                }
            }
        }
    }

    let mut manifest = Map::new();
    manifest.insert("version".to_string(), json!("1.0"));
    manifest.insert("name".to_string(), json!(fi_name));
    manifest.insert("displayName".to_string(), json!(display_name));
    if let Some(description) = input.description.as_ref() {
        manifest.insert("description".to_string(), json!(description));
    }
    manifest.insert("routes".to_string(), Value::Array(routes));
    manifest.insert("menus".to_string(), Value::Array(menus));
    manifest.insert("locales".to_string(), render_locales(input));
    manifest.insert("pages".to_string(), Value::Array(pages));
    manifest.insert(
        "build".to_string(),
        json!({
            "target": "kubesphere-extension",
            "moduleName": input.name,
            "systemjs": true,
        }),
    );

    Ok(Value::Object(manifest))
}

fn push_rendered_page(
    fi_name: &str,
    page: &ResolvedPageBinding,
    pages: &mut Vec<Value>,
    rendered_page_ids: &mut HashSet<String>,
) -> Result<(), ManifestRenderError> {
    let page_id = page_id_for_suffix(fi_name, page.placement, &page.page_id_suffix);
    if rendered_page_ids.insert(page_id) {
        pages.push(render_page(fi_name, page)?);
    }
    Ok(())
}

pub(crate) fn menu_name_for_suffix(route_namespace: &str, fi_name: &str, suffix: &str) -> String {
    format!("{route_namespace}/{fi_name}/{suffix}")
}

pub(crate) fn nested_menu_parent(placement: MenuPlacement, menu_name: &str) -> String {
    format!("{}.{}", placement.as_str(), menu_name)
}

pub(crate) fn page_id_for_suffix(fi_name: &str, placement: MenuPlacement, suffix: &str) -> String {
    format!(
        "{}-{}-{}",
        fi_name,
        placement.as_str(),
        suffix.replace('/', "_")
    )
}

pub(crate) fn render_route(
    route_namespace: &str,
    fi_name: &str,
    page: &ResolvedPageBinding,
) -> Value {
    let page_id = page_id_for_suffix(fi_name, page.placement, &page.page_id_suffix);
    json!({
        "path": format!(
            "{}{}",
            page.placement.route_prefix(),
            route_tail(route_namespace, fi_name, &page.route_suffix)
        ),
        "pageId": page_id,
    })
}

pub(crate) fn render_leaf_menu(page: &ResolvedPageBinding) -> Value {
    json!({
        "parent": page.parent,
        "name": page.menu_name,
        "title": page.title,
        "icon": menu_icon(page.icon.as_ref()),
        "order": 999,
    })
}

pub(crate) fn render_organization_menu(menu: &ResolvedOrganizationMenu) -> Value {
    json!({
        "parent": menu.placement.as_str(),
        "name": menu.name,
        "title": menu.title,
        "icon": menu_icon(menu.icon.as_ref()),
        "order": 999,
    })
}

pub(crate) fn menu_icon(icon: Option<&String>) -> &str {
    icon.map_or(DEFAULT_MENU_ICON, String::as_str)
}

pub(crate) fn route_tail(route_namespace: &str, fi_name: &str, suffix: &str) -> String {
    format!("/{route_namespace}/{fi_name}/{suffix}")
}

pub(crate) fn render_page(
    fi_name: &str,
    page: &ResolvedPageBinding,
) -> Result<Value, ManifestRenderError> {
    let page_id = page_id_for_suffix(fi_name, page.placement, &page.page_id_suffix);

    match page.page.type_ {
        PageType::Iframe => {
            let iframe =
                page.page
                    .iframe
                    .as_ref()
                    .ok_or_else(|| ManifestRenderError::InvalidPageShape {
                        fi_name: fi_name.to_string(),
                        key: page.page.key.clone(),
                        message: "type=iframe requires iframe config".to_string(),
                    })?;
            Ok(iframe_page(&page_id, &page.title, &iframe.src))
        }
        PageType::CrdTable => {
            let crd_table = page.page.crd_table.as_ref().ok_or_else(|| {
                ManifestRenderError::InvalidPageShape {
                    fi_name: fi_name.to_string(),
                    key: page.page.key.clone(),
                    message: "type=crdTable requires crdTable config".to_string(),
                }
            })?;
            Ok(crd_page(
                &page_id,
                &page.title,
                page.placement,
                crd_table,
                &crd_table.columns,
            ))
        }
    }
}
pub(crate) fn render_locales(spec: &FrontendRenderInput) -> Value {
    let locales = spec
        .locales
        .iter()
        .map(|(lang, messages)| {
            json!({
                "lang": lang,
                "messages": messages,
            })
        })
        .collect::<Vec<Value>>();
    Value::Array(locales)
}
