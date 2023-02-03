use std::sync::Arc;

use crate::types::{EdgeSink, EdgeToken};
use tokio::sync::{mpsc::Receiver, RwLock};

pub async fn poll_for_token_status(
    mut channel: Receiver<EdgeToken>,
    sink: Arc<RwLock<dyn EdgeSink>>,
) {
    loop {
        let token = channel.recv().await;
        if let Some(token) = token {
            if let Ok(validated_tokens) = sink.write().await.validate(vec![token]).await {
                let sink_result = sink.write().await.sink_tokens(validated_tokens).await;
                if let Err(_err) = sink_result {
                    //log this
                }
            }
        } else {
            // The channel is closed, so we're not ever going to get new messages, so shutdown this task now
            break;
        }
    }
}
