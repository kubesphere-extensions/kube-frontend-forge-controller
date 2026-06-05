use frontend_forge_api::FrontendExtension;
use serde_json::json;

use super::*;

fn sample_fe() -> FrontendExtension {
    serde_yaml::from_str(
        r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendExtension
metadata:
  name: fe-inspecttask
spec:
  package:
    name: inspecttask
    version: 0.1.0
    displayName:
      zh: 巡检任务
      en: Inspect Task
    description:
      zh: InspectTask extension package
      en: InspectTask extension package
    category: dev-tools
    keywords:
      - Frontend
    sources:
      - https://github.com/kubesphere-extensions/frontend-forge
    kubeVersion: ">=1.23.0-0"
    ksVersion: ">=4.2.1-0"
    maintainers:
      - name: KubeSphere
        email: kubesphere@yunify.com
    home: https://kubesphere.com.cn/
    provider:
      zh:
        name: 北京青云科技股份有限公司
        email: kubesphere@yunify.com
        url: https://kubesphere.com.cn/
      en:
        name: QingCloud Technologies
        email: kubesphere@yunify.com
        url: https://kubesphere.co/
    icon: ./static/frontend-forge.ico
    dependencies:
      - name: frontend
        tags:
          - extension
      - name: frontend-forge
        tags:
          - extension
    installationMode: HostOnly
    images:
      - kubesphere/frontend-forge-console:v1.0.0
      - kubesphere/frontend-forge-controller:v1.0.0
      - kubesphere/frontend-forge-runner:v1.0.0
    charts:
      values:
        replicaCount: 1
  source:
    type: Inline
    inline:
      schemaVersion: v1
      frontend:
        menus:
          - displayName: Inspect Tasks
            key: inspecttasks
            pageKey: inspecttasks
            placements:
              - cluster
            type: page
        pages:
          - key: inspecttasks
            placements:
              - cluster
            type: iframe
            iframe:
              src: http://example.test
      extensionResources:
        jsBundle:
          name: inspecttask
"#,
    )
    .unwrap()
}

