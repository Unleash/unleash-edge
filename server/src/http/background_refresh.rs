use std::{collections::HashSet, sync::Arc, time::Duration};

use crate::types::{
    ClientFeaturesRequest, ClientFeaturesResponse, EdgeSink, EdgeToken, TokenType,
    TokenValidationStatus, ValidateTokensRequest,
};
use tokio::sync::{mpsc::Receiver, mpsc::Sender, RwLock};
use tracing::{info, warn};

use super::unleash_client::UnleashClient;

pub async fn poll_for_token_status(
    mut token_channel: Receiver<EdgeToken>,
    feature_channel: Sender<EdgeToken>,
    sink: Arc<RwLock<dyn EdgeSink>>,
    unleash_client: UnleashClient,
) {
    loop {
        let token = token_channel.recv().await;
        if let Some(token) = token {
            match unleash_client
                .validate_tokens(ValidateTokensRequest {
                    tokens: vec![token.token.clone()],
                })
                .await
            {
                Ok(validated_tokens) => {
                    let mut write_lock = sink.write().await;
                    match write_lock.sink_tokens(validated_tokens.clone()).await {
                        Ok(_) => {
                            for valid in validated_tokens {
                                let _ = feature_channel.send(valid).await;
                            }
                        }
                        Err(err) => {
                            warn!("Couldn't sink token result: {err:?}")
                        }
                    }
                }
                Err(e) => {
                    warn!("Couldn't validate tokens: {e:?}");
                }
            }
        } else {
            // The channel is closed, so we're not ever going to get new messages, so shutdown this task now
            break;
        }
    }
}

pub async fn refresh_features(
    mut channel: Receiver<EdgeToken>,
    sink: Arc<RwLock<dyn EdgeSink>>,
    unleash_client: UnleashClient,
) {
    let mut tokens = HashSet::new();
    loop {
        tokio::select! {
            token = channel.recv() => { // Got a new token
                if let Some(token) = token {
                    if token.token_type == Some(TokenType::Client)  && token.status == TokenValidationStatus::Validated {
                        tokens.insert(token);
                    }
                } else {
                    break;
                }
            }
            ,
            _ = tokio::time::sleep(Duration::from_secs(10)) => { // Iterating over known tokens
                let mut write_lock = sink.write().await;
                info!("Updating features for known tokens. Know of {} tokens", tokens.len());
                for token in tokens.iter() {
                    let features_result = unleash_client.get_client_features(ClientFeaturesRequest {
                        api_key: token.token.clone(),
                        etag: None,
                    }).await;
                    match features_result {
                        Ok(feature_response) => match feature_response {
                            ClientFeaturesResponse::NoUpdate(_) => info!("No update needed"),
                            ClientFeaturesResponse::Updated(features, _) => {
                                info!("Got updated client features. Writing to sink {features:?}");
                                let sink_result = write_lock.sink_features(token, features).await;
                                if let Err(err) = sink_result {
                                    warn!("Failed to sink features in updater {err:?}");
                                }
                            }
                        },
                        Err(e) => {
                            warn!("Couldn't refresh features: {e:?}");
                        }
                    }
                }
            },
        }
    }
}
