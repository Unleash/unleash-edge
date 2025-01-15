use actix_web::http::header::EntityTag;
use json_structural_diff::JsonDiff;
use reqwest::StatusCode;
use tracing::{debug, info, warn};
use unleash_types::client_features::ClientFeaturesDelta;
use unleash_yggdrasil::EngineState;

use crate::error::{EdgeError, FeatureError};
use crate::http::refresher::feature_refresher::FeatureRefresher;
use crate::tokens::cache_key;
use crate::types::{
    ClientFeaturesDeltaResponse, ClientFeaturesRequest, ClientFeaturesResponse, EdgeToken,
    TokenRefresh,
};

impl FeatureRefresher {
    async fn handle_client_features_delta_updated(
        &self,
        refresh_token: &EdgeToken,
        delta: ClientFeaturesDelta,
        etag: Option<EntityTag>,
    ) {
        let updated_len = delta.updated.len();
        let removed_len = delta.removed.len();

        debug!(
            "Got updated client features delta. Updating features with {etag:?}, updated {updated_len}, removed {removed_len}"
        );

        let key = cache_key(refresh_token);
        self.features_cache.apply_delta(key.clone(), &delta);
        self.update_last_refresh(
            refresh_token,
            etag,
            self.features_cache.get(&key).unwrap().features.len(),
        );
        self.engine_cache
            .entry(key.clone())
            .and_modify(|engine| {
                engine.take_delta(&delta);
            })
            .or_insert_with(|| {
                let mut new_state = EngineState::default();

                let warnings = new_state.take_delta(&delta);
                if let Some(warnings) = warnings {
                    warn!("The following toggle failed to compile and will be defaulted to off: {warnings:?}");
                };
                new_state
            });
    }

    pub async fn refresh_single_delta(&self, refresh: TokenRefresh) {
        let delta_result = self
            .unleash_client
            .get_client_features_delta(ClientFeaturesRequest {
                api_key: refresh.token.token.clone(),
                etag: refresh.etag.clone(),
            })
            .await;
        match delta_result {
            Ok(delta_response) => match delta_response {
                ClientFeaturesDeltaResponse::NoUpdate(tag) => {
                    debug!("No update needed. Will update last check time with {tag}");
                    self.update_last_check(&refresh.token.clone());
                }
                ClientFeaturesDeltaResponse::Updated(features, etag) => {
                    self.handle_client_features_delta_updated(&refresh.token, features, etag)
                        .await;
                    self.compare_delta_cache(&refresh).await;
                }
            },
            Err(e) => {
                match e {
                    EdgeError::ClientFeaturesFetchError(fe) => {
                        match fe {
                            FeatureError::Retriable(status_code) => match status_code {
                                StatusCode::INTERNAL_SERVER_ERROR
                                | StatusCode::BAD_GATEWAY
                                | StatusCode::SERVICE_UNAVAILABLE
                                | StatusCode::GATEWAY_TIMEOUT => {
                                    info!("Upstream is having some problems, increasing my waiting period");
                                    self.backoff(&refresh.token);
                                }
                                StatusCode::TOO_MANY_REQUESTS => {
                                    info!("Got told that upstream is receiving too many requests");
                                    self.backoff(&refresh.token);
                                }
                                _ => {
                                    info!("Couldn't refresh features, but will retry next go")
                                }
                            },
                            FeatureError::AccessDenied => {
                                info!("Token used to fetch features was Forbidden, will remove from list of refresh tasks");
                                self.tokens_to_refresh.remove(&refresh.token.token);
                                if !self.tokens_to_refresh.iter().any(|e| {
                                    e.value().token.environment == refresh.token.environment
                                }) {
                                    let cache_key = cache_key(&refresh.token);
                                    // No tokens left that access the environment of our current refresh. Deleting client features and engine cache
                                    self.features_cache.remove(&cache_key);
                                    self.engine_cache.remove(&cache_key);
                                }
                            }
                            FeatureError::NotFound => {
                                info!("Had a bad URL when trying to fetch features. Increasing waiting period for the token before trying again");
                                self.backoff(&refresh.token);
                            }
                        }
                    }
                    EdgeError::ClientCacheError => {
                        info!("Couldn't refresh features, but will retry next go")
                    }
                    _ => info!("Couldn't refresh features: {e:?}. Will retry next pass"),
                }
            }
        }
    }

    async fn compare_delta_cache(&self, refresh: &TokenRefresh) {
        let features_result = self
            .unleash_client
            .get_client_features(ClientFeaturesRequest {
                api_key: refresh.token.token.clone(),
                etag: refresh.clone().etag,
            })
            .await;

        let key = cache_key(&refresh.token);
        if let Some(delta_features) = self.features_cache.get(&key).as_ref() {
            if let Ok(ClientFeaturesResponse::Updated(features, _etag)) = features_result {
                let d_features = &delta_features.features;
                let o_features = features.features;

                let delta_json = serde_json::to_value(&d_features).unwrap();
                let old_json = serde_json::to_value(&o_features).unwrap();

                let delta_json_len = delta_json.to_string().len();
                let old_json_len = old_json.to_string().len();

                if delta_json_len == old_json_len {
                    info!("The JSON structures are identical.");
                } else {
                    info!("Structural differences found:");
                    info!("Length of delta_json: {}", delta_json_len);
                    info!("Length of old_json: {}", old_json_len);
                    let diff = JsonDiff::diff(&delta_json, &old_json, false);
                    debug!("{:?}", diff.diff.unwrap());
                }
            }
        }
    }
}

