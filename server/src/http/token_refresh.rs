use std::sync::{Arc, RwLock};

use crate::types::{EdgeToken, ValidateTokenRequest, FeatureSink};
use tokio::sync::mpsc::Receiver;

use super::unleash_client::UnleashClient;

pub async fn poll_for_token_status(mut channel: Receiver<EdgeToken>, sink: Arc<RwLock<dyn FeatureSink>>) {
    loop {
        let token = channel.recv().await;
        if let Some(token) = token {

        } else {
            // The channel is closed, so we're not ever going to get new messages, so shutdown this task now
            break;
        }
    }
}

pub(crate) async fn refresh_token(client: &UnleashClient, token: EdgeToken) {
    let request = ValidateTokenRequest {
        tokens: vec![token.token],
    };
    match client.validate_token(request).await {
        Ok(response) => todo!(),
        Err(error) => todo!(),
    }
}
