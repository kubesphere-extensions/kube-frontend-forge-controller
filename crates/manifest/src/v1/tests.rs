use frontend_forge_api::{FrontendExtension, FrontendIntegration};

use super::*;

fn render_v1_manifest(fi: &FrontendIntegration) -> Result<Value, ManifestRenderError> {
    let input = FrontendRenderInput::from_frontend_integration(fi);
    super::render_v1_manifest(&input)
}

fn render_v1_fe_manifest(fe: &FrontendExtension) -> Result<Value, ManifestRenderError> {
    let input = FrontendRenderInput::from_frontend_extension(fe)?;
    super::render_v1_manifest(&input)
}

#[test]
fn renders_workspace_crd_pages_with_workspace_page_state() {
    let fi: FrontendIntegration = serde_yaml::from_str(
        r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendIntegration
metadata:
  name: test
spec:
  menus:
    - displayName: Cluster Tasks
      key: cluster-tasks
      placement: cluster
      type: page
    - displayName: Workspace Tasks
      key: workspace-tasks
      placement: workspace
      type: page
  pages:
    - key: cluster-tasks
      type: crdTable
      crdTable:
        names:
          plural: serviceaccounts
          kind: ServiceAccount
        version: v1alpha1
        group: kubesphere.io
        scope: Namespaced
        columns:
          - key: name
            title: NAME
            enableSorting: true
            render:
              type: text
              path: metadata.name
    - key: workspace-tasks
      type: crdTable
      crdTable:
        names:
          plural: serviceaccounts
          kind: ServiceAccount
        version: v1alpha1
        group: kubesphere.io
        scope: Namespaced
        columns:
          - key: name
            title: NAME
            enableSorting: true
            render:
              type: text
              path: metadata.name
"#,
    )
    .unwrap();

    let manifest = render_v1_manifest(&fi).unwrap();
    assert_eq!(manifest["locales"], json!([]));
    let pages = manifest["pages"].as_array().unwrap();

    let cluster_page_state = &pages[0]["componentsTree"]["dataSources"][1];
    assert_eq!(cluster_page_state["type"], "crd-page-state");
    assert_eq!(
        cluster_page_state["config"]["PAGE_ID"],
        "test-cluster-cluster-tasks"
    );
    assert_eq!(cluster_page_state["config"]["SCOPE"], "namespace");

    let workspace_page_state = &pages[1]["componentsTree"]["dataSources"][1];
    assert_eq!(workspace_page_state["type"], "workspace-crd-page-state");
    assert_eq!(
        workspace_page_state["config"]["PAGE_ID"],
        "test-workspace-workspace-tasks"
    );
    assert!(workspace_page_state["config"].get("SCOPE").is_none());
}

#[test]
fn renders_display_name_fallback_and_locales_from_spec() {
    let fi: FrontendIntegration = serde_yaml::from_str(
        r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendIntegration
metadata:
  name: demo-fi
spec:
  locales:
    zh:
      xx: Chinese
      yy: Chinese 2
    en:
      xx: English
      yy: English 2
    tc:
      xx: Traditional Chinese
  menus:
    - displayName: Overview
      key: overview
      placement: cluster
      type: page
  pages:
    - key: overview
      type: iframe
      iframe:
        src: http://example.test
"#,
    )
    .unwrap();

    let manifest = render_v1_manifest(&fi).unwrap();
    assert_eq!(manifest["displayName"], "demo-fi");
    assert_eq!(
        manifest["locales"],
        json!([
            {
                "lang": "en",
                "messages": {
                    "xx": "English",
                    "yy": "English 2"
                }
            },
            {
                "lang": "tc",
                "messages": {
                    "xx": "Traditional Chinese"
                }
            },
            {
                "lang": "zh",
                "messages": {
                    "xx": "Chinese",
                    "yy": "Chinese 2"
                }
            }
        ])
    );
}

