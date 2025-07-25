use actix_web::{
    body::MessageBody,
    dev::{ServiceRequest, ServiceResponse},
    web::Data,
};
use dashmap::DashMap;
use tracing::debug;

use crate::{
    http::refresher::feature_refresher::FeatureRefresher,
    tokens,
    types::{EdgeResult, EdgeToken, TokenValidationStatus},
};

pub(crate) async fn create_client_token_for_fe_token(
    req: &ServiceRequest,
    token: &EdgeToken,
) -> EdgeResult<()> {
    if let Some(feature_refresher) = req.app_data::<Data<FeatureRefresher>>().cloned() {
        debug!("Had a feature refresher");
        let _ = feature_refresher
            .create_client_token_for_fe_token(token.clone())
            .await;
    }
    Ok(())
}

pub async fn client_token_from_frontend_token(
    token: EdgeToken,
    req: ServiceRequest,
    srv: crate::middleware::as_async_middleware::Next<impl MessageBody + 'static>,
) -> Result<ServiceResponse<impl MessageBody>, actix_web::Error> {
    if let Some(token_cache) = req.app_data::<Data<DashMap<String, EdgeToken>>>() {
        if let Some(fe_token) = token_cache.get(&token.token) {
            debug!(
                "Token got extracted to {:#?}",
                tokens::anonymize_token(fe_token.value())
            );
            if fe_token.status == TokenValidationStatus::Validated {
                create_client_token_for_fe_token(&req, &fe_token).await?;
            }
        } else {
            debug!("Did not find token");
        }
    }
    srv.call(req).await
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use std::sync::Arc;
    use tracing_test::traced_test;

    use actix_http::HttpService;
    use actix_http_test::{TestServer, test_server};
    use actix_service::map_config;
    use actix_web::dev::AppConfig;
    use actix_web::{App, web};
    use chrono::Duration;
    use dashmap::DashMap;
    use reqwest::{StatusCode, Url};
    use unleash_yggdrasil::EngineState;

    use crate::auth::token_validator::TokenValidator;
    use crate::delta_cache_manager::DeltaCacheManager;
    use crate::feature_cache::FeatureCache;
    use crate::http::refresher::feature_refresher::FeatureRefresher;
    use crate::http::unleash_client::{HttpClientArgs, UnleashClient, new_reqwest_client};
    use crate::tests::upstream_server;
    use crate::types::{EdgeToken, TokenType, TokenValidationStatus};

    pub async fn local_server(
        unleash_client: Arc<UnleashClient>,
        local_token_cache: Arc<DashMap<String, EdgeToken>>,
        local_features_cache: Arc<FeatureCache>,
        local_engine_cache: Arc<DashMap<String, EngineState>>,
    ) -> TestServer {
        let token_validator = Arc::new(TokenValidator {
            unleash_client: unleash_client.clone(),
            token_cache: local_token_cache.clone(),
            persistence: None,
        });
        let feature_refresher = Arc::new(FeatureRefresher {
            unleash_client: unleash_client.clone(),
            features_cache: local_features_cache.clone(),
            engine_cache: local_engine_cache.clone(),
            refresh_interval: Duration::seconds(5),
            ..Default::default()
        });
        test_server(move || {
            let config = serde_qs::actix::QsQueryConfig::default()
                .qs_config(serde_qs::Config::new(5, false));

            HttpService::new(map_config(
                App::new()
                    .app_data(config)
                    .app_data(web::Data::from(token_validator.clone()))
                    .app_data(web::Data::from(local_features_cache.clone()))
                    .app_data(web::Data::from(local_engine_cache.clone()))
                    .app_data(web::Data::from(local_token_cache.clone()))
                    .app_data(web::Data::from(feature_refresher.clone()))
                    .service(
                        web::scope("/api")
                            .configure(crate::client_api::configure_client_api)
                            .configure(|cfg| {
                                crate::frontend_api::configure_frontend_api(cfg, false)
                            }),
                    )
                    .service(web::scope("/edge").configure(crate::edge_api::configure_edge_api)),
                |_| AppConfig::default(),
            ))
            .tcp()
        })
        .await
    }

    #[tokio::test]
    #[traced_test]
    pub async fn request_with_frontend_token_not_covered_by_client_token_returns_511() {
        let mut frontend_token = EdgeToken::from_str("*:development.frontendtoken").unwrap();
        frontend_token.status = TokenValidationStatus::Validated;
        frontend_token.token_type = Some(TokenType::Frontend);
        let upstream_features_cache = Arc::new(FeatureCache::default());
        let upstream_token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        upstream_token_cache.insert(frontend_token.token.clone(), frontend_token.clone());
        let upstream_delta_cache_manager: Arc<DeltaCacheManager> =
            Arc::new(DeltaCacheManager::new());
        let upstream_engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let upstream_server = upstream_server(
            upstream_token_cache.clone(),
            upstream_features_cache.clone(),
            upstream_delta_cache_manager.clone(),
            upstream_engine_cache.clone(),
        )
        .await;

        let meta_information = crate::http::unleash_client::ClientMetaInformation::test_config();
        let http_client = new_reqwest_client(HttpClientArgs {
            client_meta_information: meta_information.clone(),
            ..Default::default()
        })
        .expect("Failed to create client");

        let unleash_client = UnleashClient::from_url(
            Url::parse(&upstream_server.url("/")).unwrap(),
            "test-client".into(),
            http_client,
            meta_information.clone(),
        );
        let local_features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
        let local_token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let local_engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());

        let local_server = local_server(
            Arc::new(unleash_client),
            local_token_cache,
            local_features_cache,
            local_engine_cache,
        )
        .await;
        let client = reqwest::Client::default();
        let frontend_response = client
            .get(local_server.url("/api/frontend"))
            .header("Authorization", frontend_token.token.clone())
            .send()
            .await
            .expect("Failed to send request");
        assert_eq!(
            frontend_response.status(),
            StatusCode::NETWORK_AUTHENTICATION_REQUIRED
        )
    }
}
