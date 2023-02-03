use crate::types::{EdgeJsonResult, EdgeSource, EdgeToken};
use actix_web::get;
use actix_web::web::{self, Json};
use tokio::sync::RwLock;
use unleash_types::client_features::ClientFeatures;

#[get("/client/features")]
async fn features(
    edge_token: EdgeToken,
    features_source: web::Data<RwLock<dyn EdgeSource>>,
) -> EdgeJsonResult<ClientFeatures> {
    features_source
        .read()
        .await
        .get_client_features(&edge_token)
        .await
        .map(Json)
}

pub fn configure_client_api(cfg: &mut web::ServiceConfig) {
    cfg.service(features);
}