#[test]
fn renders_nested_org_menu_bindings() {
    let fi: FrontendIntegration = serde_yaml::from_str(
        r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendIntegration
metadata:
  name: demo-fi
spec:
  displayName: Demo
  menus:
    - displayName: Ops
      key: ops
      icon: Folder
      placement: workspace
      type: organization
      children:
        - displayName: Inspect Tasks
          key: inspecttasks
        - displayName: Ops Guide
          key: ops-guide
          icon: File
  pages:
    - key: inspecttasks
      type: crdTable
      crdTable:
        names:
          plural: inspecttasks
          kind: InspectTask
        group: kubeeye.kubesphere.io
        version: v1alpha2
        scope: Cluster
        columns:
          - key: name
            title: NAME
            render:
              type: text
              path: metadata.name
    - key: ops-guide
      type: iframe
      iframe:
        src: http://example.test/ops-guide
"#,
    )
    .unwrap();

    let manifest = render_v1_manifest(&fi).unwrap();
    let menus = manifest["menus"].as_array().unwrap();
    let routes = manifest["routes"].as_array().unwrap();
    let pages = manifest["pages"].as_array().unwrap();

    assert_eq!(menus.len(), 3);
    assert_eq!(routes.len(), 2);
    assert_eq!(pages.len(), 2);
    assert_eq!(menus[0]["name"], "frontendintegrations/demo-fi/ops");
    assert_eq!(menus[0]["parent"], "workspace");
    assert_eq!(menus[0]["icon"], "Folder");
    assert_eq!(
        menus[1]["parent"],
        "workspace.frontendintegrations/demo-fi/ops"
    );
    assert_eq!(
        menus[1]["name"],
        "frontendintegrations/demo-fi/ops/inspecttasks"
    );
    assert_eq!(menus[1]["icon"], "GridDuotone");
    assert_eq!(
        menus[2]["parent"],
        "workspace.frontendintegrations/demo-fi/ops"
    );
    assert_eq!(
        menus[2]["name"],
        "frontendintegrations/demo-fi/ops/ops-guide"
    );
    assert_eq!(menus[2]["icon"], "File");
    assert_eq!(
        routes[0]["path"],
        "/workspaces/:workspace/frontendintegrations/demo-fi/ops/inspecttasks"
    );
    assert_eq!(routes[0]["pageId"], "demo-fi-workspace-ops_inspecttasks");
    assert_eq!(
        routes[1]["path"],
        "/workspaces/:workspace/frontendintegrations/demo-fi/ops/ops-guide"
    );
    assert_eq!(routes[1]["pageId"], "demo-fi-workspace-ops_ops-guide");
    assert_eq!(pages[0]["componentsTree"]["meta"]["title"], "Inspect Tasks");
    assert_eq!(
        pages[0]["componentsTree"]["dataSources"][1]["type"],
        "workspace-crd-page-state"
    );
    assert_eq!(pages[1]["componentsTree"]["meta"]["title"], "Ops Guide");
}

#[test]
fn allows_multiple_fe_menus_to_bind_one_page_key() {
    let fe: FrontendExtension = serde_yaml::from_str(
        r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendExtension
metadata:
  name: reuse-demo
spec:
  package:
    name: reuse-demo
    version: 0.1.0
    displayName:
      en: Reuse Demo
    description:
      en: Reuse Demo
  source:
    type: Inline
    inline:
      schemaVersion: v1
      frontend:
        menus:
          - displayName: First Entry
            key: first-entry
            pageKey: shared-page
            placement: global
            type: page
          - displayName: Second Entry
            key: second-entry
            pageKey: shared-page
            placement: global
            type: page
        pages:
          - key: shared-page
            placement: global
            type: iframe
            iframe:
              src: http://example.test/shared
"#,
    )
    .unwrap();

    let manifest = render_v1_fe_manifest(&fe).unwrap();
    let routes = manifest["routes"].as_array().unwrap();
    let pages = manifest["pages"].as_array().unwrap();

    assert_eq!(routes.len(), 2);
    assert_eq!(pages.len(), 1);
    assert_eq!(
        routes[0]["path"],
        "/frontendextensions/reuse-demo/first-entry"
    );
    assert_eq!(
        routes[1]["path"],
        "/frontendextensions/reuse-demo/second-entry"
    );
    assert_eq!(routes[0]["pageId"], "reuse-demo-global-shared-page");
    assert_eq!(routes[1]["pageId"], "reuse-demo-global-shared-page");
    assert_eq!(pages[0]["id"], "reuse-demo-global-shared-page");
    assert_eq!(pages[0]["componentsTree"]["meta"]["title"], "First Entry");
}

