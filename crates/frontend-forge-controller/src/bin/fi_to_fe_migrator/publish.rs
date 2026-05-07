use super::*;

pub(crate) async fn publish_fe(
    http: &reqwest::Client,
    cfg: &MigratorConfig,
    fe_name: &str,
    artifact_digest: &str,
) -> Result<()> {
    let url = format!(
        "{}/apis/{}/{}/frontendextensions/{}/publish",
        cfg.fe_api_base_url, cfg.fe_api_group, cfg.fe_api_version, fe_name
    );
    let request_id = publish_request_id(fe_name, artifact_digest);
    let response = http
        .post(&url)
        .json(&json!({
            "requestId": request_id,
            "expectedArtifactDigest": artifact_digest,
        }))
        .send()
        .await
        .map_err(|source| Error::Http {
            action: format!("posting FE publish request to {url}"),
            source: Box::new(source),
        })?;
    let status = response.status();
    if matches!(
        status,
        StatusCode::OK | StatusCode::ACCEPTED | StatusCode::CREATED
    ) {
        info!(fe = %fe_name, %request_id, "publish request accepted");
        return Ok(());
    }
    let body = response.text().await.unwrap_or_default();
    Err(Error::Message {
        message: format!("FE publish request for {fe_name} failed with {status}: {body}"),
    })
}

pub(crate) fn publish_http_client(cfg: &MigratorConfig) -> Result<reqwest::Client> {
    let mut builder =
        reqwest::Client::builder().danger_accept_invalid_certs(cfg.fe_api_insecure_skip_tls_verify);
    if !cfg.fe_api_insecure_skip_tls_verify
        && let Some(path) = cfg.fe_api_ca_cert_path.as_ref()
    {
        match fs::read(path) {
            Ok(bytes) => {
                let cert =
                    reqwest::Certificate::from_pem(&bytes).map_err(|source| Error::Http {
                        action: format!("loading CA certificate {path}"),
                        source: Box::new(source),
                    })?;
                builder = builder.add_root_certificate(cert);
            }
            Err(source) => {
                return Err(Error::ReadFile {
                    path: path.clone(),
                    source,
                });
            }
        }
    }
    builder.build().map_err(|source| Error::Http {
        action: "building HTTP client".to_string(),
        source: Box::new(source),
    })
}
pub(crate) fn publish_request_id(fe_name: &str, artifact_digest: &str) -> String {
    let digest = artifact_digest
        .strip_prefix("sha256:")
        .unwrap_or(artifact_digest)
        .chars()
        .take(12)
        .collect::<String>();
    format!("fi-migration-{fe_name}-{digest}")
}
