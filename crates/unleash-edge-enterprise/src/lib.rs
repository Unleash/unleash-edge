use std::{pin::Pin, sync::Arc};
use tokio::sync::{RwLock, watch::Sender};
use ulid::Ulid;
use unleash_edge_http_client::UnleashClient;
use unleash_edge_persistence::EdgePersistence;
use unleash_edge_types::{RefreshState, enterprise::LicenseState, tokens::EdgeToken};

pub fn create_enterprise_heartbeat_task(
    unleash_client: Arc<UnleashClient>,
    token: EdgeToken,
    refresh_state_tx: Sender<RefreshState>,
    connection_id: Ulid,
    app_license_state: Arc<RwLock<LicenseState>>,
    persistence: Option<Arc<dyn EdgePersistence>>,
) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    Box::pin(async move {
        let sleep_duration = tokio::time::Duration::from_secs(90);
        loop {
            tokio::time::sleep(sleep_duration).await;
            let license_state = unleash_client
                .send_heartbeat(&token.clone(), &connection_id)
                .await;

            if let Ok(new_state) = license_state {
                let mut app_license_state = app_license_state.write().await;
                *app_license_state = new_state;

                let _ = refresh_state_tx.send(new_state.into());

                if let Some(persistence) = &persistence {
                    let _ = persistence.save_license_state(&new_state).await;
                }
            }
        }
    })
}
