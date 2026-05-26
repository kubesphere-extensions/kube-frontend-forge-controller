use std::collections::{HashMap, HashSet};

use super::*;

pub fn resolve_v1_pages(
    input: &FrontendRenderInput,
) -> Result<Vec<ResolvedFrontendPage>, ManifestRenderError> {
    let fi_name = input.name.clone();
    let resolved_menus = resolve_spec(input, &fi_name, &input.route_namespace)?;
    let mut pages = Vec::new();
    let mut rendered_page_keys = HashSet::new();

    for menu in resolved_menus {
        match menu {
            ResolvedTopMenu::Page(page) => {
                push_resolved_frontend_page(*page, &mut pages, &mut rendered_page_keys);
            }
            ResolvedTopMenu::Organization { children, .. } => {
                for child in children {
                    push_resolved_frontend_page(child, &mut pages, &mut rendered_page_keys);
                }
            }
        }
    }

    Ok(pages)
}

fn push_resolved_frontend_page(
    page: ResolvedPageBinding,
    pages: &mut Vec<ResolvedFrontendPage>,
    rendered_page_keys: &mut HashSet<(String, String)>,
) {
    if rendered_page_keys.insert((
        page.placement.as_str().to_string(),
        page.page_id_suffix.clone(),
    )) {
        pages.push(resolved_frontend_page(page));
    }
}

pub(crate) fn resolved_frontend_page(page: ResolvedPageBinding) -> ResolvedFrontendPage {
    ResolvedFrontendPage {
        title: page.page_title,
        placement: page.placement,
        route_suffix: page.route_suffix,
        action_key: page.page.key.clone(),
        page: page.page,
    }
}

