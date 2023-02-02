use std::sync::Arc;

use crate::types::{EdgeToken, TokenSink, ValidateTokenRequest};
use tokio::sync::mpsc::Receiver;

use super::unleash_client::UnleashClient;

pub async fn poll_for_token_status<T>(mut channel: Receiver<EdgeToken>, _sink: Arc<T>)
where
    T: TokenSink,
{
    loop {
        let token = channel.recv().await;
        if let Some(_token) = token {
        } else {
            // The channel is closed, so we're not ever going to get new messages, so shutdown this task now
            break;
        }
    }
}

pub async fn refresh_token(client: &UnleashClient, token: EdgeToken) {
    let request = ValidateTokenRequest {
        tokens: vec![token.token],
    };
    match client.validate_token(request).await {
        Ok(_) => todo!(),
        Err(_) => todo!(),
    }
}
