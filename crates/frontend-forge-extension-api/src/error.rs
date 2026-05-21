use super::*;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub(crate) enum Error {
    #[snafu(display("failed to initialize Kubernetes client: {source}"))]
    KubeClientInit { source: kube::Error },
    #[snafu(display("invalid EXTENSION_API_BIND_ADDR '{value}': {source}"))]
    InvalidBindAddr {
        value: String,
        source: AddrParseError,
    },
    #[snafu(display("extension API server failed on {bind_addr}: {source}"))]
    Server {
        bind_addr: SocketAddr,
        source: std::io::Error,
    },
}

#[derive(Debug)]
pub(crate) struct ApiError {
    pub(crate) status: StatusCode,
    pub(crate) message: String,
}

impl ApiError {
    pub(crate) fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    pub(crate) fn kube(action: &str, source: &kube::Error) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("{action}: {source}"),
        )
    }

    pub(crate) fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, message)
    }

    pub(crate) fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let reason = status_reason(self.status);
        let status = k8s_openapi::apimachinery::pkg::apis::meta::v1::Status {
            code: Some(i32::from(self.status.as_u16())),
            message: Some(self.message),
            reason: Some(reason.to_string()),
            status: Some("Failure".to_string()),
            ..Default::default()
        };

        (self.status, Json(status)).into_response()
    }
}

fn status_reason(status: StatusCode) -> &'static str {
    match status {
        StatusCode::BAD_REQUEST => "BadRequest",
        StatusCode::UNAUTHORIZED => "Unauthorized",
        StatusCode::FORBIDDEN => "Forbidden",
        StatusCode::NOT_FOUND => "NotFound",
        StatusCode::METHOD_NOT_ALLOWED => "MethodNotAllowed",
        StatusCode::CONFLICT => "Conflict",
        StatusCode::GONE => "Gone",
        StatusCode::UNPROCESSABLE_ENTITY => "Invalid",
        StatusCode::TOO_MANY_REQUESTS => "TooManyRequests",
        StatusCode::INTERNAL_SERVER_ERROR => "InternalError",
        StatusCode::SERVICE_UNAVAILABLE => "ServiceUnavailable",
        StatusCode::GATEWAY_TIMEOUT => "Timeout",
        _ if status.is_client_error() => "BadRequest",
        _ if status.is_server_error() => "InternalError",
        _ => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn api_error_responds_with_kubernetes_status() {
        let response = ApiError::conflict(
            "publish expectedArtifactDigest cannot be checked until the artifact is ready",
        )
        .into_response();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(value["apiVersion"], "v1");
        assert_eq!(value["kind"], "Status");
        assert_eq!(value["status"], "Failure");
        assert_eq!(value["reason"], "Conflict");
        assert_eq!(value["code"], 409);
        assert_eq!(
            value["message"],
            "publish expectedArtifactDigest cannot be checked until the artifact is ready"
        );
        assert!(value.get("error").is_none());
    }

    #[test]
    fn status_reason_maps_common_kubernetes_reasons() {
        assert_eq!(status_reason(StatusCode::NOT_FOUND), "NotFound");
        assert_eq!(status_reason(StatusCode::INTERNAL_SERVER_ERROR), "InternalError");
        assert_eq!(status_reason(StatusCode::UNPROCESSABLE_ENTITY), "Invalid");
    }
}
