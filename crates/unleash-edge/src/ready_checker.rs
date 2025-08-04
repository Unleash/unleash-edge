use reqwest::{ClientBuilder, Url};
use unleash_edge_backstage::EdgeStatus;
use unleash_edge_cli::ReadyCheckArgs;
use unleash_edge_types::errors::EdgeError;
use unleash_edge_types::Status;
use crate::tls::build_upstream_certificate;

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
    use axum::body::Body;
    use axum::http::{Response, StatusCode};
    use axum::response::IntoResponse;
    use axum::Router;
    use axum::routing::get;
    use axum_test::TestServer;
    use dashmap::DashMap;
    use unleash_types::client_features::{ClientFeature, ClientFeatures};
    use unleash_edge_appstate::AppState;
    use unleash_edge_backstage::ready;
    use unleash_edge_cli::ReadyCheckArgs;
    use unleash_edge_feature_cache::FeatureCache;
    use unleash_edge_types::tokens::EdgeToken;
    use crate::ready_checker::check_ready;

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
        let app_state = AppState::builder().with_token_cache(token_cache_arc.clone()).with_features_cache(client_features_arc.clone()).build();

        let router = Router::new().route("/internal-backstage/ready", get(ready)).with_state(app_state);
        let srv = TestServer::builder().http_transport().build(router).unwrap();
        let url = srv.server_url("/").unwrap();
        let check_result = check_ready(ReadyCheckArgs {
            ca_certificate_file: None,
            edge_url: url.to_string(),
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

    async fn conflict() -> impl IntoResponse {
        Response::builder().status(StatusCode::CONFLICT).body(Body::empty()).unwrap()
    }

    #[tokio::test]
    pub async fn errors_if_ready_check_returns_different_status_than_200() {
        let router = Router::new().route("/internal-backstage/ready", get(conflict));
        let srv = TestServer::builder().http_transport().build(router).expect("Failed to build test server");
        let url = srv.server_url("/").expect("Failed to get server url");
        let check_result = check_ready(ReadyCheckArgs {
            ca_certificate_file: None,
            edge_url: url.to_string(),
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
