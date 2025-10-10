use std::{pin::Pin, process, sync::Arc};
use tracing::{debug, error, info};
use unleash_edge_http_client::UnleashClient;
use unleash_edge_types::{errors::EdgeError, tokens::EdgeToken};

pub fn create_enterprise_heartbeat_task(
    unleash_client: Arc<UnleashClient>,
    token: EdgeToken,
) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    Box::pin(async move {
        let sleep_duration = tokio::time::Duration::from_secs(1);
        loop {
            tokio::select! {
                _ = tokio::time::sleep(sleep_duration) => {
                    match unleash_client.send_heartbeat(&token).await {
                        Err(EdgeError::Forbidden(e)) => {
                            // this should poison pill the process keeping the tcp server alive rather
                            error!("Error sending heartbeat: {}", e);
                            process::exit(1);
                        }
                        Err(e) => {
                            info!("Unexpected error sending heartbeat: {}", e);
                        }
                        Ok(_) => {
                            debug!("Successfully sent heartbeat");
                        }
                    }
                }
            }
        }
    })
}
