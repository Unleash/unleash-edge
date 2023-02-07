use crate::metrics::client_metrics::{MetricsCache, MetricsKey};
use crate::types::{EdgeJsonResult, EdgeResult, EdgeSource, EdgeToken};
use actix_web::web::{self, Json};
use actix_web::{get, post, HttpRequest, HttpResponse};
use tokio::sync::RwLock;
use tracing::info;
use unleash_types::client_features::ClientFeatures;
use unleash_types::client_metrics::ClientApplication;

#[get("/client/features")]
async fn features(
    edge_token: EdgeToken,
    features_source: web::Data<RwLock<dyn EdgeSource>>,
) -> EdgeJsonResult<ClientFeatures> {
    info!("Getting data for {edge_token:?}");
    features_source
        .read()
        .await
        .get_client_features(&edge_token)
        .await
        .map(Json)
}

#[post("/client/register")]
async fn register(
    edge_token: EdgeToken,
    _req: HttpRequest,
    client_application: web::Json<ClientApplication>,
    metrics_cache: web::Data<RwLock<MetricsCache>>,
) -> EdgeResult<HttpResponse> {
    let mut writeable_cache = metrics_cache.write().await;
    let client_application = client_application.into_inner();
    let to_write = ClientApplication {
        environment: edge_token.environment,
        ..client_application
    };
    writeable_cache.applications.insert(
        MetricsKey {
            app_name: to_write.app_name.clone(),
            instance_id: to_write
                .instance_id
                .clone()
                .unwrap_or_else(|| ulid::Ulid::new().to_string()),
        },
        to_write,
    );
    Ok(HttpResponse::Accepted().finish())
}

#[get("/client/applications")]
async fn show_applications(
    metrics_cache: web::Data<RwLock<MetricsCache>>,
) -> EdgeJsonResult<Vec<ClientApplication>> {
    Ok(Json(
        metrics_cache
            .read()
            .await
            .applications
            .values()
            .cloned()
            .collect(),
    ))
}

// #[get("/client/metrics")]
// async fn metrics(
//     edge_token: EdgeToken,
//     features_source: web::Data<RwLock<dyn EdgeSource>>,
// ) -> EdgeResult<()> {
//     Ok(())
// }

pub fn configure_client_api(cfg: &mut web::ServiceConfig) {
    cfg.service(features)
        .service(register)
        .service(show_applications);
}