#[test]
fn rejects_duplicate_fe_menu_routes() {
    let fe: FrontendExtension = serde_yaml::from_str(
        r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendExtension
metadata:
  name: duplicate-route-demo
spec:
  package:
    version: 0.1.0
    displayName:
      en: Duplicate Route Demo
    description:
      en: Duplicate Route Demo
  source:
    type: Inline
    inline:
      schemaVersion: v1
      frontend:
        menus:
          - displayName: Tools
            key: tools
            placement: global
            type: organization
            children:
              - displayName: First
                key: inspect
                pageKey: first-page
              - displayName: Second
                key: inspect
                pageKey: second-page
        pages:
          - key: first-page
            placement: global
            type: iframe
            iframe:
              src: http://example.test/first
          - key: second-page
            placement: global
            type: iframe
            iframe:
              src: http://example.test/second
"#,
    )
    .unwrap();

    assert!(matches!(
        render_v1_fe_manifest(&fe),
        Err(ManifestRenderError::DuplicateMenuRoute { .. })
    ));
}

#[test]
fn rejects_fe_page_placement_mismatch() {
    let fe: FrontendExtension = serde_yaml::from_str(
        r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendExtension
metadata:
  name: mismatch-demo
spec:
  package:
    version: 0.1.0
    displayName:
      en: Mismatch Demo
    description:
      en: Mismatch Demo
  source:
    type: Inline
    inline:
      schemaVersion: v1
      frontend:
        menus:
          - displayName: Overview
            key: overview
            pageKey: overview-page
            placement: cluster
            type: page
        pages:
          - key: overview-page
            placement: workspace
            type: iframe
            iframe:
              src: http://example.test
"#,
    )
    .unwrap();

    assert!(matches!(
        render_v1_fe_manifest(&fe),
        Err(ManifestRenderError::InvalidPageShape { .. })
    ));
}

#[test]
fn rejects_fe_page_menu_without_page_key() {
    let fe: FrontendExtension = serde_yaml::from_str(
        r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendExtension
metadata:
  name: missing-page-key
spec:
  package:
    version: 0.1.0
    displayName:
      en: Missing Page Key
    description:
      en: Missing Page Key
  source:
    type: Inline
    inline:
      schemaVersion: v1
      frontend:
        menus:
          - displayName: Overview
            key: overview
            placement: cluster
            type: page
        pages:
          - key: overview
            placement: cluster
            type: iframe
            iframe:
              src: http://example.test
"#,
    )
    .unwrap();

    assert!(matches!(
        render_v1_fe_manifest(&fe),
        Err(ManifestRenderError::InvalidMenuShape { .. })
    ));
}

#[test]
fn allows_reusing_page_keys_across_cluster_and_workspace() {
    let fi: FrontendIntegration = serde_yaml::from_str(
        r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendIntegration
metadata:
  name: demo-fi
spec:
  menus:
    - displayName: Cluster Ops
      key: cluster-ops
      placement: cluster
      type: organization
      children:
        - displayName: Global Rule Groups
          key: globalrulegroups
        - displayName: Guide
          key: guide
    - displayName: Workspace Ops
      key: workspace-ops
      placement: workspace
      type: organization
      children:
        - displayName: Global Rule Groups
          key: globalrulegroups
        - displayName: Guide
          key: guide
  pages:
    - key: globalrulegroups
      type: crdTable
      crdTable:
        names:
          plural: globalrulegroups
          kind: GlobalRuleGroup
        group: alerting.kubesphere.io
        version: v2beta1
        scope: Cluster
        columns:
          - key: name
            title: NAME
            render:
              type: text
              path: metadata.name
    - key: guide
      type: iframe
      iframe:
        src: http://example.test/guide
"#,
    )
    .unwrap();

    let manifest = render_v1_manifest(&fi).unwrap();
    let routes = manifest["routes"].as_array().unwrap();
    let pages = manifest["pages"].as_array().unwrap();

    assert_eq!(routes.len(), 4);
    assert_eq!(pages.len(), 4);
    assert_eq!(
        routes[0]["pageId"],
        "demo-fi-cluster-cluster-ops_globalrulegroups"
    );
    assert_eq!(
        routes[2]["pageId"],
        "demo-fi-workspace-workspace-ops_globalrulegroups"
    );
    assert_eq!(
        pages[0]["componentsTree"]["dataSources"][1]["type"],
        "crd-page-state"
    );
    assert_eq!(
        pages[2]["componentsTree"]["dataSources"][1]["type"],
        "workspace-crd-page-state"
    );
}

