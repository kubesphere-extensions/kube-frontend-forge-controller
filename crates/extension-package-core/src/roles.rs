use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum RoleScope {
    Cluster,
    Workspace,
    Namespace,
}

const ROLE_SCOPES: [RoleScope; 3] = [
    RoleScope::Cluster,
    RoleScope::Workspace,
    RoleScope::Namespace,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum RoleAction {
    View,
    Manage,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct GeneratedRule {
    api_group: String,
    resource: String,
    verbs: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct RoleTemplateAggregate {
    action_keys: BTreeSet<String>,
    rules: BTreeSet<GeneratedRule>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct RoleTemplateAggregates {
    cluster_view: RoleTemplateAggregate,
    cluster_manage: RoleTemplateAggregate,
    workspace_view: RoleTemplateAggregate,
    workspace_manage: RoleTemplateAggregate,
    namespace_view: RoleTemplateAggregate,
    namespace_manage: RoleTemplateAggregate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RoleTemplateContribution {
    scope: RoleScope,
    action: RoleAction,
    action_key: String,
    rule: Option<GeneratedRule>,
}

pub(crate) fn role_template_template(
    package_name: &str,
    pages: &[ResolvedFrontendPage],
) -> Result<String, ExtensionPackageError> {
    let aggregates = role_template_aggregates(pages);
    let category_scopes = ROLE_SCOPES
        .into_iter()
        .filter(|scope| aggregates.has_scope(*scope))
        .collect::<Vec<_>>();
    let mut out = String::from("{{- if .Values.roleTemplate.enabled }}\n");
    out.push_str(&category_installer_template(package_name, &category_scopes));

    for scope in ROLE_SCOPES {
        for action in [RoleAction::View, RoleAction::Manage] {
            append_role_template(
                &mut out,
                package_name,
                scope,
                action,
                aggregates.aggregate(scope, action),
            )?;
        }
    }

    out.push_str("{{- end }}\n");
    Ok(out)
}

impl RoleTemplateAggregates {
    const fn aggregate(&self, scope: RoleScope, action: RoleAction) -> &RoleTemplateAggregate {
        match (scope, action) {
            (RoleScope::Cluster, RoleAction::View) => &self.cluster_view,
            (RoleScope::Cluster, RoleAction::Manage) => &self.cluster_manage,
            (RoleScope::Workspace, RoleAction::View) => &self.workspace_view,
            (RoleScope::Workspace, RoleAction::Manage) => &self.workspace_manage,
            (RoleScope::Namespace, RoleAction::View) => &self.namespace_view,
            (RoleScope::Namespace, RoleAction::Manage) => &self.namespace_manage,
        }
    }

    const fn aggregate_mut(
        &mut self,
        scope: RoleScope,
        action: RoleAction,
    ) -> &mut RoleTemplateAggregate {
        match (scope, action) {
            (RoleScope::Cluster, RoleAction::View) => &mut self.cluster_view,
            (RoleScope::Cluster, RoleAction::Manage) => &mut self.cluster_manage,
            (RoleScope::Workspace, RoleAction::View) => &mut self.workspace_view,
            (RoleScope::Workspace, RoleAction::Manage) => &mut self.workspace_manage,
            (RoleScope::Namespace, RoleAction::View) => &mut self.namespace_view,
            (RoleScope::Namespace, RoleAction::Manage) => &mut self.namespace_manage,
        }
    }

    fn has_scope(&self, scope: RoleScope) -> bool {
        !self.aggregate(scope, RoleAction::View).is_empty()
            || !self.aggregate(scope, RoleAction::Manage).is_empty()
    }
}

impl RoleTemplateAggregate {
    fn is_empty(&self) -> bool {
        self.action_keys.is_empty()
    }
}

pub(crate) fn role_template_aggregates(pages: &[ResolvedFrontendPage]) -> RoleTemplateAggregates {
    let mut aggregates = RoleTemplateAggregates::default();

    for page in pages {
        for contribution in role_template_contributions(page) {
            add_role_rule(&mut aggregates, contribution);
        }
    }

    aggregates
}

pub(crate) fn role_template_contributions(
    page: &ResolvedFrontendPage,
) -> Vec<RoleTemplateContribution> {
    match page.page.type_ {
        PageType::CrdTable => crd_table_role_template_contributions(page),
        PageType::Iframe => iframe_role_template_contribution(page)
            .into_iter()
            .collect(),
    }
}

pub(crate) fn crd_table_role_template_contributions(
    page: &ResolvedFrontendPage,
) -> Vec<RoleTemplateContribution> {
    let Some(crd) = page.page.crd_table.as_ref() else {
        return Vec::new();
    };
    let scope = crd_role_scope(page.placement, crd.scope);
    let action_key = crd
        .auth_key
        .clone()
        .unwrap_or_else(|| page.action_key.clone());

    vec![
        RoleTemplateContribution {
            scope,
            action: RoleAction::View,
            action_key: action_key.clone(),
            rule: Some(GeneratedRule {
                api_group: crd.group.clone(),
                resource: crd.names.plural.clone(),
                verbs: view_verbs(),
            }),
        },
        RoleTemplateContribution {
            scope,
            action: RoleAction::Manage,
            action_key,
            rule: Some(GeneratedRule {
                api_group: crd.group.clone(),
                resource: crd.names.plural.clone(),
                verbs: manage_verbs(),
            }),
        },
    ]
}

pub(crate) fn iframe_role_template_contribution(
    page: &ResolvedFrontendPage,
) -> Option<RoleTemplateContribution> {
    let scope = iframe_role_scope(page.placement)?;
    Some(RoleTemplateContribution {
        scope,
        action: RoleAction::View,
        action_key: page.action_key.clone(),
        rule: None,
    })
}

pub(crate) fn add_role_rule(
    aggregates: &mut RoleTemplateAggregates,
    contribution: RoleTemplateContribution,
) {
    let aggregate = aggregates.aggregate_mut(contribution.scope, contribution.action);
    aggregate.action_keys.insert(contribution.action_key);
    if let Some(rule) = contribution.rule {
        aggregate.rules.insert(rule);
    }
}

pub(crate) fn view_verbs() -> Vec<String> {
    ["get", "list", "watch"]
        .into_iter()
        .map(str::to_string)
        .collect()
}

pub(crate) fn manage_verbs() -> Vec<String> {
    vec!["*".to_string()]
}

pub(crate) const fn crd_role_scope(placement: MenuPlacement, crd_scope: CrdScope) -> RoleScope {
    match placement {
        MenuPlacement::Cluster => RoleScope::Cluster,
        MenuPlacement::Workspace => RoleScope::Namespace,
        MenuPlacement::Global => match crd_scope {
            CrdScope::Cluster => RoleScope::Cluster,
            CrdScope::Namespaced => RoleScope::Namespace,
        },
    }
}

pub(crate) const fn iframe_role_scope(placement: MenuPlacement) -> Option<RoleScope> {
    match placement {
        MenuPlacement::Cluster => Some(RoleScope::Cluster),
        MenuPlacement::Workspace => Some(RoleScope::Workspace),
        MenuPlacement::Global => None,
    }
}

/// Field manager used by the category installer Job's server-side apply.
pub(crate) const CATEGORY_FIELD_MANAGER: &str = "frontend-forge-category";

/// Kubernetes caps Job names (and their `job-name` pod label) at 63 chars.
const MAX_JOB_NAME_LEN: usize = 63;

pub(crate) fn category_manifest(
    scope: RoleScope,
    category_name: &str,
    display_name_en: &str,
    display_name_zh: &str,
) -> String {
    let scope_name = role_scope_name(scope);
    format!(
        r#"---
apiVersion: iam.kubesphere.io/v1beta1
kind: Category
metadata:
  name: {category_name}
  labels:
    iam.kubesphere.io/scope: {scope_name}
    kubesphere.io/managed: "true"
spec:
  displayName:
    en: {display_name_en}
    zh: {display_name_zh}
"#
    )
}

/// Render the hook resources that create the shared role Categories.
///
/// Categories are shared by every generated extension on a cluster, so they
/// must not be Helm release resources: when several agent releases install
/// concurrently, every release renders before the Category exists and all but
/// the first fail Helm's ownership check. Instead, a pre-install/pre-upgrade
/// hook Job server-side applies them. Server-side apply is an idempotent
/// create-or-update, so concurrent Jobs from different releases converge on
/// the same object, and hook resources are never adopted by a release. The
/// `lookup` guard skips the Job once every Category exists.
pub(crate) fn category_installer_template(package_name: &str, scopes: &[RoleScope]) -> String {
    if scopes.is_empty() {
        return String::new();
    }

    let name = category_installer_name(package_name);
    let mut out = String::from("{{- $missingCategory := false }}\n");
    let mut manifests = String::new();
    for scope in scopes {
        let category_name = role_category_name(*scope);
        out.push_str(&format!(
            "{{{{- if not (lookup \"iam.kubesphere.io/v1beta1\" \"Category\" \"\" \
             \"{category_name}\") }}}}\n{{{{- $missingCategory = true }}}}\n{{{{- end }}}}\n"
        ));
        manifests.push_str(&category_manifest(
            *scope,
            &category_name,
            "Quick Integration",
            "快速集成",
        ));
    }
    let manifests = indent_lines(&manifests, 14);
    let field_manager = CATEGORY_FIELD_MANAGER;

    out.push_str(&format!(
        r#"{{{{- if $missingCategory }}}}
{{{{- $global := .Values.global | default dict }}}}
{{{{- $image := .Values.roleTemplate.categoryInstaller.image }}}}
{{{{- $registry := $image.registry | default (dig "imageRegistry" "" $global) | default "docker.io" }}}}
---
apiVersion: v1
kind: ServiceAccount
metadata:
  name: {name}
  namespace: {{{{ .Release.Namespace }}}}
  annotations:
    "helm.sh/hook": pre-install,pre-upgrade
    "helm.sh/hook-weight": "-10"
    "helm.sh/hook-delete-policy": before-hook-creation,hook-succeeded
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: {name}
  annotations:
    "helm.sh/hook": pre-install,pre-upgrade
    "helm.sh/hook-weight": "-10"
    "helm.sh/hook-delete-policy": before-hook-creation,hook-succeeded
rules:
  - apiGroups:
      - iam.kubesphere.io
    resources:
      - categories
    verbs:
      - get
      - create
      - patch
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: {name}
  annotations:
    "helm.sh/hook": pre-install,pre-upgrade
    "helm.sh/hook-weight": "-10"
    "helm.sh/hook-delete-policy": before-hook-creation,hook-succeeded
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: {name}
subjects:
  - kind: ServiceAccount
    name: {name}
    namespace: {{{{ .Release.Namespace }}}}
---
apiVersion: batch/v1
kind: Job
metadata:
  name: {name}
  namespace: {{{{ .Release.Namespace }}}}
  annotations:
    "helm.sh/hook": pre-install,pre-upgrade
    "helm.sh/hook-weight": "-5"
    "helm.sh/hook-delete-policy": before-hook-creation,hook-succeeded
spec:
  backoffLimit: 3
  template:
    spec:
      restartPolicy: Never
      serviceAccountName: {name}
      {{{{- with (dig "imagePullSecrets" (list) $global) }}}}
      imagePullSecrets:
        {{{{- toYaml . | nindent 8 }}}}
      {{{{- end }}}}
      containers:
        - name: apply-categories
          image: {{{{ printf "%s/%s:%s" $registry $image.repository $image.tag | quote }}}}
          imagePullPolicy: {{{{ $image.pullPolicy }}}}
          command:
            - /bin/bash
            - -c
            - |
              kubectl apply --server-side --force-conflicts --field-manager={field_manager} -f - <<'EOF'
{manifests}              EOF
{{{{- end }}}}
"#
    ));
    out
}

pub(crate) fn category_installer_name(package_name: &str) -> String {
    const SUFFIX: &str = "-fe-category";
    let prefix = package_name
        .chars()
        .take(MAX_JOB_NAME_LEN - SUFFIX.len())
        .collect::<String>();
    format!("{}{SUFFIX}", prefix.trim_end_matches('-'))
}

fn indent_lines(text: &str, width: usize) -> String {
    let padding = " ".repeat(width);
    text.lines().fold(String::new(), |mut out, line| {
        if !line.is_empty() {
            out.push_str(&padding);
            out.push_str(line);
        }
        out.push('\n');
        out
    })
}

#[allow(clippy::format_push_string)]
pub(crate) fn append_role_template(
    out: &mut String,
    package_name: &str,
    scope: RoleScope,
    action: RoleAction,
    aggregate: &RoleTemplateAggregate,
) -> Result<(), ExtensionPackageError> {
    if aggregate.is_empty() {
        return Ok(());
    }

    let scope_name = role_scope_name(scope);
    let action_name = role_action_name(action);
    let role_name = format!("{scope_name}-{action_name}-{package_name}");
    let category = role_category_name(scope);
    let annotation = role_action_annotation(&aggregate.action_keys, action)?;
    let dependency = if action == RoleAction::Manage {
        Some(format!("{scope_name}-view-{package_name}"))
    } else {
        None
    };

    out.push_str("---\n");
    out.push_str("apiVersion: iam.kubesphere.io/v1beta1\n");
    out.push_str("kind: RoleTemplate\n");
    out.push_str("metadata:\n");
    out.push_str("  annotations:\n");
    if let Some(dependency) = dependency {
        out.push_str(&format!(
            "    iam.kubesphere.io/dependencies: '[\"{dependency}\"]'\n"
        ));
    }
    out.push_str(&format!(
        "    iam.kubesphere.io/role-template-rules: '{}'\n",
        annotation.replace('\'', "''")
    ));
    out.push_str("  labels:\n");
    append_role_labels(out, scope, action, &category);
    out.push_str(&format!("  name: {role_name}\n"));
    out.push_str("spec:\n");
    append_role_description(out, package_name, scope, action);
    append_role_display_name(out, package_name, action);
    append_rules(out, &aggregate.rules);
    Ok(())
}

pub(crate) fn role_action_annotation(
    action_keys: &BTreeSet<String>,
    action: RoleAction,
) -> Result<String, ExtensionPackageError> {
    let values = action_keys
        .iter()
        .map(|key| (key.clone(), role_action_name(action).to_string()))
        .collect::<BTreeMap<_, _>>();
    serde_json::to_string(&values).context(SerializeJsonSnafu {
        name: "role-template-rules annotation",
    })
}

#[allow(clippy::format_push_string)]
pub(crate) fn append_role_labels(
    out: &mut String,
    scope: RoleScope,
    action: RoleAction,
    category: &str,
) {
    match (scope, action) {
        (RoleScope::Cluster, RoleAction::View) => {
            out.push_str("    iam.kubesphere.io/aggregate-to-cluster-viewer: \"\"\n");
        }
        (RoleScope::Namespace, RoleAction::View) => {
            out.push_str("    iam.kubesphere.io/aggregate-to-viewer: \"\"\n");
            out.push_str("    iam.kubesphere.io/aggregate-to-operator: \"\"\n");
        }
        (RoleScope::Namespace, RoleAction::Manage) => {
            out.push_str("    iam.kubesphere.io/aggregate-to-operator: \"\"\n");
        }
        (RoleScope::Workspace, RoleAction::View) => {
            out.push_str("    iam.kubesphere.io/aggregate-to-self-provisioner: \"\"\n");
            out.push_str("    iam.kubesphere.io/aggregate-to-viewer: \"\"\n");
            out.push_str("    iam.kubesphere.io/aggregate-to-regular: \"\"\n");
        }
        (RoleScope::Cluster | RoleScope::Workspace, RoleAction::Manage) => {}
    }
    out.push_str(&format!("    iam.kubesphere.io/category: {category}\n"));
    out.push_str(&format!(
        "    iam.kubesphere.io/scope: {}\n",
        role_scope_name(scope)
    ));
    out.push_str("    kubesphere.io/managed: \"true\"\n");
}

#[allow(clippy::format_push_string)]
pub(crate) fn append_role_description(
    out: &mut String,
    package_name: &str,
    scope: RoleScope,
    action: RoleAction,
) {
    out.push_str("  description:\n");
    match (scope, action) {
        (_, RoleAction::View) => {
            out.push_str(&format!("    en: View {package_name} list.\n"));
            out.push_str(&format!("    zh: 查看 {package_name} 列表。\n"));
        }
        (RoleScope::Cluster, RoleAction::Manage) => {
            out.push_str(&format!("    en: Manage {package_name}.\n"));
            out.push_str(&format!("    zh: 管理 {package_name}。\n"));
        }
        (RoleScope::Workspace, RoleAction::Manage) => {
            out.push_str(&format!("    en: Workspace {package_name} management.\n"));
            out.push_str(&format!("    zh: 企业空间 {package_name} 管理。\n"));
        }
        (RoleScope::Namespace, RoleAction::Manage) => {
            out.push_str(&format!("    en: Namespace {package_name} management.\n"));
            out.push_str(&format!("    zh: 项目 {package_name} 管理。\n"));
        }
    }
}

#[allow(clippy::format_push_string)]
pub(crate) fn append_role_display_name(out: &mut String, package_name: &str, action: RoleAction) {
    out.push_str("  displayName:\n");
    match action {
        RoleAction::View => {
            out.push_str(&format!("    en: View {package_name} List\n"));
            out.push_str(&format!("    zh: 查看 {package_name} 列表\n"));
        }
        RoleAction::Manage => {
            out.push_str(&format!("    en: Manage {package_name}\n"));
            out.push_str(&format!("    zh: 管理 {package_name}\n"));
        }
    }
}

#[allow(clippy::format_push_string)]
pub(crate) fn append_rules(out: &mut String, rules: &BTreeSet<GeneratedRule>) {
    if rules.is_empty() {
        out.push_str("  rules: []\n");
        return;
    }

    out.push_str("  rules:\n");
    for rule in rules {
        out.push_str("  - apiGroups:\n");
        out.push_str(&format!("    - '{}'\n", rule.api_group.replace('\'', "''")));
        out.push_str("    resources:\n");
        out.push_str(&format!("    - '{}'\n", rule.resource.replace('\'', "''")));
        out.push_str("    verbs:\n");
        for verb in &rule.verbs {
            out.push_str(&format!("    - '{}'\n", verb.replace('\'', "''")));
        }
    }
}

pub(crate) const fn role_scope_name(scope: RoleScope) -> &'static str {
    match scope {
        RoleScope::Cluster => "cluster",
        RoleScope::Workspace => "workspace",
        RoleScope::Namespace => "namespace",
    }
}

pub(crate) fn role_category_name(scope: RoleScope) -> String {
    format!("{}-fe-management", role_scope_name(scope))
}

pub(crate) const fn role_action_name(action: RoleAction) -> &'static str {
    match action {
        RoleAction::View => "view",
        RoleAction::Manage => "manage",
    }
}
