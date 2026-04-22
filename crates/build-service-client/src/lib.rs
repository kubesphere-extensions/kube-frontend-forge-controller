use std::{path::Path, time::Duration};

use serde::Deserialize;
use snafu::{ResultExt, Snafu};

#[derive(Debug, Snafu)]
pub enum BuildServiceError {
    #[snafu(display(
        "failed to initialize build-service HTTP client (timeout={timeout_seconds}s): {source}"
    ))]
    ClientInit {
        timeout_seconds: u64,
        source: reqwest::Error,
    },
    #[snafu(display("build-service request failed during {operation} {url}: {source}"))]
    Request {
        operation: &'static str,
        url: String,
        source: reqwest::Error,
    },
    #[snafu(display("build-service returned non-success during {operation} {url}: {source}"))]
    ResponseStatus {
        operation: &'static str,
        url: String,
        source: reqwest::Error,
    },
    #[snafu(display("failed to decode build-service response during {operation} {url}: {source}"))]
    Decode {
        operation: &'static str,
        url: String,
        source: reqwest::Error,
    },
    #[snafu(display("build-service returned failure: {message}"))]
    BuildFailed { message: String },
    #[snafu(display("no suitable JS bundle artifact found (wanted key '{desired_key}')"))]
    MissingBundleArtifact { desired_key: String },
}

#[derive(Clone)]
pub struct BuildServiceClient {
    base_url: String,
    client: reqwest::Client,
}

#[derive(Debug, Deserialize)]
struct ProjectBuildResponse {
    ok: bool,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    files: Vec<RemoteFile>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct RemoteFile {
    pub path: String,
    pub content: String,
}

impl BuildServiceClient {
    /// Create a client for the frontend-forge build-service.
    ///
    /// # Errors
    ///
    /// Returns an error when the underlying HTTP client cannot be built.
    pub fn new(base_url: &str, timeout_seconds: u64) -> Result<Self, BuildServiceError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_seconds))
            .build()
            .context(ClientInitSnafu { timeout_seconds })?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client,
        })
    }

    /// Build a rendered frontend manifest into bundle files.
    ///
    /// # Errors
    ///
    /// Returns an error if the build-service request fails, returns a non-2xx
    /// response, cannot be decoded, or reports `ok: false`.
    pub async fn build_project(
        &self,
        manifest: &str,
    ) -> Result<Vec<RemoteFile>, BuildServiceError> {
        let url = format!("{}/api/project/build", self.base_url);
        let resp = self
            .client
            .post(&url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(manifest.to_string())
            .send()
            .await
            .context(RequestSnafu {
                operation: "project_build",
                url: url.clone(),
            })?;
        let resp = resp.error_for_status().context(ResponseStatusSnafu {
            operation: "project_build",
            url: url.clone(),
        })?;
        let payload: ProjectBuildResponse = resp.json().await.context(DecodeSnafu {
            operation: "project_build",
            url,
        })?;
        if !payload.ok {
            return Err(BuildServiceError::BuildFailed {
                message: payload
                    .message
                    .unwrap_or_else(|| "build-service returned ok=false".to_string()),
            });
        }
        Ok(payload.files)
    }
}

/// Select the JS bundle artifact using the same priority as the FI runner.
///
/// # Errors
///
/// Returns an error when no file matches the desired key and no JS fallback can
/// be selected.
pub fn select_bundle_artifact(
    desired_key: &str,
    remote_files: Vec<RemoteFile>,
) -> Result<(String, String), BuildServiceError> {
    let selected_idx = remote_files
        .iter()
        .position(|f| f.path == desired_key)
        .or_else(|| {
            if remote_files.len() == 1 {
                Some(0)
            } else {
                remote_files.iter().position(|f| {
                    Path::new(&f.path)
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("js"))
                })
            }
        })
        .ok_or_else(|| BuildServiceError::MissingBundleArtifact {
            desired_key: desired_key.to_string(),
        })?;

    let file = remote_files.into_iter().nth(selected_idx).ok_or_else(|| {
        BuildServiceError::MissingBundleArtifact {
            desired_key: desired_key.to_string(),
        }
    })?;
    let key = if file.path.contains('/') {
        desired_key.to_string()
    } else {
        file.path
    };
    Ok((key, file.content))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_exact_desired_key() {
        let (key, content) = select_bundle_artifact(
            "index.js",
            vec![
                RemoteFile {
                    path: "other.js".to_string(),
                    content: "other".to_string(),
                },
                RemoteFile {
                    path: "index.js".to_string(),
                    content: "index".to_string(),
                },
            ],
        )
        .unwrap();

        assert_eq!(key, "index.js");
        assert_eq!(content, "index");
    }

    #[test]
    fn selects_single_file_passthrough() {
        let (key, content) = select_bundle_artifact(
            "index.js",
            vec![RemoteFile {
                path: "main.css".to_string(),
                content: "body{}".to_string(),
            }],
        )
        .unwrap();

        assert_eq!(key, "main.css");
        assert_eq!(content, "body{}");
    }

    #[test]
    fn selects_js_fallback_as_desired_key_when_nested() {
        let (key, content) = select_bundle_artifact(
            "index.js",
            vec![
                RemoteFile {
                    path: "style.css".to_string(),
                    content: "body{}".to_string(),
                },
                RemoteFile {
                    path: "bundle/main.js".to_string(),
                    content: "console.log('js')".to_string(),
                },
            ],
        )
        .unwrap();

        assert_eq!(key, "index.js");
        assert_eq!(content, "console.log('js')");
    }
}