#[test]
fn omits_kind_when_crd_table_kind_is_missing() {
    let fi: FrontendIntegration = serde_yaml::from_str(
        r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendIntegration
metadata:
  name: demo-fi
spec:
  menus:
    - displayName: Inspect Tasks
      key: inspecttasks
      placement: cluster
      type: page
  pages:
    - key: inspecttasks
      type: crdTable
      crdTable:
        names:
          plural: inspecttasks
        group: kubeeye.kubesphere.io
        version: v1alpha2
        scope: Cluster
        columns:
          - key: name
            title: NAME
            render:
              type: text
              path: metadata.name
"#,
    )
    .unwrap();

    let manifest = render_v1_manifest(&fi).unwrap();
    let page = &manifest["pages"].as_array().unwrap()[0];
    let props = &page["componentsTree"]["root"]["props"];
    let page_state = &page["componentsTree"]["dataSources"][1];

    assert_eq!(
        props["CREATE_INITIAL_VALUE"],
        json!({
            "apiVersion": "kubeeye.kubesphere.io/v1alpha2",
            "metadata": {
                "name": "",
                "labels": {},
                "annotations": {}
            },
            "spec": {}
        })
    );
    assert_eq!(
        page_state["config"]["CRD_CONFIG"],
        json!({
            "apiVersion": "v1alpha2",
            "plural": "inspecttasks",
            "group": "kubeeye.kubesphere.io",
            "kapi": true
        })
    );
    assert_eq!(props["AUTH_KEY"], "");
}

#[test]
fn includes_kind_metadata_and_spec_in_crd_create_initial_value() {
    let fi: FrontendIntegration = serde_yaml::from_str(
        r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendIntegration
metadata:
  name: demo-fi
spec:
  menus:
    - displayName: Cluster Tasks
      key: cluster-tasks
      placement: cluster
      type: page
  pages:
    - key: cluster-tasks
      type: crdTable
      crdTable:
        names:
          plural: serviceaccounts
          kind: ServiceAccount
        version: v1alpha1
        group: kubesphere.io
        scope: Namespaced
        columns:
          - key: name
            title: NAME
            render:
              type: text
              path: metadata.name
"#,
    )
    .unwrap();

    let manifest = render_v1_manifest(&fi).unwrap();
    let page = &manifest["pages"].as_array().unwrap()[0];
    let props = &page["componentsTree"]["root"]["props"];

    assert_eq!(
        props["CREATE_INITIAL_VALUE"],
        json!({
            "apiVersion": "kubesphere.io/v1alpha1",
            "kind": "ServiceAccount",
            "metadata": {
                "name": "",
                "labels": {},
                "annotations": {},
                "namespace": ""
            },
            "spec": {}
        })
    );
}

#[test]
fn includes_auth_key_in_crd_table_props_when_present() {
    let fi: FrontendIntegration = serde_yaml::from_str(
        r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendIntegration
metadata:
  name: demo-fi
spec:
  menus:
    - displayName: Inspect Tasks
      key: inspecttasks
      placement: cluster
      type: page
  pages:
    - key: inspecttasks
      type: crdTable
      crdTable:
        names:
          plural: inspecttasks
          kind: InspectTask
        group: kubeeye.kubesphere.io
        version: v1alpha2
        authKey: kubeeye-auth
        scope: Cluster
        columns:
          - key: name
            title: NAME
            render:
              type: text
              path: metadata.name
"#,
    )
    .unwrap();

    let manifest = render_v1_manifest(&fi).unwrap();
    let page = &manifest["pages"].as_array().unwrap()[0];
    let props = &page["componentsTree"]["root"]["props"];
    let page_state = &page["componentsTree"]["dataSources"][1];

    assert_eq!(props["AUTH_KEY"], "kubeeye-auth");
    assert!(page_state["config"]["CRD_CONFIG"].get("authKey").is_none());
}

#[test]
fn keeps_distinct_page_ids_for_top_level_and_nested_suffixes() {
    let fi: FrontendIntegration = serde_yaml::from_str(
        r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendIntegration
metadata:
  name: demo-fi
spec:
  menus:
    - displayName: Ops Guide
      key: ops-guide
      placement: workspace
      type: page
    - displayName: Ops
      key: ops
      placement: workspace
      type: organization
      children:
        - displayName: Guide
          key: guide
  pages:
    - key: ops-guide
      type: iframe
      iframe:
        src: http://example.test/top-level
    - key: guide
      type: iframe
      iframe:
        src: http://example.test/nested
"#,
    )
    .unwrap();

    let manifest = render_v1_manifest(&fi).unwrap();
    let routes = manifest["routes"].as_array().unwrap();
    let pages = manifest["pages"].as_array().unwrap();

    assert_eq!(routes[0]["pageId"], "demo-fi-workspace-ops-guide");
    assert_eq!(routes[1]["pageId"], "demo-fi-workspace-ops_guide");
    assert_ne!(routes[0]["pageId"], routes[1]["pageId"]);
    assert_eq!(pages[0]["id"], "demo-fi-workspace-ops-guide");
    assert_eq!(pages[1]["id"], "demo-fi-workspace-ops_guide");
}

