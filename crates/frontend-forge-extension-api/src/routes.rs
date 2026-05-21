use super::*;

pub(crate) fn api_routes(prefix: &str, group: &str, version: &str) -> Router<Arc<AppState>> {
    let resource_prefix = format!("{prefix}/{group}/{version}/{API_RESOURCE}");
    Router::new()
        .route(
            &resource_prefix,
            get(list_extensions).post(create_extension),
        )
        .route(&format!("{resource_prefix}/{{name}}"), get(get_extension))
        .route(
            &format!("{resource_prefix}/{{name}}/download"),
            get(download_extension),
        )
        .route(
            &format!("{resource_prefix}/{{name}}/publish"),
            get(get_publish_status).post(trigger_publish),
        )
        .route(
            &format!("{resource_prefix}/{{name}}/unpublish"),
            get(get_unpublish_status).post(trigger_unpublish),
        )
        .route(
            &format!("{resource_prefix}/{{name}}/delete"),
            post(delete_extension),
        )
}
