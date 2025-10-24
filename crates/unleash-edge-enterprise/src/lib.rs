use std::{pin::Pin, sync::Arc};
use tokio::sync::watch::Sender;
use tracing::{debug, info, warn};
use unleash_edge_http_client::UnleashClient;
use unleash_edge_types::{RefreshState, errors::EdgeError, tokens::EdgeToken};

pub async fn send_heartbeat(
    unleash_client: Arc<UnleashClient>,
    token: EdgeToken,
    refresh_state_tx: &Sender<RefreshState>,
) {
    match unleash_client.send_heartbeat(&token).await {
        Err(EdgeError::ExpiredLicense(e)) => {
            warn!("License is expired according to upstream: {}", e);
        }
        Err(EdgeError::InvalidLicense(e)) => {
            warn!("License is invalid according to upstream: {}", e);
            let _ = refresh_state_tx.send(RefreshState::Paused);
        }
        Err(e) => {
            info!("Unexpected error sending heartbeat: {}", e);
        }
        Ok(_) => {
            debug!("Successfully sent heartbeat");
            let _ = refresh_state_tx.send(RefreshState::Running);
        }
    }
}

pub fn create_enterprise_heartbeat_task(
    unleash_client: Arc<UnleashClient>,
    token: EdgeToken,
    refresh_state_tx: Sender<RefreshState>,
) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    Box::pin(async move {
        let sleep_duration = tokio::time::Duration::from_secs(90);
        loop {
            tokio::time::sleep(sleep_duration).await;
            send_heartbeat(unleash_client.clone(), token.clone(), &refresh_state_tx).await;
        }
    })
}
