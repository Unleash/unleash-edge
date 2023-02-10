use crate::metrics::client_metrics::{ApplicationKey, MetricsCache};
use crate::types::{EdgeJsonResult, EdgeResult, EdgeSource, EdgeToken};
use actix_web::web::{self, Json};
use actix_web::{get, post, HttpRequest, HttpResponse};
use tokio::sync::RwLock;
use tracing::info;
use unleash_types::client_features::ClientFeatures;
use unleash_types::client_metrics::{
    from_bucket_app_name_and_env, ClientApplication, ClientMetrics,
};

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

#[post("/client/register")]
async fn register(
    edge_token: EdgeToken,
    _req: HttpRequest,
    client_application: web::Json<ClientApplication>,
    metrics_cache: web::Data<RwLock<MetricsCache>>,
) -> EdgeResult<HttpResponse> {
    let client_application = client_application.into_inner();
    let to_write = ClientApplication {
        environment: edge_token.environment,
        ..client_application
    };
    {
        let mut writeable_cache = metrics_cache.write().await;
        writeable_cache.applications.insert(
            ApplicationKey {
                app_name: to_write.app_name.clone(),
                instance_id: to_write
                    .instance_id
                    .clone()
                    .unwrap_or_else(|| ulid::Ulid::new().to_string()),
            },
            to_write,
        );
    }
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

#[get("/client/metrics")]
async fn metrics(
    edge_token: EdgeToken,
    metrics: web::Json<ClientMetrics>,
    metrics_cache: web::Data<RwLock<MetricsCache>>,
) -> EdgeResult<HttpResponse> {
    let metrics = metrics.into_inner();
    let metrics = from_bucket_app_name_and_env(
        metrics.bucket,
        metrics.app_name,
        edge_token.environment.unwrap(),
    );

    {
        let mut writeable_cache = metrics_cache.write().await;

        writeable_cache.sink_metrics(&metrics);
    }
    Ok(HttpResponse::Accepted().finish())
}

pub fn configure_client_api(cfg: &mut web::ServiceConfig) {
    cfg.service(features)
        .service(register)
        .service(show_applications);
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
    use unleash_types::client_metrics::ClientMetricsEnv;

    async fn make_test_request() -> Request {
        test::TestRequest::get()
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
        let metrics_cache = Arc::new(RwLock::new(MetricsCache::default()));

        let app = test::init_service(
            App::new()
                .app_data(Data::from(metrics_cache.clone()))
                .service(web::scope("/api").service(super::metrics)),
        )
        .await;

        let req = make_test_request().await;
        let _result = test::call_and_read_body(&app, req).await;

        let cache = metrics_cache.read().await;

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
