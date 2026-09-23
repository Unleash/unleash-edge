use std::{pin::Pin, sync::Arc};
use tokio::sync::watch::Sender;
use tracing::warn;
use ulid::Ulid;
use unleash_edge_http_client::UnleashClient;
use unleash_edge_persistence::EdgePersistence;
use unleash_edge_types::{
    RefreshState, TokenCache, TokenType, TokenValidationStatus,
    enterprise::{ApplicationLicenseState, LicenseState},
    tokens::EdgeToken,
};

fn find_first_valid_token(token_cache: &TokenCache) -> Option<EdgeToken> {
    token_cache
        .iter()
        .find_map(|t| match (&t.status, &t.token_type) {
            (TokenValidationStatus::Validated, Some(TokenType::Backend)) => Some(t.value().clone()),
            _ => None,
        })
}

pub fn create_enterprise_heartbeat_task(
    unleash_client: Arc<UnleashClient>,
    token_cache: Arc<TokenCache>,
    refresh_state_tx: Sender<RefreshState>,
    connection_id: Ulid,
    app_license_state: ApplicationLicenseState,
    persistence: Option<Arc<dyn EdgePersistence>>,
) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    Box::pin(async move {
        let sleep_duration = tokio::time::Duration::from_secs(90);
        loop {
            tokio::time::sleep(sleep_duration).await;
            if let Some(token) = find_first_valid_token(&token_cache) {
                let license_state = unleash_client.send_heartbeat(&token, &connection_id).await;

                if let Ok(new_state) = license_state {
                    app_license_state.set(new_state);
                    if new_state == LicenseState::Invalid {
                        warn!(
                            "Edge license is invalid, features will not be refreshed until this is resolved. This needs to be fixed in your upstream Unleash instance."
                        );
                    }

                    let _ = refresh_state_tx.send(new_state.into());

                    if let Some(persistence) = &persistence {
                        let _ = persistence.save_license_state(&new_state).await;
                    }
                }
            } else {
                warn!(
                    "We could not find a token to validate your Edge license. Features will not be refreshed until this is resolved."
                );
            }
        }
    })
}
