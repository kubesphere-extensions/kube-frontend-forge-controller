mod v1;

use frontend_forge_api::{
    FrontendExtension, FrontendExtensionSourceType, FrontendIntegration, PageSpec, PrimaryMenuSpec,
};
use kube::ResourceExt;
use serde_json::Value;
use snafu::Snafu;
use std::collections::BTreeMap;

#[derive(Debug, Snafu)]
pub enum ManifestRenderError {
    #[snafu(display(
        "FrontendIntegration {} has duplicate top-level menu key '{}'",
        fi_name,
        key
    ))]
    DuplicateTopLevelMenuKey { fi_name: String, key: String },
    #[snafu(display("FrontendIntegration {} has duplicate page key '{}'", fi_name, key))]
    DuplicatePageKey { fi_name: String, key: String },
    #[snafu(display(
        "FrontendIntegration {} is missing page config for menu key '{}'",
        fi_name,
        key
    ))]
    MissingPageForMenuKey { fi_name: String, key: String },
    #[snafu(display(
        "FrontendIntegration {} has page config '{}' without a menu binding",
        fi_name,
        key
    ))]
    OrphanPageConfig { fi_name: String, key: String },
    #[snafu(display(
        "FrontendIntegration {} has invalid menu shape for key '{}': {}",
        fi_name,
        key,
        message
    ))]
    InvalidMenuShape {
        fi_name: String,
        key: String,
        message: String,
    },
    #[snafu(display(
        "FrontendIntegration {} has invalid page shape for key '{}': {}",
        fi_name,
        key,
        message
    ))]
    InvalidPageShape {
        fi_name: String,
        key: String,
        message: String,
    },
    #[snafu(display("FrontendIntegration {} has invalid menu key '{}'", fi_name, key))]
    InvalidMenuKey { fi_name: String, key: String },
    #[snafu(display(
        "FrontendIntegration {} requires columns for CRD page '{}'",
        fi_name,
        key
    ))]
    MissingCrdColumns { fi_name: String, key: String },
    #[snafu(display(
        "FrontendIntegration {} requested unsupported builder.engineVersion '{}'",
        fi_name,
        engine_version
    ))]
    UnsupportedEngineVersion {
        fi_name: String,
        engine_version: String,
    },
    #[snafu(display(
        "FrontendExtension {} requested unsupported source.schemaVersion '{}'",
        fe_name,
        schema_version
    ))]
    UnsupportedSchemaVersion {
        fe_name: String,
        schema_version: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct FrontendRenderInput {
    pub name: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub schema_version: Option<String>,
    pub route_namespace: String,
    pub locales: BTreeMap<String, BTreeMap<String, String>>,
    pub menus: Vec<PrimaryMenuSpec>,
    pub pages: Vec<PageSpec>,
}

impl FrontendRenderInput {
    pub fn from_frontend_integration(fi: &FrontendIntegration) -> Self {
        Self {
            name: fi.name_any(),
            display_name: fi.spec.display_name.clone(),
            description: fi
                .metadata
                .annotations
                .as_ref()
                .and_then(|a| a.get("kubesphere.io/description").cloned()),
            schema_version: fi.spec.engine_version().map(ToString::to_string),
            route_namespace: "frontendintegrations".to_string(),
            locales: fi.spec.locales.clone(),
            menus: fi.spec.menus.clone(),
            pages: fi.spec.pages.clone(),
        }
    }

    pub fn from_frontend_extension(fe: &FrontendExtension) -> Result<Self, ManifestRenderError> {
        match fe.spec.source.type_ {
            FrontendExtensionSourceType::Inline => {
                let inline = &fe.spec.source.inline;

                Ok(Self {
                    name: frontend_extension_package_name(fe),
                    display_name: inline
                        .frontend
                        .display_name
                        .clone()
                        .or_else(|| localized_text(&fe.spec.package.display_name)),
                    description: localized_text(&fe.spec.package.description),
                    schema_version: Some(inline.schema_version.clone()),
                    route_namespace: "frontendextensions".to_string(),
                    locales: inline.frontend.locales.clone(),
                    menus: inline.frontend.menus.clone(),
                    pages: inline.frontend.pages.clone(),
                })
            }
        }
    }
}

fn localized_text(values: &BTreeMap<String, String>) -> Option<String> {
    values
        .get("en")
        .or_else(|| values.get("zh"))
        .or_else(|| values.values().next())
        .cloned()
}

fn frontend_extension_package_name(fe: &FrontendExtension) -> String {
    fe.spec
        .package
        .name
        .clone()
        .unwrap_or_else(|| fe.name_any())
}

// Rendering remains versioned so runner and webhook share the same validation semantics.
pub fn render_extension_manifest(fi: &FrontendIntegration) -> Result<Value, ManifestRenderError> {
    let input = FrontendRenderInput::from_frontend_integration(fi);
    let requested = fi.spec.engine_version().unwrap_or("v1").trim();
    let normalized = if requested.is_empty() {
        "v1"
    } else {
        requested
    }
    .to_ascii_lowercase();

    match normalized.as_str() {
        "v1" | "v1alpha1" | "1" | "1.0" => v1::render_v1_manifest(&input),
        _ => Err(ManifestRenderError::UnsupportedEngineVersion {
            fi_name: fi.name_any(),
            engine_version: requested.to_string(),
        }),
    }
}

pub fn render_frontend_extension_manifest(
    fe: &FrontendExtension,
) -> Result<Value, ManifestRenderError> {
    let input = FrontendRenderInput::from_frontend_extension(fe)?;
    let requested = input.schema_version.as_deref().unwrap_or("v1").trim();
    let normalized = if requested.is_empty() {
        "v1"
    } else {
        requested
    }
    .to_ascii_lowercase();

    match normalized.as_str() {
        "v1" | "v1alpha1" | "1" | "1.0" => v1::render_v1_manifest(&input),
        _ => Err(ManifestRenderError::UnsupportedSchemaVersion {
            fe_name: fe.name_any(),
            schema_version: requested.to_string(),
        }),
    }
}

pub fn validate_frontend_integration(fi: &FrontendIntegration) -> Result<(), ManifestRenderError> {
    render_extension_manifest(fi).map(|_| ())
}

pub fn validate_frontend_extension(fe: &FrontendExtension) -> Result<(), ManifestRenderError> {
    render_frontend_extension_manifest(fe).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use frontend_forge_api::FrontendExtension;
    use serde_yaml;

    #[test]
    fn defaults_to_v1_renderer() {
        let fi: FrontendIntegration = serde_yaml::from_str(
            r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendIntegration
metadata:
  name: demo
spec:
  menus:
    - displayName: Demo
      key: demo
      placement: global
      type: page
  pages:
    - key: demo
      type: iframe
      iframe:
        src: http://example.test
"#,
        )
        .unwrap();

        let manifest = render_extension_manifest(&fi).unwrap();
        assert_eq!(manifest["version"], "1.0");
    }

    #[test]
    fn rejects_unknown_engine_version() {
        let fi: FrontendIntegration = serde_yaml::from_str(
            r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendIntegration
metadata:
  name: demo
spec:
  builder:
    engineVersion: v99
  menus:
    - displayName: Demo
      key: demo
      placement: global
      type: page
  pages:
    - key: demo
      type: iframe
      iframe:
        src: http://example.test
"#,
        )
        .unwrap();

        assert!(matches!(
            render_extension_manifest(&fi),
            Err(ManifestRenderError::UnsupportedEngineVersion { .. })
        ));
    }

    #[test]
    fn validate_frontend_integration_reuses_render_path() {
        let fi: FrontendIntegration = serde_yaml::from_str(
            r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendIntegration
metadata:
  name: demo
spec:
  menus:
    - displayName: Demo
      key: demo
      placement: global
      type: page
  pages:
    - key: demo
      type: iframe
      iframe:
        src: http://example.test
"#,
        )
        .unwrap();

        assert!(validate_frontend_integration(&fi).is_ok());
    }

    #[test]
    fn validate_frontend_integration_returns_domain_errors() {
        let fi: FrontendIntegration = serde_yaml::from_str(
            r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendIntegration
metadata:
  name: demo
spec:
  menus:
    - displayName: Demo
      key: demo
      placement: global
      type: page
  pages:
    - key: demo
      type: iframe
      iframe:
        src: http://example.test
    - key: demo
      type: iframe
      iframe:
        src: http://example.test/other
"#,
        )
        .unwrap();

        assert!(matches!(
            validate_frontend_integration(&fi),
            Err(ManifestRenderError::DuplicatePageKey { .. })
        ));
    }

    #[test]
    fn renders_frontend_extension_inline_source() {
        let fe: FrontendExtension = serde_yaml::from_str(
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
  source:
    type: Inline
    inline:
      schemaVersion: v1
      frontend:
        locales:
          en:
            title: Inspect Tasks
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
"#,
        )
        .unwrap();

        let manifest = render_frontend_extension_manifest(&fe).unwrap();

        assert_eq!(manifest["name"], "inspecttask");
        assert_eq!(manifest["displayName"], "Inspect Task");
        assert_eq!(manifest["description"], "InspectTask extension package");
        assert_eq!(manifest["version"], "1.0");
        assert_eq!(
            manifest["routes"][0]["path"],
            "/clusters/:cluster/frontendextensions/inspecttask/inspecttasks"
        );
        assert_eq!(
            manifest["menus"][0]["name"],
            "frontendextensions/inspecttask/inspecttasks"
        );
    }

    #[test]
    fn rejects_unknown_frontend_extension_schema_version() {
        let fe: FrontendExtension = serde_yaml::from_str(
            r#"
apiVersion: frontend-forge.kubesphere.io/v1alpha1
kind: FrontendExtension
metadata:
  name: inspecttask
spec:
  package:
    version: 0.1.0
    displayName:
      en: Inspect Task
    description:
      en: InspectTask extension package
  source:
    type: Inline
    inline:
      schemaVersion: v99
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
"#,
        )
        .unwrap();

        assert!(matches!(
            render_frontend_extension_manifest(&fe),
            Err(ManifestRenderError::UnsupportedSchemaVersion { .. })
        ));
    }
}
