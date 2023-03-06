use crate::error::EdgeError;
use crate::metrics::client_metrics::{ApplicationKey, MetricsCache};
use crate::tokens::cache_key;
use crate::types::{EdgeJsonResult, EdgeResult, EdgeToken};
use actix_web::web::{self, Json};
use actix_web::{get, post, HttpRequest, HttpResponse};
use dashmap::DashMap;
use tracing::debug;
use unleash_types::client_features::ClientFeatures;
use unleash_types::client_metrics::{
    from_bucket_app_name_and_env, ClientApplication, ClientMetrics, ConnectVia,
};

#[utoipa::path(
    path = "/api/client/features",
    responses(
        (status = 200, description = "Return feature toggles for this token", body = ClientFeatures),
        (status = 403, description = "Was not allowed to access features"),
        (status = 400, description = "Invalid parameters used")
    ),
    security(
        ("Authorization" = [])
    )
)]
#[get("/client/features")]
pub async fn features(
    edge_token: EdgeToken,
    features_cache: web::Data<DashMap<String, ClientFeatures>>,
) -> EdgeJsonResult<ClientFeatures> {
    features_cache
        .get(&cache_key(edge_token))
        .map(|features| features.clone())
        .map(Json)
        .ok_or_else(|| EdgeError::PersistenceError("Feature set not present in cache yet".into()))
}

#[utoipa::path(
    path = "/api/client/register",
    responses(
        (status = 202, description = "Accepted client application registration"),
        (status = 403, description = "Was not allowed to access features"),
    ),
    request_body = ClientApplication,
    security(
        ("Authorization" = [])
    )
)]
#[post("/client/register")]
pub async fn register(
    edge_token: EdgeToken,
    connect_via: web::Data<ConnectVia>,
    _req: HttpRequest,
    client_application: web::Json<ClientApplication>,
    metrics_cache: web::Data<MetricsCache>,
) -> EdgeResult<HttpResponse> {
    let client_application = client_application.into_inner();
    let updated_with_connection_info = client_application.connect_via(
        connect_via.app_name.as_str(),
        connect_via.instance_id.as_str(),
    );
    let to_write = ClientApplication {
        environment: edge_token.environment,
        ..updated_with_connection_info
    };
    metrics_cache.applications.insert(
        ApplicationKey {
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

#[utoipa::path(
    path = "/api/client/metrics",
    responses(
        (status = 202, description = "Accepted client metrics"),
        (status = 403, description = "Was not allowed to access features"),
    ),
    request_body = ClientMetrics,
    security(
        ("Authorization" = [])
    )
)]
#[post("/client/metrics")]
pub async fn metrics(
    edge_token: EdgeToken,
    metrics: Json<ClientMetrics>,
    metrics_cache: web::Data<MetricsCache>,
) -> EdgeResult<HttpResponse> {
    let metrics = metrics.into_inner();
    let metrics = from_bucket_app_name_and_env(
        metrics.bucket,
        metrics.app_name,
        edge_token.environment.unwrap(),
    );
    debug!("Received metrics: {metrics:?}");
    metrics_cache.sink_metrics(&metrics);
    Ok(HttpResponse::Accepted().finish())
}

pub fn configure_client_api(cfg: &mut web::ServiceConfig) {
    cfg.service(features).service(register).service(metrics);
}

#[cfg(test)]
mod tests {

    use std::{collections::HashMap, sync::Arc};

    use crate::metrics::client_metrics::MetricsKey;

    use super::*;

    use actix_http::Request;
    use actix_web::{
        http::header::ContentType,
        test,
        web::{self, Data},
        App,
    };
    use chrono::{DateTime, Utc};
    use serde_json::json;
    use ulid::Ulid;
    use unleash_types::client_metrics::ClientMetricsEnv;

    async fn make_test_request() -> Request {
        test::TestRequest::post()
            .uri("/api/client/metrics")
            .insert_header(ContentType::json())
            .insert_header((
                "Authorization",
                "*:development.03fa5f506428fe80ed5640c351c7232e38940814d2923b08f5c05fa7",
            ))
            .set_json(json!({
                "appName": "some-app",
                "instanceId": "some-instance",
                "bucket": {
                  "start": "1867-11-07T12:00:00Z",
                  "stop": "1934-11-07T12:00:00Z",
                  "toggles": {
                    "some-feature": {
                      "yes": 1,
                      "no": 0
                    }
                  }
                }
            }))
            .to_request()
    }

    #[actix_web::test]
    async fn metrics_endpoint_correctly_aggregates_data() {
        let metrics_cache = Arc::new(MetricsCache::default());

        let app = test::init_service(
            App::new()
                .app_data(Data::new(ConnectVia {
                    app_name: "test".into(),
                    instance_id: Ulid::new().to_string(),
                }))
                .app_data(Data::from(metrics_cache.clone()))
                .service(web::scope("/api").service(super::metrics)),
        )
        .await;

        let req = make_test_request().await;
        let _result = test::call_and_read_body(&app, req).await;

        let cache = metrics_cache.clone();

        let found_metric = cache
            .metrics
            .get(&MetricsKey {
                app_name: "some-app".into(),
                feature_name: "some-feature".into(),
                timestamp: DateTime::parse_from_rfc3339("1867-11-07T12:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
            })
            .unwrap();

        let expected = ClientMetricsEnv {
            app_name: "some-app".into(),
            feature_name: "some-feature".into(),
            environment: "development".into(),
            timestamp: DateTime::parse_from_rfc3339("1867-11-07T12:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            yes: 1,
            no: 0,
            variants: HashMap::new(),
        };

        assert_eq!(found_metric.yes, expected.yes);
        assert_eq!(found_metric.yes, 1);
        assert_eq!(found_metric.no, 0);
        assert_eq!(found_metric.no, expected.no);
    }
}
