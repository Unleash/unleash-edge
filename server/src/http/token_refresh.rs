use std::sync::Arc;

use crate::types::{EdgeSink, EdgeToken, ValidateTokenRequest};
use tokio::sync::{mpsc::Receiver, RwLock};

use super::unleash_client::UnleashClient;

pub async fn poll_for_token_status(
    mut channel: Receiver<EdgeToken>,
    sink: Arc<RwLock<dyn EdgeSink>>,
) {
    loop {
        let token = channel.recv().await;
        if let Some(token) = token {
            let sink_result = sink.write().await.sink_tokens(vec![token]).await;
            if let Err(sink_err) = sink_result {
                // probably log some stuff
                println!("Error sinking token: {sink_err:?}");
            } else {
                println!("I got a token to check :)");
            }
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
