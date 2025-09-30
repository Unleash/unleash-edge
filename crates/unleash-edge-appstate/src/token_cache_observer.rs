use prometheus::{IntGaugeVec, register_int_gauge_vec};
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use unleash_edge_auth::token_validator::TokenValidator;
use unleash_edge_types::{BackgroundTask, TokenValidationStatus};
pub const INSTANCE_ID: &str = "instance_id";
pub const APP_NAME: &str = "app_name";
pub const STATUS: &str = "status";
pub const TOKEN_OBSERVATION_DELAY: Duration = Duration::from_secs(60);

pub static TOKEN_VALIDATION_STATUS: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register_int_gauge_vec!(
        "token_status",
        "Tokens validated",
        &[APP_NAME, INSTANCE_ID, STATUS]
    )
    .unwrap()
});

pub fn observe_tokens_in_background(
    app_name: String,
    instance_id: String,
    token_validator: Arc<TokenValidator>,
) -> BackgroundTask {
    Box::pin(async move {
        loop {
            tokio::time::sleep(TOKEN_OBSERVATION_DELAY).await;
            let mut invalid = 0;
            let mut valid = 0;
            let mut unknown = 0;
            let mut trusted = 0;
            token_validator
                .token_cache
                .iter()
                .for_each(|t| match t.status {
                    TokenValidationStatus::Invalid => {
                        invalid += 1;
                    }
                    TokenValidationStatus::Unknown => {
                        unknown += 1;
                    }
                    TokenValidationStatus::Trusted => {
                        trusted += 1;
                    }
                    TokenValidationStatus::Validated => {
                        valid += 1;
                    }
                });
            TOKEN_VALIDATION_STATUS
                .with_label_values(&[app_name.as_str(), instance_id.as_str(), "invalid"])
                .set(invalid);
            TOKEN_VALIDATION_STATUS
                .with_label_values(&[app_name.as_str(), instance_id.as_str(), "unknown"])
                .set(unknown);
            TOKEN_VALIDATION_STATUS
                .with_label_values(&[app_name.as_str(), instance_id.as_str(), "trusted"])
                .set(trusted);
            TOKEN_VALIDATION_STATUS
                .with_label_values(&[app_name.as_str(), instance_id.as_str(), "validated"])
                .set(valid);
        }
    })
}