#[derive(Clone, Debug)]
pub(crate) enum ResolvedTopMenu {
    Page(Box<ResolvedPageBinding>),
    Organization {
        menu: ResolvedOrganizationMenu,
        children: Vec<ResolvedPageBinding>,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedOrganizationMenu {
    pub(crate) name: String,
    pub(crate) title: String,
    pub(crate) icon: Option<String>,
    pub(crate) placement: MenuPlacement,
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedPageBinding {
    pub(crate) menu_title: String,
    pub(crate) page_title: String,
    pub(crate) icon: Option<String>,
    pub(crate) placement: MenuPlacement,
    pub(crate) route_suffix: String,
    pub(crate) page_id_suffix: String,
    pub(crate) menu_name: String,
    pub(crate) parent: String,
    pub(crate) page: FrontendPageSpec,
}

pub(crate) fn resolve_spec(
    spec: &FrontendRenderInput,
    fi_name: &str,
    route_namespace: &str,
) -> Result<Vec<ResolvedTopMenu>, ManifestRenderError> {
    let referenced_page_keys = referenced_page_keys(spec);
    let pages_by_key = resolve_pages(spec, fi_name, &referenced_page_keys)?;
    let mut top_level_keys = HashSet::new();
    let mut bound_menu_routes = HashSet::new();
    let mut resolved = Vec::new();

    for menu in &spec.menus {
        validate_key(fi_name, &menu.key, true)?;
        if !top_level_keys.insert((menu.placement.as_str().to_string(), menu.key.clone())) {
            return Err(ManifestRenderError::DuplicateTopLevelMenuKey {
                fi_name: fi_name.to_string(),
                key: menu.key.clone(),
            });
        }

        match menu.type_ {
            MenuNodeType::Page => {
                let top_menu_name = menu_name_for_suffix(route_namespace, fi_name, &menu.key);
                if !menu.children.is_empty() {
                    return Err(ManifestRenderError::InvalidMenuShape {
                        fi_name: fi_name.to_string(),
                        key: menu.key.clone(),
                        message: "page menus cannot define children".to_string(),
                    });
                }

                let page_key = resolve_page_key(
                    fi_name,
                    &menu.key,
                    menu.page_key.as_deref(),
                    spec.require_page_key,
                )?;
                let route_suffix = route_suffix_for_menu(&menu.key);
                let page = bind_page(
                    fi_name,
                    menu.placement,
                    &page_key,
                    &route_suffix,
                    &pages_by_key,
                    &mut bound_menu_routes,
                )?;
                let page_title = page_title(&page, &menu.display_name);
                resolved.push(ResolvedTopMenu::Page(Box::new(ResolvedPageBinding {
                    menu_title: menu.display_name.clone(),
                    page_title,
                    icon: menu.icon.clone(),
                    placement: menu.placement,
                    page_id_suffix: page_id_suffix(&route_suffix, &page_key, spec.require_page_key),
                    route_suffix,
                    menu_name: top_menu_name,
                    parent: menu.placement.as_str().to_string(),
                    page,
                })));
            }
            MenuNodeType::Organization => {
                let top_menu_name = menu_name_for_suffix(route_namespace, fi_name, &menu.key);
                if menu.children.is_empty() {
                    return Err(ManifestRenderError::InvalidMenuShape {
                        fi_name: fi_name.to_string(),
                        key: menu.key.clone(),
                        message: "organization menus must define at least one child".to_string(),
                    });
                }
                if menu.page_key.is_some() {
                    return Err(ManifestRenderError::InvalidMenuShape {
                        fi_name: fi_name.to_string(),
                        key: menu.key.clone(),
                        message: "organization menus cannot define pageKey".to_string(),
                    });
                }

                let mut children = Vec::new();
                for child in &menu.children {
                    validate_key(fi_name, &child.key, true)?;
                    let page_key = resolve_page_key(
                        fi_name,
                        &child.key,
                        child.page_key.as_deref(),
                        spec.require_page_key,
                    )?;
                    let route_suffix = route_suffix_for_child(&menu.key, &child.key);
                    let page = bind_page(
                        fi_name,
                        menu.placement,
                        &page_key,
                        &route_suffix,
                        &pages_by_key,
                        &mut bound_menu_routes,
                    )?;
                    let page_title = page_title(&page, &child.display_name);
                    children.push(ResolvedPageBinding {
                        menu_title: child.display_name.clone(),
                        page_title,
                        icon: child.icon.clone(),
                        placement: menu.placement,
                        page_id_suffix: page_id_suffix(
                            &route_suffix,
                            &page_key,
                            spec.require_page_key,
                        ),
                        route_suffix: route_suffix.clone(),
                        menu_name: menu_name_for_suffix(route_namespace, fi_name, &route_suffix),
                        parent: nested_menu_parent(menu.placement, &top_menu_name),
                        page,
                    });
                }

                resolved.push(ResolvedTopMenu::Organization {
                    menu: ResolvedOrganizationMenu {
                        name: top_menu_name,
                        title: menu.display_name.clone(),
                        icon: menu.icon.clone(),
                        placement: menu.placement,
                    },
                    children,
                });
            }
        }
    }

    Ok(resolved)
}

fn page_title(page: &FrontendPageSpec, menu_display_name: &str) -> String {
    page.display_name
        .as_deref()
        .unwrap_or(menu_display_name)
        .to_string()
}

pub(crate) fn resolve_pages(
    spec: &FrontendRenderInput,
    fi_name: &str,
    referenced_page_keys: &HashSet<&str>,
) -> Result<HashMap<String, FrontendPageSpec>, ManifestRenderError> {
    let mut pages = HashMap::new();

    for page in &spec.pages {
        if !referenced_page_keys.contains(page.key.as_str()) {
            continue;
        }

        validate_key(fi_name, &page.key, false)?;
        validate_page_shape(fi_name, page)?;
        if pages.insert(page.key.clone(), page.clone()).is_some() {
            return Err(ManifestRenderError::DuplicatePageKey {
                fi_name: fi_name.to_string(),
                key: page.key.clone(),
            });
        }
    }

    Ok(pages)
}

fn referenced_page_keys(spec: &FrontendRenderInput) -> HashSet<&str> {
    let mut keys = HashSet::new();

    for menu in &spec.menus {
        match menu.type_ {
            MenuNodeType::Page => {
                if let Some(key) =
                    referenced_page_key(menu.page_key.as_deref(), &menu.key, spec.require_page_key)
                {
                    keys.insert(key);
                }
            }
            MenuNodeType::Organization => {
                for child in &menu.children {
                    if let Some(key) = referenced_page_key(
                        child.page_key.as_deref(),
                        &child.key,
                        spec.require_page_key,
                    ) {
                        keys.insert(key);
                    }
                }
            }
        }
    }

    keys
}

fn referenced_page_key<'a>(
    page_key: Option<&'a str>,
    menu_key: &'a str,
    require_page_key: bool,
) -> Option<&'a str> {
    page_key.or_else(|| (!require_page_key).then_some(menu_key))
}

pub(crate) fn bind_page(
    fi_name: &str,
    placement: MenuPlacement,
    key: &str,
    route_suffix: &str,
    pages_by_key: &HashMap<String, FrontendPageSpec>,
    bound_menu_routes: &mut HashSet<(String, String)>,
) -> Result<FrontendPageSpec, ManifestRenderError> {
    if !bound_menu_routes.insert((placement.as_str().to_string(), route_suffix.to_string())) {
        return Err(ManifestRenderError::DuplicateMenuRoute {
            fi_name: fi_name.to_string(),
            placement: placement.as_str().to_string(),
            route: route_suffix.to_string(),
        });
    }
    let Some(page) = pages_by_key.get(key) else {
        return missing_page(fi_name, key);
    };

    if !page_accepts_placement(page, placement) {
        return Err(ManifestRenderError::InvalidPageShape {
            fi_name: fi_name.to_string(),
            key: key.to_string(),
            message: format!(
                "page placements must include bound menu placement '{}'",
                placement.as_str()
            ),
        });
    }

    Ok(page.clone())
}

fn missing_page(fi_name: &str, key: &str) -> Result<FrontendPageSpec, ManifestRenderError> {
    Err(ManifestRenderError::MissingPageForMenuKey {
        fi_name: fi_name.to_string(),
        key: key.to_string(),
    })
}

pub(crate) fn resolve_page_key(
    fi_name: &str,
    menu_key: &str,
    explicit_page_key: Option<&str>,
    require_page_key: bool,
) -> Result<String, ManifestRenderError> {
    match explicit_page_key {
        Some(page_key) => {
            validate_key(fi_name, page_key, false)?;
            Ok(page_key.to_string())
        }
        None if !require_page_key => Ok(menu_key.to_string()),
        None => Err(ManifestRenderError::InvalidMenuShape {
            fi_name: fi_name.to_string(),
            key: menu_key.to_string(),
            message: "page menus must define pageKey".to_string(),
        }),
    }
}

pub(crate) fn page_accepts_placement(page: &FrontendPageSpec, placement: MenuPlacement) -> bool {
    page.placements
        .as_ref()
        .is_none_or(|placements| placements.contains(&placement))
}

pub(crate) fn validate_key(
    fi_name: &str,
    key: &str,
    is_menu_key: bool,
) -> Result<(), ManifestRenderError> {
    let is_valid = !key.is_empty()
        && !key.starts_with('-')
        && !key.ends_with('-')
        && key
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-');

    if is_valid {
        Ok(())
    } else if is_menu_key {
        Err(ManifestRenderError::InvalidMenuKey {
            fi_name: fi_name.to_string(),
            key: key.to_string(),
        })
    } else {
        Err(ManifestRenderError::InvalidPageShape {
            fi_name: fi_name.to_string(),
            key: key.to_string(),
            message: "page keys must be kebab-case route fragments".to_string(),
        })
    }
}

pub(crate) fn validate_page_shape(
    fi_name: &str,
    page: &FrontendPageSpec,
) -> Result<(), ManifestRenderError> {
    if matches!(page.placements.as_ref(), Some(placements) if placements.is_empty()) {
        return Err(ManifestRenderError::InvalidPageShape {
            fi_name: fi_name.to_string(),
            key: page.key.clone(),
            message: "pages must define at least one placement".to_string(),
        });
    }

    match page.type_ {
        PageType::Iframe => {
            if page.iframe.is_none() {
                return Err(ManifestRenderError::InvalidPageShape {
                    fi_name: fi_name.to_string(),
                    key: page.key.clone(),
                    message: "type=iframe requires iframe config".to_string(),
                });
            }
            if page.crd_table.is_some() {
                return Err(ManifestRenderError::InvalidPageShape {
                    fi_name: fi_name.to_string(),
                    key: page.key.clone(),
                    message: "type=iframe cannot define crdTable config".to_string(),
                });
            }
        }
        PageType::CrdTable => {
            let Some(crd_table) = page.crd_table.as_ref() else {
                return Err(ManifestRenderError::InvalidPageShape {
                    fi_name: fi_name.to_string(),
                    key: page.key.clone(),
                    message: "type=crdTable requires crdTable config".to_string(),
                });
            };
            if page.iframe.is_some() {
                return Err(ManifestRenderError::InvalidPageShape {
                    fi_name: fi_name.to_string(),
                    key: page.key.clone(),
                    message: "type=crdTable cannot define iframe config".to_string(),
                });
            }
            if crd_table.columns.is_empty() {
                return Err(ManifestRenderError::MissingCrdColumns {
                    fi_name: fi_name.to_string(),
                    key: page.key.clone(),
                });
            }
        }
    }

    Ok(())
}

pub(crate) fn route_suffix_for_menu(key: &str) -> String {
    key.to_string()
}

pub(crate) fn route_suffix_for_child(parent_key: &str, child_key: &str) -> String {
    format!("{parent_key}/{child_key}")
}

pub(crate) fn page_id_suffix(route_suffix: &str, page_key: &str, use_page_key: bool) -> String {
    if use_page_key {
        page_key.to_string()
    } else {
        route_suffix.to_string()
    }
}