#[test]
fn builds_extension_package_artifact_payload() {
    let generated_at = DateTime::from_timestamp(1_775_200_000, 0).unwrap();
    let artifact = build_extension_package(
        &sample_fe(),
        generated_at,
        "System.register([], function () {});",
    )
    .unwrap();

    assert_eq!(artifact.filename, "inspecttask-0.1.0.tgz");
    assert_eq!(artifact.media_type, PACKAGE_MEDIA_TYPE);
    assert!(artifact.digest.starts_with("sha256:"));
    assert_eq!(artifact.generated_at, generated_at);
    assert!(artifact.payload.data.contains_key(ARTIFACT_METADATA_KEY));
    assert!(artifact.payload.data.contains_key(FILES_METADATA_KEY));
    let artifact_metadata: Value =
        serde_json::from_str(&artifact.payload.data[ARTIFACT_METADATA_KEY]).unwrap();
    assert!(artifact_metadata.get("files").is_none());
    let file_metadata: Vec<PackageFileMeta> =
        serde_json::from_str(&artifact.payload.data[FILES_METADATA_KEY]).unwrap();
    assert!(!file_metadata.is_empty());
    assert_eq!(
        artifact.payload.binary_data[PACKAGE_KEY][..3],
        [0x1f, 0x8b, 0x08]
    );
    let paths = artifact
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<Vec<_>>();
    assert!(paths.contains(&"extension.yaml"));
    assert!(paths.contains(&"permissions.yaml"));
    assert!(paths.contains(&"values.yaml"));
    assert!(paths.contains(&"README.md"));
    assert!(paths.contains(&"README_zh.md"));
    assert!(paths.contains(&"charts/frontend/Chart.yaml"));
    assert!(paths.contains(&"charts/frontend/values.yaml"));
    assert!(paths.contains(&"charts/frontend/scripts/index.js"));
    assert!(paths.contains(&"charts/frontend/templates/configmap.yaml"));
    assert!(paths.contains(&"charts/frontend/templates/extensions.yaml"));
    assert!(paths.contains(&"charts/frontend/templates/helps.tpl"));
    assert!(paths.contains(&"charts/inspecttask-helper/Chart.yaml"));
    assert!(paths.contains(&"charts/inspecttask-helper/values.yaml"));
    assert!(paths.contains(&"charts/inspecttask-helper/templates/roleTemplate.yaml"));
    assert!(!paths.contains(&"frontend/manifest.json"));
    assert!(!paths.contains(&"frontend/Chart.yaml"));
    assert!(!paths.contains(&"charts/inspecttask/values.yaml"));
    assert!(!paths.contains(&"resources/jsbundle.yaml"));
    assert!(!paths.contains(&"resources/roletemplates/inspecttask-view.yaml"));

    let extension_yaml = artifact
        .files
        .iter()
        .find(|file| file.path == "extension.yaml")
        .unwrap();
    let content = std::str::from_utf8(&extension_yaml.content).unwrap();

    assert!(content.contains("apiVersion: kubesphere.io/v1alpha1"));
    assert!(content.contains("name: inspecttask"));
    assert!(content.contains("displayName:"));
    assert!(!content.contains("staticFileDirectory:"));
    assert!(!content.contains("name: inspecttask-helper"));
    assert!(!content.contains("- agent"));
    assert!(content.contains("name: frontend\n"));
    assert!(content.contains("- extension"));
    assert!(content.contains("name: frontend-forge\n"));
    assert!(content.contains("installationMode: HostOnly"));
    assert!(content.contains("kubesphere/frontend-forge-controller:v1.0.0"));

    let readme = artifact
        .files
        .iter()
        .find(|file| file.path == "README.md")
        .unwrap();
    let content = std::str::from_utf8(&readme.content).unwrap();

    assert!(
        content.contains("Inspect Task is built with KubeSphere rapid integration capabilities")
    );
    assert!(content.contains("## Features"));
    assert!(content.contains("### 1: \"Inspect Tasks\" (Page Integration)"));
    assert!(content.contains("Embeds a third-party page through IFrame."));
    assert!(content.contains("```text\nhttp://example.test\n```"));
    assert!(content.contains("Menu entry: Cluster"));
    assert!(content.contains("## Quick Start"));

    let readme_zh = artifact
        .files
        .iter()
        .find(|file| file.path == "README_zh.md")
        .unwrap();
    let content = std::str::from_utf8(&readme_zh.content).unwrap();

    assert!(content.contains("巡检任务 基于 KubeSphere 快速集成能力构建的扩展组件"));
    assert!(content.contains("## 功能"));
    assert!(content.contains("### 1：「Inspect Tasks」（页面集成）"));
    assert!(content.contains("通过 IFrame 方式嵌入第三方页面。"));
    assert!(content.contains("```text\nhttp://example.test\n```"));
    assert!(content.contains("菜单入口：集群"));
    assert!(content.contains("## 快速开始"));

    let frontend_chart = artifact
        .files
        .iter()
        .find(|file| file.path == "charts/frontend/Chart.yaml")
        .unwrap();
    let content = std::str::from_utf8(&frontend_chart.content).unwrap();

    assert!(content.contains("apiVersion: v2"));
    assert!(content.contains("type: application"));

    let permissions = artifact
        .files
        .iter()
        .find(|file| file.path == "permissions.yaml")
        .unwrap();
    let content = std::str::from_utf8(&permissions.content).unwrap();

    assert!(content.contains("kind: ClusterRole"));
    assert!(content.contains("kind: Role"));
    assert!(content.contains("- 'categories'"));
    assert!(content.contains("- 'roletemplates'"));
}

