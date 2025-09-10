use axum::Json;
use axum::extract::State;
use axum::routing::{Router, post};
use tracing::instrument;
use unleash_edge_appstate::AppState;
use unleash_edge_metrics::client_metrics::{register_bulk_metrics, register_client_metrics};
use unleash_edge_types::tokens::EdgeToken;
use unleash_edge_types::{AcceptedJson, BatchMetricsRequestBody, EdgeAcceptedJsonResult};
use unleash_types::client_metrics::ClientMetrics;

#[utoipa::path(
    post,
    path = "/metrics",
    context_path = "/api/client",
    responses(
        (status = 202, description = "Accepted client metrics"),
        (status = 403, description = "Was not allowed to post metrics"),
    ),
    request_body = ClientMetrics,
    security(
        ("Authorization" = [])
    )
)]
#[instrument(skip(app_state, edge_token, metrics))]
pub async fn post_metrics(
    State(app_state): State<AppState>,
    edge_token: EdgeToken,
    Json(metrics): Json<ClientMetrics>,
) -> EdgeAcceptedJsonResult<()> {
    register_client_metrics(edge_token, metrics, app_state.metrics_cache.clone());
    Ok(AcceptedJson { body: () })
}

#[utoipa::path(
post,
path = "/bulk",
context_path = "/api/client/metrics",
responses(
(status = 202, description = "Accepted bulk metrics"),
(status = 403, description = "Was not allowed to post bulk metrics")
),
request_body = BatchMetricsRequestBody,
security(
("Authorization" = [])
)
)]
#[instrument(skip(app_state, edge_token, bulk_metrics))]
pub async fn post_bulk_metrics(
    app_state: State<AppState>,
    edge_token: EdgeToken,
    bulk_metrics: Json<BatchMetricsRequestBody>,
) -> EdgeAcceptedJsonResult<()> {
    register_bulk_metrics(
        &app_state.metrics_cache,
        &app_state.connect_via,
        &edge_token,
        bulk_metrics.0,
    );
    Ok(AcceptedJson { body: () })
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/metrics", post(post_metrics))
        .route("/metrics/bulk", post(post_bulk_metrics))
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use axum_test::TestServer;
    use chrono::{DateTime, TimeZone, Utc};
    use maplit::hashmap;
    use std::collections::{BTreeMap, HashMap};
    use std::str::FromStr;
    use std::sync::Arc;
    use ulid::Ulid;
    use unleash_edge_appstate::AppState;
    use unleash_edge_types::metrics::{ImpactMetricsKey, MetricsCache};
    use unleash_edge_types::tokens::EdgeToken;
    use unleash_edge_types::{BatchMetricsRequestBody, MetricsKey, TokenCache};
    use unleash_types::client_metrics::SdkType::Backend;
    use unleash_types::client_metrics::{
        ClientApplication, ClientMetrics, ClientMetricsEnv, ConnectVia, ImpactMetric, MetricBucket,
        MetricSample, MetricType, MetricsMetadata, ToggleStats,
    };

    async fn build_metrics_server(
        metrics_cache: Arc<MetricsCache>,
        token_cache: Arc<TokenCache>,
    ) -> TestServer {
        let app_state = AppState::builder()
            .with_metrics_cache(metrics_cache.clone())
            .with_token_cache(token_cache.clone())
            .with_connect_via(ConnectVia {
                app_name: "test".into(),
                instance_id: Ulid::new().to_string(),
            })
            .build();
        let router = super::router().with_state(app_state);
        TestServer::builder()
            .http_transport()
            .build(router)
            .expect("Failed to setup test server")
    }

    #[tokio::test]
    pub async fn correctly_aggregates_data() {
        let metrics_cache = Arc::new(MetricsCache::default());
        let token_cache: Arc<TokenCache> = Arc::new(TokenCache::default());
        let token =
            EdgeToken::from_str("*:development.abc123def").expect("Failed to build edge token");
        token_cache.insert(token.token.clone(), token.clone());

        let server = build_metrics_server(metrics_cache.clone(), token_cache).await;
        make_metrics_post_request(server, &token.token).await;
        let cache = metrics_cache.clone();
        assert!(!cache.metrics.is_empty());
        let found_metric = cache
            .metrics
            .get(&MetricsKey {
                app_name: "some-app".into(),
                feature_name: "some-feature".into(),
                timestamp: DateTime::parse_from_rfc3339("1867-11-07T12:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
                environment: "development".into(),
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
            metadata: MetricsMetadata {
                platform_name: None,
                platform_version: None,
                sdk_version: None,
                sdk_type: None,
                yggdrasil_version: None,
            },
        };

        assert_eq!(found_metric.yes, expected.yes);
        assert_eq!(found_metric.yes, 1);
        assert_eq!(found_metric.no, 0);
        assert_eq!(found_metric.no, expected.no);

        let impact_key = ImpactMetricsKey {
            app_name: "some-app".into(),
            environment: "development".into(),
        };
        let impact_metrics = cache.impact_metrics.get(&impact_key).unwrap();
        assert_eq!(impact_metrics.value().len(), 1);

        let impact_metric = &impact_metrics.value()[0];

        let expected_impact_metric = ImpactMetric {
            name: "test_counter".into(),
            help: "Test counter metric".into(),
            r#type: MetricType::Counter,
            samples: vec![MetricSample {
                value: 1.0,
                labels: Some(BTreeMap::from([
                    ("label1".into(), "value1".into()),
                    ("label2".into(), "value2".into()),
                    ("origin".into(), "edge".into()),
                ])),
            }],
        };

        assert_eq!(impact_metric.impact_metric, expected_impact_metric);
    }

    #[tokio::test]
    pub async fn bulk_metrics_endpoint_correctly_accepts_data() {
        let metrics_cache = Arc::new(MetricsCache::default());
        let token_cache: Arc<TokenCache> = Arc::new(TokenCache::default());
        let token =
            EdgeToken::from_str("*:development.abc123def").expect("Failed to build edge token");
        token_cache.insert(token.token.clone(), token.clone());
        let server = build_metrics_server(metrics_cache.clone(), token_cache).await;
        make_bulk_metrics_post_request(server, &token.token).await;
    }

    #[tokio::test]
    pub async fn bulk_metrics_endpoint_correctly_refuses_metrics_without_auth_header() {
        let metrics_cache = Arc::new(MetricsCache::default());
        let token_cache = Arc::new(TokenCache::default());
        let server = build_metrics_server(metrics_cache.clone(), token_cache).await;
        let request = server
            .post("/metrics/bulk")
            .json(
                &serde_json::to_value(BatchMetricsRequestBody {
                    applications: vec![],
                    metrics: vec![],
                    impact_metrics: None,
                })
                .expect("Failed to parse Batch metrics request body"),
            )
            .await;
        assert_eq!(request.status_code(), StatusCode::FORBIDDEN);
    }

    async fn make_metrics_post_request(server: TestServer, authorization: &str) {
        let client_metric = ClientMetrics {
            app_name: "some-app".into(),
            instance_id: Some("some-instance".into()),
            connection_id: Some("some-connection".into()),
            bucket: MetricBucket {
                start: Utc.with_ymd_and_hms(1867, 11, 7, 12, 0, 0).unwrap(),
                stop: Utc.with_ymd_and_hms(1934, 11, 7, 12, 0, 0).unwrap(),
                toggles: hashmap! {
                    "some-feature".to_string() => ToggleStats {
                        yes: 1,
                        no: 0,
                        variants: hashmap! {}
                    }
                },
            },
            environment: Some("development".into()),
            impact_metrics: Some(vec![ImpactMetric {
                name: "test_counter".into(),
                help: "Test counter metric".into(),
                r#type: "counter".into(),
                samples: vec![MetricSample {
                    value: 1.0,
                    labels: Some(BTreeMap::from([
                        ("label1".into(), "value1".into()),
                        ("label2".into(), "value2".into()),
                    ])),
                }],
            }]),
            metadata: MetricsMetadata {
                platform_name: Some("test".into()),
                platform_version: Some("1.0".into()),
                sdk_version: Some("1.0".into()),
                sdk_type: Some(Backend),
                yggdrasil_version: None,
            },
        };
        let r = server
            .post("/metrics")
            .add_header("Authorization", authorization)
            .json(&serde_json::to_value(client_metric).expect("Invalid json"))
            .await;
        assert_eq!(r.status_code(), StatusCode::ACCEPTED);
    }
    async fn make_bulk_metrics_post_request(server: TestServer, authorization: &str) {
        let r = server
            .post("/metrics/bulk")
            .add_header("Authorization", authorization)
            .json(
                &serde_json::to_value(BatchMetricsRequestBody {
                    applications: vec![ClientApplication {
                        app_name: "test_app".to_string(),
                        connect_via: None,
                        environment: None,
                        projects: Some(vec![]),
                        instance_id: None,
                        connection_id: None,
                        interval: 10,
                        started: Default::default(),
                        strategies: vec![],
                        metadata: MetricsMetadata {
                            platform_name: None,
                            platform_version: None,
                            sdk_version: None,
                            sdk_type: None,
                            yggdrasil_version: None,
                        },
                    }],
                    metrics: vec![ClientMetricsEnv {
                        feature_name: "".to_string(),
                        app_name: "".to_string(),
                        environment: "".to_string(),
                        timestamp: Default::default(),
                        yes: 0,
                        no: 0,
                        variants: Default::default(),
                        metadata: MetricsMetadata {
                            platform_name: None,
                            platform_version: None,
                            sdk_version: None,
                            sdk_type: None,
                            yggdrasil_version: None,
                        },
                    }],
                    impact_metrics: Some(vec![ImpactMetric {
                        name: "bulk_test_counter".into(),
                        help: "Bulk test counter metric".into(),
                        r#type: "counter".into(),
                        samples: vec![MetricSample {
                            value: 5.0,
                            labels: Some(BTreeMap::from([
                                ("bulk_label1".into(), "bulk_value1".into()),
                                ("bulk_label2".into(), "bulk_value2".into()),
                            ])),
                        }],
                    }]),
                })
                .expect("Failed to convert to Json"),
            )
            .await;
        assert_eq!(r.status_code(), StatusCode::ACCEPTED);
    }
}
