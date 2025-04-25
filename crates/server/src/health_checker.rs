use crate::tls::build_upstream_certificate;
use reqwest::{ClientBuilder, Url};
use unleash_edge_types::cli::HealthCheckArgs;
use unleash_edge_types::error::EdgeError;

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
    use crate::health_checker::check_health;
    use crate::internal_backstage::health;
    use actix_http::HttpService;
    use actix_http_test::test_server;
    use actix_service::map_config;
    use actix_web::dev::AppConfig;
    use actix_web::{App, HttpResponse, web};
    use unleash_edge_types::cli::HealthCheckArgs;

    #[tokio::test]
    pub async fn runs_health_check() {
        let srv = test_server(move || {
            HttpService::new(map_config(
                App::new().service(web::scope("/internal-backstage").service(health)),
                |_| AppConfig::default(),
            ))
            .tcp()
        })
        .await;
        let url = srv.url("/");
        let check_result = check_health(HealthCheckArgs {
            ca_certificate_file: None,
            edge_url: url,
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

    async fn conflict() -> HttpResponse {
        HttpResponse::Conflict().finish()
    }

    #[tokio::test]
    pub async fn errors_if_health_check_returns_different_status_than_200() {
        let srv = test_server(move || {
            HttpService::new(map_config(
                App::new().service(
                    web::scope("/internal-backstage").route("/health", web::get().to(conflict)),
                ),
                |_| AppConfig::default(),
            ))
            .tcp()
        })
        .await;
        let url = srv.url("/");
        let check_result = check_health(HealthCheckArgs {
            ca_certificate_file: None,
            edge_url: url,
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
