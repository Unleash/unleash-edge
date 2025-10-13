use std::{pin::Pin, sync::Arc};
use tokio::sync::oneshot;
use tracing::{debug, error, info};
use unleash_edge_http_client::UnleashClient;
use unleash_edge_types::{errors::EdgeError, tokens::EdgeToken};

pub async fn send_heartbeat(
    unleash_client: Arc<UnleashClient>,
    token: EdgeToken,
    shutdown_tx: Arc<tokio::sync::Mutex<Option<oneshot::Sender<()>>>>,
) {
    match unleash_client.send_heartbeat(&token).await {
        Err(EdgeError::InvalidLicense(e)) => {
            error!(
                "License was invalidated by upstream: {}. Shutting down Edge.",
                e
            );
            shutdown_tx.lock().await.take().map(|tx| tx.send(()));
        }
        Err(e) => {
            info!("Unexpected error sending heartbeat: {}", e);
        }
        Ok(_) => {
            debug!("Successfully sent heartbeat");
        }
    }
}

pub fn create_enterprise_heartbeat_task(
    unleash_client: Arc<UnleashClient>,
    token: EdgeToken,
    shutdown_tx: oneshot::Sender<()>,
) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    Box::pin(async move {
        let sleep_duration = tokio::time::Duration::from_secs(90);
        let shutdown_arc = Arc::new(tokio::sync::Mutex::new(Some(shutdown_tx)));
        loop {
            tokio::select! {
                _ = tokio::time::sleep(sleep_duration) => {
                    send_heartbeat(unleash_client.clone(), token.clone(), shutdown_arc.clone()).await;
                }
            }
        }
    })
}
