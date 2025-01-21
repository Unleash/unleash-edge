use reqwest::{ClientBuilder, Url};

use crate::cli::ReadyCheckArgs;
use crate::error::EdgeError;
use crate::internal_backstage::EdgeStatus;
use crate::tls::build_upstream_certificate;
use crate::types::Status;

fn build_ready_url(url: &Url) -> Url {
    let mut with_path = url.clone();
    with_path
        .path_segments_mut()
        .expect("Could not build ready url")
        .push("internal-backstage")
        .push("ready");
    with_path
}

pub async fn check_ready(ready_check_args: ReadyCheckArgs) -> Result<(), EdgeError> {
    let client = match build_upstream_certificate(ready_check_args.ca_certificate_file)? {
        Some(cert) => ClientBuilder::new()
            .add_root_certificate(cert)
            .build()
            .expect("Failed to build ready check client"),
        None => reqwest::Client::default(),
    };
    let base_url = Url::parse(&ready_check_args.edge_url)
        .map_err(|p| EdgeError::ReadyCheckError(format!("Invalid ready check url: {p:?}")))?;
    let ready_check_url = build_ready_url(&base_url);
    let r = client
        .get(ready_check_url.clone())
        .send()
        .await
        .map_err(|e| {
            EdgeError::ReadyCheckError(format!(
                "Failed to connect to ready endpoint at {}. Failed with status {:?}",
                ready_check_url,
                e.status()
            ))
        })?;
    if r.status() == 200 {
        let ready_check_result: EdgeStatus = r.json().await.map_err(|e| {
            EdgeError::ReadyCheckError(format!(
                "Ready check endpoint returned data we didn't understand. {e:?}"
            ))
        })?;
        match ready_check_result.status {
            Status::Ready => Ok(()),
            _ => Err(EdgeError::ReadyCheckError(format!(
                "Ready check returned a different status than READY. It returned {:?}",
                ready_check_result
            ))),
        }
    } else {
        Err(EdgeError::ReadyCheckError(format!(
            "Ready check did not return 200 for {}. It returned {}",
            ready_check_url,
            r.status()
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use std::sync::Arc;

    use actix_http::HttpService;
    use actix_http_test::test_server;
    use actix_service::map_config;
    use actix_web::dev::AppConfig;
    use actix_web::{web, App, HttpResponse};
    use dashmap::DashMap;
    use unleash_types::client_features::{ClientFeature, ClientFeatures};

    use crate::cli::ReadyCheckArgs;
    use crate::feature_cache::FeatureCache;
    use crate::internal_backstage::ready;
    use crate::ready_checker::check_ready;
    use crate::types::EdgeToken;

    #[tokio::test]
    pub async fn runs_ready_check() {
        let features = ClientFeatures {
            features: vec![ClientFeature {
                name: "test".to_string(),
                ..ClientFeature::default()
            }],
            query: None,
            segments: None,
            version: 2,
            meta: None,
        };
        let client_features: FeatureCache = FeatureCache::default();
        client_features.insert(
            "testproject:testenvironment.testtoken".into(),
            features.clone(),
        );
        let client_features_arc = Arc::new(client_features);
        let token_cache: DashMap<String, EdgeToken> = DashMap::default();
        let token = EdgeToken::from_str("[]:fancyenvironment.somerandomsecretstring").unwrap();
        token_cache.insert(token.token.clone(), token);
        let token_cache_arc = Arc::new(token_cache);

        let srv = test_server(move || {
            HttpService::new(map_config(
                App::new()
                    .app_data(web::Data::from(client_features_arc.clone()))
                    .app_data(web::Data::from(token_cache_arc.clone()))
                    .service(web::scope("/internal-backstage").service(ready)),
                |_| AppConfig::default(),
            ))
            .tcp()
        })
        .await;
        let url = srv.url("/");
        let check_result = check_ready(ReadyCheckArgs {
            ca_certificate_file: None,
            edge_url: url,
        })
        .await;
        assert!(check_result.is_ok());
    }

    #[tokio::test]
    pub async fn errors_if_ready_check_fails() {
        let check_result = check_ready(ReadyCheckArgs {
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
    pub async fn errors_if_ready_check_returns_different_status_than_200() {
        let srv = test_server(move || {
            HttpService::new(map_config(
                App::new().service(
                    web::scope("/internal-backstage").route("/ready", web::get().to(conflict)),
                ),
                |_| AppConfig::default(),
            ))
            .tcp()
        })
        .await;
        let url = srv.url("/");
        let check_result = check_ready(ReadyCheckArgs {
            ca_certificate_file: None,
            edge_url: url,
        })
        .await;
        assert!(check_result.is_err());
    }

    #[tokio::test]
    pub async fn fails_if_given_an_invalid_url() {
        let check_result = check_ready(ReadyCheckArgs {
            ca_certificate_file: None,
            edge_url: ":\\///\\/".into(),
        })
        .await;
        assert!(check_result.is_err());
    }
}
