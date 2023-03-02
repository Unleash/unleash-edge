use std::time::Duration;

use actix_web::http::header::EntityTag;
use chrono::Utc;
use tracing::{debug, warn};
use unleash_types::Upsert;

use crate::{
    tokens::cache_key,
    types::{
        ClientFeaturesRequest, ClientFeaturesResponse, EdgeResult, EdgeToken, FeatureRefresher,
        TokenRefresh,
    },
};

impl FeatureRefresher {
    pub fn get_tokens_due_for_refresh(&self) -> EdgeResult<Vec<TokenRefresh>> {
        let tokens_due_for_refresh = self
            .tokens_to_refresh
            .iter()
            .map(|e| e.value().clone())
            .filter(|token| {
                token
                    .last_check
                    .map(|last| Utc::now() - last > self.refresh_interval)
                    .unwrap_or(true)
            })
            .collect();
        Ok(tokens_due_for_refresh)
    }

    pub async fn refresh_features(&self) {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(5)) => {
                    let to_refresh = self.get_tokens_due_for_refresh();
                    if let Ok(refreshes) = to_refresh {
                        for refresh in refreshes {
                            let features_result = self.unleash_client.get_client_features(ClientFeaturesRequest {
                                api_key: refresh.token.token.clone(),
                                etag: refresh.etag
                            }).await;

                            match features_result {
                                Ok(feature_response)  => match feature_response {
                                    ClientFeaturesResponse::NoUpdate(_) => {
                                        debug!("No update needed. Will update last check time");
                                        self.update_last_check(&refresh.token.clone());
                                    }
                                    ClientFeaturesResponse::Updated(features, etag) => {
                                        debug!("Got updated client features. Updating features");
                                        let key = cache_key(refresh.token.clone());
                                        self.update_last_refresh(&refresh.token, etag);
                                        self.features_cache.entry(key).and_modify(|existing_data| {
                                            *existing_data = existing_data.clone().upsert(features.clone());
                                        }).or_insert(features);
                                    }
                                },
                                Err(e) => {
                                    warn!("Couldn't refresh features: {e:?}");
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn update_last_check(&self, token: &EdgeToken) {
        if let Some(mut token) = self.tokens_to_refresh.get_mut(&token.token) {
            token.last_check = Some(chrono::Utc::now());
        }
    }

    pub fn update_last_refresh(&self, token: &EdgeToken, etag: Option<EntityTag>) {
        self.tokens_to_refresh
            .entry(token.token.clone())
            .and_modify(|token_to_refresh| {
                token_to_refresh.last_check = Some(chrono::Utc::now());
                token_to_refresh.last_refreshed = Some(chrono::Utc::now());
                token_to_refresh.etag = etag
            });
    }
}
