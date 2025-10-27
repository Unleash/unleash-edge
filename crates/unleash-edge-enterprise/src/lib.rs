use std::{pin::Pin, sync::Arc};
use tokio::sync::watch::Sender;
use tracing::{debug, warn};
use unleash_edge_http_client::UnleashClient;
use unleash_edge_types::{RefreshState, enterprise::LicenseStateResponse, tokens::EdgeToken};

async fn send_heartbeat(
    unleash_client: Arc<UnleashClient>,
    token: EdgeToken,
    refresh_state_tx: &Sender<RefreshState>,
) {
    match unleash_client.send_heartbeat(&token).await {
        Ok(response) => match response {
            LicenseStateResponse::Valid => {
                debug!("License check succeeded: Heartbeat sent successfully");
                let _ = refresh_state_tx.send(RefreshState::Running);
            }
            LicenseStateResponse::Expired => {
                warn!(
                    "License check failed: Upstream reports the Enterprise Edge license is expired"
                );
                let _ = refresh_state_tx.send(RefreshState::Running);
            }
            LicenseStateResponse::Invalid => {
                warn!(
                    "License check failed: Upstream reports the Enterprise Edge license is invalid"
                );
                let _ = refresh_state_tx.send(RefreshState::Paused);
            }
        },
        Err(err) => {
            warn!(
                "License check failed: Upstream could not verify the Enterprise Edge license: {err}"
            );
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