#[test]
fn readme_zh_describes_generated_integrations() {
    let generated_at = DateTime::from_timestamp(1_775_200_000, 0).unwrap();
    let fe: FrontendExtension = serde_yaml::from_str(
        r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendExtension
metadata:
  name: qqqq
spec:
  package:
    name: qqqq
    version: 0.1.0
    displayName:
      zh: qqqq
    description:
      zh: qqqq
  source:
    type: Inline
    inline:
      schemaVersion: v1
      frontend:
        menus:
          - displayName: Demo1
            key: demo1
            pageKey: demo1
            placements:
              - cluster
            type: page
          - displayName: Demo2
            key: demo2
            pageKey: demo2
            placements:
              - cluster
              - workspace
            type: page
          - displayName: Demo3
            key: demo3
            pageKey: demo3
            placements:
              - cluster
            type: page
        pages:
          - key: demo1
            displayName: Demo1 Page
            placements:
              - cluster
            type: crdTable
            crdTable:
              names:
                plural: clusterreports
              group: sample.frontend-forge.io
              version: v1alpha1
              scope: Cluster
              columns:
                - key: name
                  title: NAME
                  render:
                    type: text
                    path: metadata.name
          - key: demo2
            placements:
              - cluster
              - workspace
            type: crdTable
            crdTable:
              names:
                plural: namespacewidgets
              group: sample.frontend-forge.io
              version: v1alpha1
              scope: Namespaced
              columns:
                - key: name
                  title: NAME
                  render:
                    type: text
                    path: metadata.name
          - key: demo3
            placements:
              - cluster
            type: iframe
            iframe:
              src: https://www.openstreetmap.org/export/embed.html?bbox=-0.004017949104309083%2C51.47612752641776%2C0.00030577182769775396%2C51.478569861898606&layer=mapnik
"#,
    )
    .unwrap();
    let artifact = build_extension_package(&fe, generated_at, "console.log('ok');").unwrap();
    let readme_zh = artifact
        .files
        .iter()
        .find(|file| file.path == "README_zh.md")
        .unwrap();
    let content = std::str::from_utf8(&readme_zh.content).unwrap();

    assert!(content.starts_with("qqqq 基于 KubeSphere 快速集成能力构建的扩展组件"));
    assert!(content.contains("### 1：「Demo1 Page」（资源集成）"));
    assert!(content.contains("通过 Kubernetes CRD（Custom Resource Definition）方式扩展平台资源"));
    assert!(content.contains("* API Version：`sample.frontend-forge.io/v1alpha1`"));
    assert!(content.contains("* Resource：`clusterreports`"));
    assert!(content.contains("### 2：「Demo2」（资源集成）"));
    assert!(content.contains("* Resource：`namespacewidgets`"));
    assert!(content.contains("菜单入口：集群、企业空间"));
    assert!(content.contains("### 3：「Demo3」（页面集成）"));
    assert!(content.contains("通过 IFrame 方式嵌入第三方页面。"));
    assert!(content.contains("```text\nhttps://www.openstreetmap.org/export/embed.html?bbox=-0.004017949104309083%2C51.47612752641776%2C0.00030577182769775396%2C51.478569861898606&layer=mapnik\n```"));
    assert!(content.contains(
        "扩展安装完成后，可在集群、企业空间看到菜单 「Demo1」、「Demo2」、「Demo3」 入口。"
    ));
    assert!(content.contains("1. 进入一级菜单「Demo1」，查看 `clusterreports` 资源。"));
    assert!(!content.contains("进入「Demo1 Page」"));
    assert!(content.contains("2. 进入一级菜单「Demo2」，查看 `namespacewidgets` 资源。"));
    assert!(content.contains("3. 进入一级菜单「Demo3」，访问嵌入的第三方页面。"));
    assert!(!content.contains("- \n"));
}

