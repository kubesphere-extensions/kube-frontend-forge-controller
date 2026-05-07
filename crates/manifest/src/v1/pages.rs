use super::*;

pub(crate) fn page_meta(page_id: &str, title: &str) -> Value {
    json!({
      "id": page_id,
      "name": page_id,
      "title": title,
      "path": format!("/{}", page_id),
    })
}

pub(crate) fn iframe_page(page_id: &str, display_name: &str, frame_src: &str) -> Value {
    json!({
      "id": page_id,
      "entryComponent": page_id,
      "componentsTree": {
        "meta": page_meta(page_id, display_name),
        "context": {},
        "root": {
          "id": format!("{}-root", page_id),
          "type": "Iframe",
          "props": {
            "FRAME_URL": frame_src,
          },
          "meta": { "title": "Iframe", "scope": true }
        }
      }
    })
}

pub(crate) fn crd_page(
    page_id: &str,
    display_name: &str,
    placement: MenuPlacement,
    crd: &CrdTablePageSpec,
    columns: &[ColumnSpec],
) -> Value {
    let columns_config = transform_columns(columns);
    let page_state_type = crd_page_state_type(placement);
    let page_state_config = crd_page_state_config(page_id, placement, crd);
    let auth_key = crd.auth_key.as_deref().unwrap_or("");

    json!({
      "id": page_id,
      "entryComponent": page_id,
      "componentsTree": {
        "meta": page_meta(page_id, display_name),
        "context": {},
        "dataSources": [
          {
            "id": "columns",
            "type": "crd-columns",
            "config": {
              "COLUMNS_CONFIG": columns_config,
              "HOOK_NAME": "useCrdColumns"
            }
          },
          {
            "id": "pageState",
            "type": page_state_type,
            "args": [
              { "type": "binding", "source": "columns", "bind": "columns" }
            ],
            "config": page_state_config
          }
        ],
        "root": {
          "id": format!("{}-root", page_id),
          "type": "CrdTable",
          "props": {
            "TABLE_KEY": page_id,
            "TITLE": display_name,
            "PARAMS": { "type": "binding", "source": "pageState", "bind": "params" },
            "REFETCH": { "type": "binding", "source": "pageState", "bind": "refetch" },
            "TOOLBAR_LEFT": { "type": "binding", "source": "pageState", "bind": "toolbarLeft" },
            "PAGE_CONTEXT": { "type": "binding", "source": "pageState", "bind": "pageContext" },
            "COLUMNS": { "type": "binding", "source": "columns", "bind": "columns" },
            "DATA": { "type": "binding", "source": "pageState", "bind": "data" },
            "IS_LOADING": {
              "type": "binding",
              "source": "pageState",
              "bind": "loading",
              "defaultValue": false
            },
            "UPDATE": { "type": "binding", "source": "pageState", "bind": "update" },
            "DEL": { "type": "binding", "source": "pageState", "bind": "del" },
            "CREATE": { "type": "binding", "source": "pageState", "bind": "create" },
            "CREATE_INITIAL_VALUE": crd_create_initial_value(crd),
            "AUTH_KEY": auth_key
          },
          "meta": { "title": "CrdTable", "scope": true }
        }
      }
    })
}

pub(crate) fn crd_create_initial_value(crd: &CrdTablePageSpec) -> Value {
    let mut initial = Map::new();
    let mut metadata = Map::new();

    initial.insert(
        "apiVersion".to_string(),
        json!(format!("{}/{}", crd.group, crd.version)),
    );
    if let Some(kind) = crd.names.kind.as_ref() {
        initial.insert("kind".to_string(), json!(kind));
    }
    metadata.insert("name".to_string(), json!(""));
    metadata.insert("labels".to_string(), json!({}));
    metadata.insert("annotations".to_string(), json!({}));
    if crd.scope == CrdScope::Namespaced {
        metadata.insert("namespace".to_string(), json!(""));
    }
    initial.insert("metadata".to_string(), Value::Object(metadata));
    initial.insert("spec".to_string(), json!({}));
    Value::Object(initial)
}

pub(crate) const fn crd_page_state_type(placement: MenuPlacement) -> &'static str {
    match placement {
        MenuPlacement::Workspace => "workspace-crd-page-state",
        _ => "crd-page-state",
    }
}

pub(crate) fn crd_page_state_config(
    page_id: &str,
    placement: MenuPlacement,
    crd: &CrdTablePageSpec,
) -> Value {
    let mut config = Map::new();
    config.insert("PAGE_ID".to_string(), json!(page_id));
    config.insert("CRD_CONFIG".to_string(), crd_page_config(crd));
    if placement != MenuPlacement::Workspace {
        config.insert("SCOPE".to_string(), json!(crd_page_scope(crd)));
    }
    config.insert("HOOK_NAME".to_string(), json!("useCrdPageState"));
    Value::Object(config)
}

pub(crate) fn crd_page_config(crd: &CrdTablePageSpec) -> Value {
    let mut config = Map::new();
    config.insert("apiVersion".to_string(), json!(crd.version));
    config.insert("plural".to_string(), json!(crd.names.plural));
    config.insert("group".to_string(), json!(crd.group));
    config.insert("kapi".to_string(), json!(true));
    if let Some(kind) = crd.names.kind.as_ref() {
        config.insert("kind".to_string(), json!(kind));
    }
    Value::Object(config)
}

pub(crate) const fn crd_page_scope(crd: &CrdTablePageSpec) -> &'static str {
    match crd.scope {
        CrdScope::Namespaced => "namespace",
        CrdScope::Cluster => "cluster",
    }
}
