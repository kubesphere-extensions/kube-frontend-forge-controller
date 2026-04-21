pub mod fe;
pub mod fi;

pub use fe::*;
pub use fi::*;

use kube::CustomResourceExt;
use std::collections::BTreeMap;

pub const API_GROUP: &str = "frontend-forge.kubesphere.io";
pub const API_VERSION: &str = "v1alpha1";
pub const JSBUNDLE_PLURAL: &str = "jsbundles";
pub const JSBUNDLE_API_GROUP: &str = "extensions.kubesphere.io";
pub const JSBUNDLE_API_VERSION: &str = "v1alpha1";
pub const RESOURCE_SERVED_LABEL_KEY: &str = "kubesphere.io/resource-served";
pub const RESOURCE_SERVED_LABEL_VALUE: &str = "true";

pub fn frontend_integration_crd()
-> k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition {
    let mut crd = FrontendIntegration::crd();
    mark_resource_served(&mut crd);
    crd
}

pub fn frontend_extension_crd()
-> k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition {
    let mut crd = FrontendExtension::crd();
    mark_resource_served(&mut crd);
    crd
}

fn mark_resource_served(
    crd: &mut k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition,
) {
    crd.metadata
        .labels
        .get_or_insert_with(BTreeMap::new)
        .insert(
            RESOURCE_SERVED_LABEL_KEY.to_string(),
            RESOURCE_SERVED_LABEL_VALUE.to_string(),
        );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_crds_set_resource_served_label() {
        let crd = frontend_integration_crd();

        assert_eq!(
            crd.metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get(RESOURCE_SERVED_LABEL_KEY)),
            Some(&RESOURCE_SERVED_LABEL_VALUE.to_string())
        );

        let crd = frontend_extension_crd();

        assert_eq!(
            crd.metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get(RESOURCE_SERVED_LABEL_KEY)),
            Some(&RESOURCE_SERVED_LABEL_VALUE.to_string())
        );
    }
}
