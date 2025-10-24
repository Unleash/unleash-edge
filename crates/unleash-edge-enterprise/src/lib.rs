use std::{pin::Pin, sync::Arc};
use tracing::{debug, error, info};
use unleash_edge_http_client::UnleashClient;
use unleash_edge_types::{errors::EdgeError, tokens::EdgeToken};

pub async fn send_heartbeat(unleash_client: Arc<UnleashClient>, token: EdgeToken) {
    match unleash_client.send_heartbeat(&token).await {
        Err(EdgeError::InvalidLicense(e)) => {
            error!(
                "License was invalidated by upstream: {}. Shutting down Edge.",
                e
            );
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
) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    Box::pin(async move {
        let sleep_duration = tokio::time::Duration::from_secs(90);
        loop {
            tokio::time::sleep(sleep_duration).await;
            send_heartbeat(unleash_client.clone(), token.clone()).await;
        }
    })
}
