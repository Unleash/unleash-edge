use std::{collections::HashSet, sync::Arc, time::Duration};

use crate::types::{ClientFeaturesResponse, EdgeSink, EdgeToken, TokenType};
use tokio::sync::{mpsc::Receiver, mpsc::Sender, RwLock};
use tracing::{info, warn};

pub async fn poll_for_token_status(
    mut token_channel: Receiver<EdgeToken>,
    feature_channel: Sender<EdgeToken>,
    sink: Arc<RwLock<dyn EdgeSink>>,
) {
    loop {
        let token = token_channel.recv().await;
        if let Some(token) = token {
            let mut write_lock = sink.write().await;
            match write_lock.validate(vec![token.clone()]).await {
                Ok(validated_tokens) => {
                    let sink_result = write_lock.sink_tokens(validated_tokens.clone()).await;
                    if let Err(err) = sink_result {
                        warn!("Couldn't sink token result: {err:?}")
                    } else {
                        for valid in validated_tokens {
                            let _ = feature_channel.send(valid).await;
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

pub async fn refresh_features(mut channel: Receiver<EdgeToken>, sink: Arc<RwLock<dyn EdgeSink>>) {
    let mut tokens = HashSet::new();
    loop {
        tokio::select! {
            token = channel.recv() => { // Got a new token
                if let Some(token) = token {
                    if token.token_type == Some(TokenType::Client) {
                        tokens.insert(token);
                    }
                } else {
                    break;
                }
            },
            _ = tokio::time::sleep(Duration::from_secs(10)) => { // Iterating over known tokens
                let mut write_lock = sink.write().await;
                info!("Updating features for known tokens");
                for token in tokens.iter() {
                    let features_result = write_lock.fetch_features(token).await;
                    match features_result {
                        Ok(feature_response) => match feature_response {
                            ClientFeaturesResponse::NoUpdate(_) => info!("No update needed"),
                            ClientFeaturesResponse::Updated(features, _) => {
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
