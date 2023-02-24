use std::{sync::Arc, time::Duration};

use crate::types::{
    ClientFeaturesRequest, ClientFeaturesResponse, EdgeSink, EdgeSource, EdgeToken,
    ValidateTokensRequest,
};
use tokio::sync::{mpsc::Receiver, mpsc::Sender};
use tracing::{debug, warn};

use super::unleash_client::UnleashClient;

pub async fn poll_for_token_status(
    mut token_channel: Receiver<EdgeToken>,
    feature_channel: Sender<EdgeToken>,
    sink: Arc<dyn EdgeSink>,
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
                Ok(validated_tokens) => match sink.sink_tokens(validated_tokens.clone()).await {
                    Ok(_) => {
                        for valid in validated_tokens {
                            let _ = feature_channel.send(valid).await;
                        }
                    }
                    Err(err) => {
                        warn!("Couldn't sink token. Result: {err:?}")
                    }
                },
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
    source: Arc<dyn EdgeSource>,
    sink: Arc<dyn EdgeSink>,
    unleash_client: UnleashClient,
) {
    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(5)) => {
                let to_refresh = source.get_tokens_due_for_refresh().await;
                if let Ok(refreshes) = to_refresh {
                        debug!("Had {} tokens to refresh", refreshes.len());
                    for refresh in refreshes {
                        let features_result = unleash_client.get_client_features(ClientFeaturesRequest {
                            api_key: refresh.token.token.clone(),
                            etag: refresh.etag,
                        }).await;

                        match features_result {
                            Ok(feature_response) => match feature_response {
                                ClientFeaturesResponse::NoUpdate(_) => {
                                    debug!("No update needed, will update last check time");
                                    let _ = sink.update_last_check(&refresh.token).await;
                                }
                                ClientFeaturesResponse::Updated(features, etag) => {
                                    debug!("Got updated client features. Writing to sink {features:?}");
                                    let sink_result = sink.sink_features(&refresh.token, features).await;
                                    let _ = sink.update_last_refresh(&refresh.token, etag).await;
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
                }
            }
        }
    }
}