#[test]
fn readme_quick_start_describes_secondary_menus() {
    let generated_at = DateTime::from_timestamp(1_775_200_000, 0).unwrap();
    let fe: FrontendExtension = serde_yaml::from_str(
        r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendExtension
metadata:
  name: nested
spec:
  package:
    name: nested
    version: 0.1.0
    displayName:
      zh: Nested
    description:
      zh: Nested
  source:
    type: Inline
    inline:
      schemaVersion: v1
      frontend:
        menus:
          - displayName: Parent
            key: parent
            placements:
              - cluster
            type: organization
            children:
              - displayName: Child Report
                key: child-report
                pageKey: child-report
        pages:
          - key: child-report
            displayName: Child Report Page
            placements:
              - cluster
            type: crdTable
            crdTable:
              names:
                plural: childreports
              group: sample.frontend-forge.io
              version: v1alpha1
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
    let artifact = build_extension_package(&fe, generated_at, "console.log('ok');").unwrap();
    let readme_zh = artifact
        .files
        .iter()
        .find(|file| file.path == "README_zh.md")
        .unwrap();
    let content = std::str::from_utf8(&readme_zh.content).unwrap();

    assert!(content.contains("### 1：「Child Report Page」（资源集成）"));
    assert!(content.contains("1. 进入二级菜单「Child Report」，查看 `childreports` 资源。"));
    assert!(!content.contains("进入二级菜单「Child Report Page」"));
}

#[test]
fn readme_template_receives_frontend_extension_cr_object() {
    let fe = sample_fe();
    let pages = resolve_frontend_extension_pages(&fe).unwrap();
    let fe_cr = serde_json::to_value(&fe).unwrap();
    let content = render_readme_template(
        "README_zh.md.tpl",
        "inspecttask",
        &fe_cr,
        &pages,
        Locale::Zh,
    )
    .unwrap();

    assert_eq!(fe_cr["metadata"]["name"], "fe-inspecttask");
    assert_eq!(fe_cr["spec"]["package"]["name"], "inspecttask");
    assert!(fe_cr.get("status").is_none());
    assert!(content.starts_with("巡检任务 基于 KubeSphere 快速集成能力构建的扩展组件"));
}

#[test]
fn readme_placement_phrase_handles_empty_placements() {
    assert_eq!(placement_phrase(&[], Locale::En), "**Unknown**");
    assert_eq!(placement_phrase(&[], Locale::Zh), "**未知**");
}

#[test]
fn readme_template_supports_plain_string_display_name() {
    let fe = sample_fe();
    let pages = resolve_frontend_extension_pages(&fe).unwrap();
    let mut fe_cr = serde_json::to_value(&fe).unwrap();
    fe_cr["spec"]["package"]["displayName"] = json!("Plain Inspect Task");

    let content =
        render_readme_template("README.md.tpl", "inspecttask", &fe_cr, &pages, Locale::En).unwrap();

    assert!(
        content.starts_with(
            "Plain Inspect Task is built with KubeSphere rapid integration capabilities"
        )
    );
}

#[test]
fn readme_template_localizes_menu_display_name_maps() {
    let fe = sample_fe();
    let pages = resolve_frontend_extension_pages(&fe).unwrap();
    let mut fe_cr = serde_json::to_value(&fe).unwrap();
    fe_cr["spec"]["source"]["inline"]["frontend"]["menus"][0]["displayName"] =
        json!({ "en": "Inspection", "zh": "巡检" });

    let content = render_readme_template(
        "README_zh.md.tpl",
        "inspecttask",
        &fe_cr,
        &pages,
        Locale::Zh,
    )
    .unwrap();

    assert!(content.contains("菜单 「巡检」 入口。"));
}

#[test]
fn readme_template_falls_back_to_menu_key_for_missing_display_name() {
    let fe = sample_fe();
    let pages = resolve_frontend_extension_pages(&fe).unwrap();
    let mut fe_cr = serde_json::to_value(&fe).unwrap();
    fe_cr["spec"]["source"]["inline"]["frontend"]["menus"][0]
        .as_object_mut()
        .unwrap()
        .remove("displayName");

    let content = render_readme_template(
        "README_zh.md.tpl",
        "inspecttask",
        &fe_cr,
        &pages,
        Locale::Zh,
    )
    .unwrap();

    assert!(content.contains("菜单 「inspecttasks」 入口。"));
}

#[test]
fn frontend_configmap_loads_index_js_from_chart_file() {
    let generated_at = DateTime::from_timestamp(1_775_200_000, 0).unwrap();
    let artifact = build_extension_package(
        &sample_fe(),
        generated_at,
        "System.register([], function () {\n  console.log('ok');\n});",
    )
    .unwrap();

    let script = artifact
        .files
        .iter()
        .find(|file| file.path == "charts/frontend/scripts/index.js")
        .unwrap();
    let content = std::str::from_utf8(&script.content).unwrap();

    assert!(content.contains("System.register([], function () {"));
    assert!(content.contains("  console.log('ok');"));

    let template = artifact
        .files
        .iter()
        .find(|file| file.path == "charts/frontend/templates/configmap.yaml")
        .unwrap();
    let content = std::str::from_utf8(&template.content).unwrap();

    assert!(content.contains(r#"{{ (.Files.Glob "scripts/index.js").AsConfig | indent 2 }}"#));
    assert!(!content.contains(".Values.indexJsContent"));
}

#[test]
fn role_templates_are_generated_from_pages_and_ignore_explicit_resources() {
    let generated_at = DateTime::from_timestamp(1_775_200_000, 0).unwrap();
    let artifact =
        build_extension_package(&sample_fe(), generated_at, "console.log('ok');").unwrap();
    let template = artifact
        .files
        .iter()
        .find(|file| file.path == "charts/inspecttask-helper/templates/roleTemplate.yaml")
        .unwrap();
    let content = std::str::from_utf8(&template.content).unwrap();

    assert!(content.contains("name: cluster-view-inspecttask"));
    assert!(content.contains("rules: []"));
    assert!(
        content.contains(r#"iam.kubesphere.io/role-template-rules: '{"inspecttasks":"view"}'"#)
    );
    assert!(!content.contains("inspecttask-view"));
}

#[test]
fn source_hash_changes_with_package_content() {
    let generated_at = DateTime::from_timestamp(1_775_200_000, 0).unwrap();
    let a = sample_fe();
    let mut b = sample_fe();
    b.spec.package.version = "0.2.0".to_string();

    let a = build_extension_package(&a, generated_at, "console.log('a');").unwrap();
    let b = build_extension_package(&b, generated_at, "console.log('a');").unwrap();

    assert_ne!(a.source_hash, b.source_hash);
    assert_ne!(a.digest, b.digest);
}

#[test]
fn source_hash_ignores_deprecated_extension_resources() {
    let mut a = sample_fe();
    let mut b = sample_fe();
    let resources = b.spec.source.inline.extension_resources.as_mut().unwrap();
    resources.js_bundle.as_mut().unwrap().name = "changed-jsbundle".to_string();

    assert_eq!(
        frontend_extension_source_hash(&a).unwrap(),
        frontend_extension_source_hash(&b).unwrap()
    );

    a.spec.source.inline.extension_resources = None;
    assert_eq!(
        frontend_extension_source_hash(&a).unwrap(),
        frontend_extension_source_hash(&b).unwrap()
    );
}

#[test]
fn source_hash_changes_with_package_dependencies() {
    let a = sample_fe();
    let mut b = sample_fe();
    b.spec.package.dependencies = Some(vec![ExtensionDependencySpec {
        name: "legacy-chart".to_string(),
        tags: vec!["extension".to_string()],
    }]);

    assert_ne!(
        frontend_extension_source_hash(&a).unwrap(),
        frontend_extension_source_hash(&b).unwrap()
    );
}

#[test]
fn source_hash_changes_with_static_file_directory() {
    let a = sample_fe();
    let mut b = sample_fe();
    b.spec.package.static_file_directory = Some("public".to_string());

    assert_ne!(
        frontend_extension_source_hash(&a).unwrap(),
        frontend_extension_source_hash(&b).unwrap()
    );
}

#[test]
fn extension_yaml_defaults_missing_dependencies_to_generated_charts() {
    let generated_at = DateTime::from_timestamp(1_775_200_000, 0).unwrap();
    let mut fe = sample_fe();
    fe.spec.package.dependencies = None;

    let artifact = build_extension_package(&fe, generated_at, "console.log('ok');").unwrap();
    let extension_yaml = artifact
        .files
        .iter()
        .find(|file| file.path == "extension.yaml")
        .unwrap();
    let content = std::str::from_utf8(&extension_yaml.content).unwrap();

    assert!(content.contains("name: inspecttask-helper"));
    assert!(content.contains("- agent"));
    assert!(content.contains("name: frontend\n"));
    assert!(content.contains("- extension"));
}

#[test]
fn extension_yaml_uses_explicit_empty_dependencies() {
    let generated_at = DateTime::from_timestamp(1_775_200_000, 0).unwrap();
    let mut fe = sample_fe();
    fe.spec.package.dependencies = Some(Vec::new());

    let artifact = build_extension_package(&fe, generated_at, "console.log('ok');").unwrap();
    let extension_yaml = artifact
        .files
        .iter()
        .find(|file| file.path == "extension.yaml")
        .unwrap();
    let content = std::str::from_utf8(&extension_yaml.content).unwrap();

    assert!(content.contains("dependencies: []"));
    assert!(!content.contains("name: inspecttask-helper"));
    assert!(!content.contains("name: frontend\n"));
}

#[test]
fn gzip_payload_is_deterministic() {
    let generated_at = DateTime::from_timestamp(1_775_200_000, 0).unwrap();
    let a = build_extension_package(&sample_fe(), generated_at, "console.log('a');").unwrap();
    let b = build_extension_package(&sample_fe(), generated_at, "console.log('a');").unwrap();

    assert_eq!(a.digest, b.digest);
    assert_eq!(
        a.payload.binary_data[PACKAGE_KEY],
        b.payload.binary_data[PACKAGE_KEY]
    );
}

#[test]
fn crd_table_role_templates_are_aggregated_by_scope_and_action() {
    let generated_at = DateTime::from_timestamp(1_775_200_000, 0).unwrap();
    let fe: FrontendExtension = serde_yaml::from_str(
        r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendExtension
metadata:
  name: mixed
spec:
  package:
    name: mixed
    version: 0.1.0
    displayName:
      en: Mixed
    description:
      en: Mixed
  source:
    type: Inline
    inline:
      schemaVersion: v1
      frontend:
        menus:
          - displayName: Cluster Tasks
            key: cluster-tasks
            pageKey: cluster-tasks
            placements:
              - cluster
            type: page
          - displayName: Workspace Reports
            key: workspace-reports
            pageKey: workspace-reports
            placements:
              - workspace
            type: page
          - displayName: Global Items
            key: global-items
            pageKey: global-items
            placements:
              - global
            type: page
        pages:
          - key: cluster-tasks
            placements:
              - cluster
            type: crdTable
            crdTable:
              group: kubeeye.kubesphere.io
              version: v1alpha2
              scope: Cluster
              authKey: inspecttask-auth
              names:
                kind: InspectTask
                plural: inspecttasks
              columns:
                - key: name
                  title: NAME
                  render:
                    type: text
                    path: metadata.name
          - key: workspace-reports
            placements:
              - workspace
            type: crdTable
            crdTable:
              group: reports.kubesphere.io
              version: v1alpha1
              scope: Namespaced
              names:
                kind: Report
                plural: reports
              columns:
                - key: name
                  title: NAME
                  render:
                    type: text
                    path: metadata.name
          - key: global-items
            placements:
              - global
            type: crdTable
            crdTable:
              group: items.kubesphere.io
              version: v1
              scope: Namespaced
              names:
                kind: Item
                plural: items
              columns:
                - key: name
                  title: NAME
                  render:
                    type: text
                    path: metadata.name
"#,
    )
    .unwrap();
    let artifact = build_extension_package(&fe, generated_at, "console.log('ok');").unwrap();
    let template = artifact
        .files
        .iter()
        .find(|file| file.path == "charts/mixed-helper/templates/roleTemplate.yaml")
        .unwrap();
    let content = std::str::from_utf8(&template.content).unwrap();

    assert!(content.contains("name: cluster-view-mixed"));
    assert!(content.contains("name: cluster-manage-mixed"));
    assert!(content.contains("name: namespace-view-mixed"));
    assert!(content.contains("name: namespace-manage-mixed"));
    assert!(content.contains(r#""inspecttask-auth":"view""#));
    assert!(content.contains(r#""workspace-reports":"view""#));
    assert!(content.contains(r#""global-items":"manage""#));
    assert!(content.contains("- 'inspecttasks'"));
    assert!(content.contains("- 'reports'"));
    assert!(content.contains("- 'items'"));
    assert!(content.contains("- '*'"));
    assert!(!content.contains("    - *\n"));
    assert!(content.contains("iam.kubesphere.io/dependencies: '[\"cluster-view-mixed\"]'"));
    assert!(content.contains("iam.kubesphere.io/dependencies: '[\"namespace-view-mixed\"]'"));
}

#[test]
fn role_template_scope_resolution_matches_page_rules() {
    assert_eq!(
        crd_role_scope(MenuPlacement::Cluster, CrdScope::Namespaced),
        RoleScope::Cluster
    );
    assert_eq!(
        crd_role_scope(MenuPlacement::Workspace, CrdScope::Cluster),
        RoleScope::Namespace
    );
    assert_eq!(
        crd_role_scope(MenuPlacement::Global, CrdScope::Cluster),
        RoleScope::Cluster
    );
    assert_eq!(
        crd_role_scope(MenuPlacement::Global, CrdScope::Namespaced),
        RoleScope::Namespace
    );
    assert_eq!(
        iframe_role_scope(MenuPlacement::Cluster),
        Some(RoleScope::Cluster)
    );
    assert_eq!(
        iframe_role_scope(MenuPlacement::Workspace),
        Some(RoleScope::Namespace)
    );
    assert_eq!(iframe_role_scope(MenuPlacement::Global), None);
}

#[test]
fn iframe_global_pages_do_not_generate_role_templates() {
    let generated_at = DateTime::from_timestamp(1_775_200_000, 0).unwrap();
    let fe: FrontendExtension = serde_yaml::from_str(
        r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendExtension
metadata:
  name: global-frame
spec:
  package:
    version: 0.1.0
    displayName:
      en: Global Frame
    description:
      en: Global Frame
  source:
    type: Inline
    inline:
      schemaVersion: v1
      frontend:
        menus:
          - displayName: Global Frame
            key: global-frame
            pageKey: global-frame
            placements:
              - global
            type: page
        pages:
          - key: global-frame
            placements:
              - global
            type: iframe
            iframe:
              src: http://example.test
"#,
    )
    .unwrap();
    let artifact = build_extension_package(&fe, generated_at, "console.log('ok');").unwrap();
    let template = artifact
        .files
        .iter()
        .find(|file| file.path == "charts/global-frame-helper/templates/roleTemplate.yaml")
        .unwrap();
    let content = std::str::from_utf8(&template.content).unwrap();

    assert_eq!(
        content.trim(),
        "{{- if .Values.roleTemplate.enabled }}\n{{- end }}"
    );
}
