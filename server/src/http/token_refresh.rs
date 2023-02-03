use std::sync::Arc;

use crate::types::{EdgeSink, EdgeToken};
use tokio::sync::{mpsc::Receiver, RwLock};
use tracing::warn;

pub async fn poll_for_token_status(
    mut channel: Receiver<EdgeToken>,
    sink: Arc<RwLock<dyn EdgeSink>>,
) {
    loop {
        let token = channel.recv().await;
        if let Some(token) = token {
            let mut write_lock = sink.write().await;
            match write_lock.validate(vec![token]).await {
                Ok(validated_tokens) => {
                    let sink_result = write_lock.sink_tokens(validated_tokens).await;
                    if let Err(err) = sink_result {
                        warn!("Couldn't sink token result {err:?}")
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
