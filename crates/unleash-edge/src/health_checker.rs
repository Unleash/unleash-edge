use crate::cli::HealthCheckArgs;
use crate::error::EdgeError;
use crate::tls::build_upstream_certificate;
use reqwest::{ClientBuilder, Url};
use unleash_edge_cli::HealthCheckArgs;
use unleash_edge_types::errors::EdgeError;

fn build_health_url(url: &Url) -> Url {
    let mut with_path = url.clone();
    with_path
        .path_segments_mut()
        .expect("Could not build health check url")
        .push("internal-backstage")
        .push("health");
    with_path
}

pub async fn check_health(health_check_args: HealthCheckArgs) -> Result<(), EdgeError> {
    let client = match build_upstream_certificate(health_check_args.ca_certificate_file)? {
        Some(cert) => ClientBuilder::new()
            .add_root_certificate(cert)
            .build()
            .expect("Failed to build health check client"),
        None => reqwest::Client::default(),
    };
    let base_url = Url::parse(&health_check_args.edge_url)
        .map_err(|p| EdgeError::HealthCheckError(format!("Invalid health check url: {p:?}")))?;
    let health_check_url = build_health_url(&base_url);
    client
        .get(health_check_url)
        .send()
        .await
        .map_err(|e| EdgeError::HealthCheckError(format!("{e:?}")))
        .map(|r| {
            if r.status() == 200 {
                Ok(())
            } else {
                Err(EdgeError::HealthCheckError(
                    "Healthcheck had different status than 200".into(),
                ))
            }
        })?
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Response, StatusCode};
    use axum::response::IntoResponse;
    use axum::Router;
    use axum::routing::get;
    use axum_test::TestServer;
    use unleash_edge_cli::HealthCheckArgs;
    use crate::health_checker::check_health;

    #[tokio::test]
    pub async fn runs_health_check() {
        let router = Router::new().route("/internal-backstage", get(unleash_edge_backstage::health));
        let srv = TestServer::builder().http_transport().build(router).unwrap();
        let url = srv.server_url("/").unwrap();
        let check_result = check_health(HealthCheckArgs {
            ca_certificate_file: None,
            edge_url: url.to_string(),
        })
            .await;
        assert!(check_result.is_ok());
    }

    #[tokio::test]
    pub async fn errors_if_health_check_fails() {
        let check_result = check_health(HealthCheckArgs {
            ca_certificate_file: None,
            edge_url: "http://bogusurl".into(),
        })
            .await;
        assert!(check_result.is_err());
    }

    async fn conflict() -> impl IntoResponse {
        Response::builder().status(StatusCode::CONFLICT).body(Body::empty())
    }

    #[tokio::test]
    pub async fn errors_if_health_check_returns_different_status_than_200() {
        let router = Router::new().route("/health", get(conflict));
        let srv = TestServer::builder().http_transport().build(router).unwrap();
        let url = srv.server_url("/").unwrap();
        let check_result = check_health(HealthCheckArgs {
            ca_certificate_file: None,
            edge_url: url.to_string(),
        })
            .await;
        assert!(check_result.is_err());
    }

    #[tokio::test]
    pub async fn fails_if_given_an_invalid_url() {
        let check_result = check_health(HealthCheckArgs {
            ca_certificate_file: None,
            edge_url: ":\\///\\/".into(),
        })
            .await;
        assert!(check_result.is_err());
    }
}