#[test]
fn rejects_page_menu_with_children() {
    let fi: FrontendIntegration = serde_yaml::from_str(
        r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendIntegration
metadata:
  name: demo
spec:
  menus:
    - displayName: Overview
      key: overview
      placement: cluster
      type: page
      children:
        - displayName: Child
          key: child
  pages:
    - key: overview
      type: iframe
      iframe:
        src: http://example.test
"#,
    )
    .unwrap();

    assert!(matches!(
        render_v1_manifest(&fi),
        Err(ManifestRenderError::InvalidMenuShape { .. })
    ));
}

#[test]
fn rejects_org_menu_without_children() {
    let fi: FrontendIntegration = serde_yaml::from_str(
        r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendIntegration
metadata:
  name: demo
spec:
  menus:
    - displayName: Ops
      key: ops
      placement: cluster
      type: organization
  pages: []
"#,
    )
    .unwrap();

    assert!(matches!(
        render_v1_manifest(&fi),
        Err(ManifestRenderError::InvalidMenuShape { .. })
    ));
}

#[test]
fn rejects_missing_page_for_menu_key() {
    let fi: FrontendIntegration = serde_yaml::from_str(
        r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendIntegration
metadata:
  name: demo
spec:
  menus:
    - displayName: Overview
      key: overview
      placement: cluster
      type: page
  pages: []
"#,
    )
    .unwrap();

    assert!(matches!(
        render_v1_manifest(&fi),
        Err(ManifestRenderError::MissingPageForMenuKey { .. })
    ));
}

#[test]
fn rejects_orphan_page_config() {
    let fi: FrontendIntegration = serde_yaml::from_str(
        r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendIntegration
metadata:
  name: demo
spec:
  menus:
    - displayName: Overview
      key: overview
      placement: cluster
      type: page
  pages:
    - key: overview
      type: iframe
      iframe:
        src: http://example.test
    - key: orphan
      type: iframe
      iframe:
        src: http://example.test/orphan
"#,
    )
    .unwrap();

    assert!(matches!(
        render_v1_manifest(&fi),
        Err(ManifestRenderError::OrphanPageConfig { .. })
    ));
}

#[test]
fn rejects_invalid_page_shapes_and_keys() {
    let invalid_menu_key: FrontendIntegration = serde_yaml::from_str(
        r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendIntegration
metadata:
  name: demo
spec:
  menus:
    - displayName: Overview
      key: invalid_key
      placement: cluster
      type: page
  pages:
    - key: overview
      type: iframe
      iframe:
        src: http://example.test
"#,
    )
    .unwrap();
    assert!(matches!(
        render_v1_manifest(&invalid_menu_key),
        Err(ManifestRenderError::InvalidMenuKey { .. })
    ));

    let invalid_page_key: FrontendIntegration = serde_yaml::from_str(
        r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendIntegration
metadata:
  name: demo
spec:
  menus:
    - displayName: Overview
      key: overview
      placement: cluster
      type: page
  pages:
    - key: invalid_key
      type: iframe
      iframe:
        src: http://example.test
"#,
    )
    .unwrap();
    assert!(matches!(
        render_v1_manifest(&invalid_page_key),
        Err(ManifestRenderError::InvalidPageShape { .. })
    ));

    let missing_iframe: FrontendIntegration = serde_yaml::from_str(
        r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendIntegration
metadata:
  name: demo
spec:
  menus:
    - displayName: Overview
      key: overview
      placement: cluster
      type: page
  pages:
    - key: overview
      type: iframe
"#,
    )
    .unwrap();
    assert!(matches!(
        render_v1_manifest(&missing_iframe),
        Err(ManifestRenderError::InvalidPageShape { .. })
    ));

    let missing_columns: FrontendIntegration = serde_yaml::from_str(
        r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendIntegration
metadata:
  name: demo
spec:
  menus:
    - displayName: Overview
      key: overview
      placement: cluster
      type: page
  pages:
    - key: overview
      type: crdTable
      crdTable:
        names:
          plural: serviceaccounts
          kind: ServiceAccount
        version: v1alpha1
        group: kubesphere.io
        scope: Namespaced
        columns: []
"#,
    )
    .unwrap();
    assert!(matches!(
        render_v1_manifest(&missing_columns),
        Err(ManifestRenderError::MissingCrdColumns { .. })
    ));
}
