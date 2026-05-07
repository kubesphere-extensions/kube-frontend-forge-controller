use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum RoleScope {
    Cluster,
    Namespace,
}

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
    let mut out = String::from("{{- if .Values.roleTemplate.enabled }}\n");

    if aggregates.has_scope(RoleScope::Cluster) {
        out.push_str(&category_template(
            RoleScope::Cluster,
            "cluster-fe-management",
            "Quick Integration",
            "快速集成",
        ));
    }
    append_role_template(
        &mut out,
        package_name,
        RoleScope::Cluster,
        RoleAction::View,
        &aggregates.cluster_view,
    )?;
    append_role_template(
        &mut out,
        package_name,
        RoleScope::Cluster,
        RoleAction::Manage,
        &aggregates.cluster_manage,
    )?;

    if aggregates.has_scope(RoleScope::Namespace) {
        out.push_str(&category_template(
            RoleScope::Namespace,
            "namespace-fe-management",
            "Quick Integration",
            "快速集成",
        ));
    }
    append_role_template(
        &mut out,
        package_name,
        RoleScope::Namespace,
        RoleAction::View,
        &aggregates.namespace_view,
    )?;
    append_role_template(
        &mut out,
        package_name,
        RoleScope::Namespace,
        RoleAction::Manage,
        &aggregates.namespace_manage,
    )?;

    out.push_str("{{- end }}\n");
    Ok(out)
}

impl RoleTemplateAggregates {
    const fn aggregate_mut(
        &mut self,
        scope: RoleScope,
        action: RoleAction,
    ) -> &mut RoleTemplateAggregate {
        match (scope, action) {
            (RoleScope::Cluster, RoleAction::View) => &mut self.cluster_view,
            (RoleScope::Cluster, RoleAction::Manage) => &mut self.cluster_manage,
            (RoleScope::Namespace, RoleAction::View) => &mut self.namespace_view,
            (RoleScope::Namespace, RoleAction::Manage) => &mut self.namespace_manage,
        }
    }

    fn has_scope(&self, scope: RoleScope) -> bool {
        match scope {
            RoleScope::Cluster => !self.cluster_view.is_empty() || !self.cluster_manage.is_empty(),
            RoleScope::Namespace => {
                !self.namespace_view.is_empty() || !self.namespace_manage.is_empty()
            }
        }
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
        MenuPlacement::Workspace => Some(RoleScope::Namespace),
        MenuPlacement::Global => None,
    }
}

pub(crate) fn category_template(
    scope: RoleScope,
    category_name: &str,
    display_name_en: &str,
    display_name_zh: &str,
) -> String {
    let scope_name = role_scope_name(scope);
    format!(
        r#"---
{{{{- $existing := lookup "iam.kubesphere.io/v1beta1" "Category" "" "{category_name}" -}}}}

{{{{- if not $existing }}}}
apiVersion: iam.kubesphere.io/v1beta1
kind: Category
metadata:
  name: {category_name}
  annotations:
    "helm.sh/resource-policy": keep
  labels:
    iam.kubesphere.io/scope: {scope_name}
    kubesphere.io/managed: "true"
spec:
  displayName:
    en: {display_name_en}
    zh: {display_name_zh}
{{{{- end }}}}
"#
    )
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
    let category = format!("{scope_name}-fe-management");
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
        (RoleScope::Cluster, RoleAction::Manage) => {}
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
        RoleScope::Namespace => "namespace",
    }
}

pub(crate) const fn role_action_name(action: RoleAction) -> &'static str {
    match action {
        RoleAction::View => "view",
        RoleAction::Manage => "manage",
    }
}