#[cfg(feature = "delta")]
#[cfg(test)]
mod tests {
    use crate::feature_cache::FeatureCache;
    use crate::http::refresher::feature_refresher::FeatureRefresher;
    use crate::http::unleash_client::{ClientMetaInformation, UnleashClient};
    use crate::types::EdgeToken;
    use actix_http::header::IF_NONE_MATCH;
    use actix_http::HttpService;
    use actix_http_test::{test_server, TestServer};
    use actix_service::map_config;
    use actix_web::dev::AppConfig;
    use actix_web::http::header::{ETag, EntityTag};
    use actix_web::{web, App, HttpRequest, HttpResponse};
    use chrono::Duration;
    use dashmap::DashMap;
    use std::sync::Arc;
    use unleash_types::client_features::{
        ClientFeature, ClientFeatures, ClientFeaturesDelta, Constraint, Operator, Segment,
    };
    use unleash_yggdrasil::EngineState;

    #[actix_web::test]
    #[tracing_test::traced_test]
    async fn test_delta() {
        let srv = test_features_server().await;
        let unleash_client = Arc::new(UnleashClient::new(srv.url("/").as_str(), None).unwrap());
        let features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
        let engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());

        let feature_refresher = Arc::new(FeatureRefresher {
            unleash_client: unleash_client.clone(),
            tokens_to_refresh: Arc::new(Default::default()),
            features_cache: features_cache.clone(),
            engine_cache: engine_cache.clone(),
            refresh_interval: Duration::seconds(6000),
            persistence: None,
            strict: false,
            streaming: false,
            delta: true,
            client_meta_information: ClientMetaInformation::test_config(),
        });
        let features = ClientFeatures {
            version: 2,
            features: vec![],
            segments: None,
            query: None,
            meta: None,
        };
        let initial_features = features.modify_and_copy(&revision(1));
        let final_features = initial_features.modify_and_copy(&revision(2));
        let token =
            EdgeToken::try_from("*:development.abcdefghijklmnopqrstuvwxyz".to_string()).unwrap();
        feature_refresher
            .register_token_for_refresh(token.clone(), None)
            .await;
        feature_refresher.refresh_features().await;
        let refreshed_features = features_cache
            .get(&cache_key(&token))
            .unwrap()
            .value()
            .clone();
        assert_eq!(refreshed_features, initial_features);

        let token_refresh = feature_refresher
            .tokens_to_refresh
            .get(&token.token)
            .unwrap()
            .clone();
        feature_refresher.refresh_single_delta(token_refresh).await;
        let refreshed_features = features_cache
            .get(&cache_key(&token))
            .unwrap()
            .value()
            .clone();
        assert_eq!(refreshed_features, final_features);
    }

    fn cache_key(token: &EdgeToken) -> String {
        token
            .environment
            .clone()
            .unwrap_or_else(|| token.token.clone())
    }

    fn revision(revision_id: u32) -> ClientFeaturesDelta {
        match revision_id {
            1 => ClientFeaturesDelta {
                updated: vec![
                    ClientFeature {
                        name: "test1".into(),
                        feature_type: Some("release".into()),
                        ..Default::default()
                    },
                    ClientFeature {
                        name: "test2".into(),
                        feature_type: Some("release".into()),
                        ..Default::default()
                    },
                ],
                removed: vec![],
                segments: Some(vec![Segment {
                    id: 1,
                    constraints: vec![Constraint {
                        context_name: "userId".into(),
                        operator: Operator::In,
                        case_insensitive: false,
                        inverted: false,
                        values: Some(vec!["7".into()]),
                        value: None,
                    }],
                }]),
                revision_id: 1,
            },
            _ => ClientFeaturesDelta {
                updated: vec![ClientFeature {
                    name: "test1".into(),
                    feature_type: Some("release".into()),
                    ..Default::default()
                }],
                removed: vec!["test2".to_string()],
                segments: None,
                revision_id: 2,
            },
        }
    }

    async fn return_client_features_delta(etag_header: Option<String>) -> HttpResponse {
        match etag_header {
            Some(value) => match value.as_str() {
                "\"1\"" => HttpResponse::Ok()
                    .insert_header(ETag(EntityTag::new_strong("2".to_string())))
                    .json(revision(2)),
                "\"2\"" => HttpResponse::NotModified().finish(),
                _ => HttpResponse::NotModified().finish(),
            },
            None => HttpResponse::Ok()
                .insert_header(ETag(EntityTag::new_strong("1".to_string())))
                .json(revision(1)),
        }
    }

    async fn test_features_server() -> TestServer {
        test_server(move || {
            HttpService::new(map_config(
                App::new().service(web::resource("/api/client/delta").route(web::get().to(
                    |req: HttpRequest| {
                        let etag_header = req
                            .headers()
                            .get(IF_NONE_MATCH)
                            .and_then(|h| h.to_str().ok());
                        return_client_features_delta(etag_header.map(|s| s.to_string()))
                    },
                ))),
                |_| AppConfig::default(),
            ))
            .tcp()
        })
        .await
    }
}
