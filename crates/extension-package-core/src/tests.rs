use frontend_forge_api::FrontendExtension;

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
            placement: cluster
            type: page
        pages:
          - key: inspecttasks
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
        content.contains("This is a inspecttask extension, which is shown in more detail here")
    );
    assert!(content.contains("Markdown syntax"));

    let readme_zh = artifact
        .files
        .iter()
        .find(|file| file.path == "README_zh.md")
        .unwrap();
    let content = std::str::from_utf8(&readme_zh.content).unwrap();

    assert!(content.contains("这是一个inspecttask扩展组件"));
    assert!(content.contains("Markdown 语法"));

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
            placement: cluster
            type: page
          - displayName: Workspace Reports
            key: workspace-reports
            placement: workspace
            type: page
          - displayName: Global Items
            key: global-items
            placement: global
            type: page
        pages:
          - key: cluster-tasks
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
            placement: global
            type: page
        pages:
          - key: global-frame
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
