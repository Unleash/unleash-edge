use crate::cli::{EdgeArgs, EdgeMode};
use crate::delta_filters::{DeltaFilterSet, combined_filter};
use crate::error::EdgeError;
use crate::feature_cache::FeatureCache;
use crate::filters::{
    FeatureFilterSet, filter_client_features, name_match_filter, name_prefix_filter, project_filter,
};
use crate::http::broadcaster::Broadcaster;
use crate::http::instance_data::InstanceDataSending;
use crate::http::refresher::feature_refresher::FeatureRefresher;
use crate::metrics::client_metrics::MetricsCache;
use crate::metrics::edge_metrics::EdgeInstanceData;
use crate::tokens::cache_key;
use crate::types::{
    self, BatchMetricsRequestBody, EdgeJsonResult, EdgeResult, EdgeToken, FeatureFilters,
};
use actix_web::Responder;
use actix_web::web::{self, Data, Json, Query};
use actix_web::{HttpRequest, HttpResponse, get, post};
use dashmap::DashMap;
use tokio::sync::RwLock;
use tracing::instrument;
use unleash_types::client_features::{ClientFeature, ClientFeatures, ClientFeaturesDelta};
use unleash_types::client_metrics::{ClientApplication, ClientMetrics, ConnectVia};

#[utoipa::path(
    context_path = "/api/client",
    params(FeatureFilters),
    responses(
        (status = 200, description = "Return feature toggles for this token", body = ClientFeatures),
        (status = 403, description = "Was not allowed to access features"),
        (status = 400, description = "Invalid parameters used")
    ),
    security(
        ("Authorization" = [])
    )
)]
#[get("/features")]
pub async fn get_features(
    edge_token: EdgeToken,
    features_cache: Data<FeatureCache>,
    token_cache: Data<DashMap<String, EdgeToken>>,
    filter_query: Query<FeatureFilters>,
    req: HttpRequest,
) -> EdgeJsonResult<ClientFeatures> {
    resolve_features(edge_token, features_cache, token_cache, filter_query, req).await
}

#[get("/delta")]
pub async fn get_delta(
    edge_token: EdgeToken,
    token_cache: Data<DashMap<String, EdgeToken>>,
    filter_query: Query<FeatureFilters>,
    req: HttpRequest,
) -> impl Responder {
    let requested_revision_id = req
        .headers()
        .get("If-None-Match")
        .and_then(|value| value.to_str().ok())
        .and_then(|etag| etag.trim_matches('"').parse::<u32>().ok())
        .unwrap_or(0);

    match resolve_delta(
        edge_token,
        token_cache,
        filter_query,
        requested_revision_id,
        req,
    )
    .await
    {
        Ok(Json(None)) => HttpResponse::NotModified().finish(),
        Ok(Json(Some(delta))) => {
            let last_event_id = delta.events.last().map(|e| e.get_event_id()).unwrap_or(0); // should never occur

            HttpResponse::Ok()
                .insert_header(("ETag", format!("{}", last_event_id)))
                .json(delta)
        }
        Err(err) => HttpResponse::InternalServerError().body(format!("Error: {:?}", err)),
    }
}

#[get("/streaming")]
pub async fn stream_features(
    edge_token: EdgeToken,
    broadcaster: Data<Broadcaster>,
    token_cache: Data<DashMap<String, EdgeToken>>,
    edge_mode: Data<EdgeMode>,
    filter_query: Query<FeatureFilters>,
) -> EdgeResult<impl Responder> {
    match edge_mode.get_ref() {
        EdgeMode::Edge(EdgeArgs {
            streaming: true, ..
        }) => {
            let (validated_token, _filter_set, query) =
                get_feature_filter(&edge_token, &token_cache, filter_query.clone())?;

            broadcaster.connect(validated_token, query).await
        }
        _ => Err(EdgeError::Forbidden(
            "This endpoint is only enabled in streaming mode".into(),
        )),
    }
}

#[utoipa::path(
    context_path = "/api/client",
    params(FeatureFilters),
    responses(
        (status = 200, description = "Return feature toggles for this token", body = ClientFeatures),
        (status = 403, description = "Was not allowed to access features"),
        (status = 400, description = "Invalid parameters used")
    ),
    security(
        ("Authorization" = [])
    )
)]
#[post("/features")]
pub async fn post_features(
    edge_token: EdgeToken,
    features_cache: Data<FeatureCache>,
    token_cache: Data<DashMap<String, EdgeToken>>,
    filter_query: Query<FeatureFilters>,
    req: HttpRequest,
) -> EdgeJsonResult<ClientFeatures> {
    resolve_features(edge_token, features_cache, token_cache, filter_query, req).await
}

fn get_feature_filter(
    edge_token: &EdgeToken,
    token_cache: &Data<DashMap<String, EdgeToken>>,
    filter_query: Query<FeatureFilters>,
) -> EdgeResult<(
    EdgeToken,
    FeatureFilterSet,
    unleash_types::client_features::Query,
)> {
    let validated_token = token_cache
        .get(&edge_token.token)
        .map(|e| e.value().clone())
        .ok_or(EdgeError::AuthorizationDenied)?;

    let query_filters = filter_query.into_inner();
    let query = unleash_types::client_features::Query {
        tags: None,
        projects: Some(validated_token.projects.clone()),
        name_prefix: query_filters.name_prefix.clone(),
        environment: validated_token.environment.clone(),
        inline_segment_constraints: Some(false),
    };

    let filter_set = if let Some(name_prefix) = query_filters.name_prefix {
        FeatureFilterSet::from(Box::new(name_prefix_filter(name_prefix)))
    } else {
        FeatureFilterSet::default()
    }
    .with_filter(project_filter(&validated_token));

    Ok((validated_token, filter_set, query))
}

fn get_delta_filter(
    edge_token: &EdgeToken,
    token_cache: &Data<DashMap<String, EdgeToken>>,
    filter_query: Query<FeatureFilters>,
    requested_revision_id: u32,
) -> EdgeResult<DeltaFilterSet> {
    let validated_token = token_cache
        .get(&edge_token.token)
        .map(|e| e.value().clone())
        .ok_or(EdgeError::AuthorizationDenied)?;

    let query_filters = filter_query.into_inner();

    let delta_filter_set = DeltaFilterSet::default().with_filter(combined_filter(
        requested_revision_id,
        validated_token.projects.clone(),
        query_filters.name_prefix.clone(),
    ));

    Ok(delta_filter_set)
}

async fn resolve_features(
    edge_token: EdgeToken,
    features_cache: Data<FeatureCache>,
    token_cache: Data<DashMap<String, EdgeToken>>,
    filter_query: Query<FeatureFilters>,
    req: HttpRequest,
) -> EdgeJsonResult<ClientFeatures> {
    let (validated_token, filter_set, query) =
        get_feature_filter(&edge_token, &token_cache, filter_query.clone())?;

    let client_features = match req.app_data::<Data<FeatureRefresher>>() {
        Some(refresher) => {
            refresher
                .features_for_filter(validated_token.clone(), &filter_set)
                .await
        }
        None => features_cache
            .get(&cache_key(&validated_token))
            .map(|client_features| filter_client_features(&client_features, &filter_set))
            .ok_or(EdgeError::ClientCacheError),
    }?;

    Ok(Json(ClientFeatures {
        query: Some(query),
        ..client_features
    }))
}
async fn resolve_delta(
    edge_token: EdgeToken,
    token_cache: Data<DashMap<String, EdgeToken>>,
    filter_query: Query<FeatureFilters>,
    requested_revision_id: u32,
    req: HttpRequest,
) -> EdgeJsonResult<Option<ClientFeaturesDelta>> {
    let (validated_token, filter_set, ..) =
        get_feature_filter(&edge_token, &token_cache, filter_query.clone())?;

    let delta_filter_set = get_delta_filter(
        &edge_token,
        &token_cache,
        filter_query.clone(),
        requested_revision_id,
    )?;

    let refresher = req.app_data::<Data<FeatureRefresher>>().ok_or_else(|| {
        EdgeError::ClientHydrationFailed(
            "FeatureRefresher is missing - cannot resolve delta in offline mode".to_string(),
        )
    })?;

    let delta = refresher
        .delta_events_for_filter(
            validated_token.clone(),
            &filter_set,
            &delta_filter_set,
            requested_revision_id,
        )
        .await?;

    if delta.events.is_empty() {
        return Ok(Json(None));
    }

    Ok(Json(Some(delta)))
}

#[utoipa::path(
    context_path = "/api/client",
    params(("feature_name" = String, Path,)),
    responses(
        (status = 200, description = "Return feature toggles for this token", body = ClientFeature),
        (status = 403, description = "Was not allowed to access feature"),
        (status = 400, description = "Invalid parameters used"),
        (status = 404, description = "Feature did not exist or token used was not allowed to access it")
    ),
    security(
        ("Authorization" = [])
    )
)]
#[get("/features/{feature_name}")]
pub async fn get_feature(
    edge_token: EdgeToken,
    features_cache: Data<FeatureCache>,
    token_cache: Data<DashMap<String, EdgeToken>>,
    feature_name: web::Path<String>,
    req: HttpRequest,
) -> EdgeJsonResult<ClientFeature> {
    let validated_token = token_cache
        .get(&edge_token.token)
        .map(|e| e.value().clone())
        .ok_or(EdgeError::AuthorizationDenied)?;

    let filter_set = FeatureFilterSet::from(Box::new(name_match_filter(feature_name.clone())))
        .with_filter(project_filter(&validated_token));

    match req.app_data::<Data<FeatureRefresher>>() {
        Some(refresher) => {
            refresher
                .features_for_filter(validated_token.clone(), &filter_set)
                .await
        }
        None => features_cache
            .get(&cache_key(&validated_token))
            .map(|client_features| filter_client_features(&client_features, &filter_set))
            .ok_or(EdgeError::ClientCacheError),
    }
    .map(|client_features| client_features.features.into_iter().next())?
    .ok_or(EdgeError::FeatureNotFound(feature_name.into_inner()))
    .map(Json)
}

#[utoipa::path(
    context_path = "/api/client",
    responses(
        (status = 202, description = "Accepted client application registration"),
        (status = 403, description = "Was not allowed to register client application"),
    ),
    request_body = ClientApplication,
    security(
        ("Authorization" = [])
    )
)]
#[post("/register")]
pub async fn register(
    edge_token: EdgeToken,
    connect_via: Data<ConnectVia>,
    client_application: Json<ClientApplication>,
    metrics_cache: Data<MetricsCache>,
) -> EdgeResult<HttpResponse> {
    crate::metrics::client_metrics::register_client_application(
        edge_token,
        &connect_via,
        client_application.into_inner(),
        metrics_cache,
    );
    Ok(HttpResponse::Accepted()
        .append_header(("X-Edge-Version", types::EDGE_VERSION))
        .finish())
}

#[utoipa::path(
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
#[post("/metrics")]
pub async fn metrics(
    edge_token: EdgeToken,
    metrics: Json<ClientMetrics>,
    metrics_cache: Data<MetricsCache>,
) -> EdgeResult<HttpResponse> {
    crate::metrics::client_metrics::register_client_metrics(
        edge_token,
        metrics.into_inner(),
        metrics_cache,
    );
    Ok(HttpResponse::Accepted().finish())
}

#[utoipa::path(
context_path = "/api/client",
responses(
(status = 202, description = "Accepted bulk metrics"),
(status = 403, description = "Was not allowed to post bulk metrics")
),
request_body = BatchMetricsRequestBody,
security(
("Authorization" = [])
)
)]
#[post("/metrics/bulk")]
pub async fn post_bulk_metrics(
    edge_token: EdgeToken,
    bulk_metrics: Json<BatchMetricsRequestBody>,
    connect_via: Data<ConnectVia>,
    metrics_cache: Data<MetricsCache>,
) -> EdgeResult<HttpResponse> {
    crate::metrics::client_metrics::register_bulk_metrics(
        metrics_cache.get_ref(),
        connect_via.get_ref(),
        &edge_token,
        bulk_metrics.into_inner(),
    );
    Ok(HttpResponse::Accepted().finish())
}

#[post("/metrics/edge")]
#[instrument(skip(_edge_token, instance_data, connected_instances))]
pub async fn post_edge_instance_data(
    _edge_token: EdgeToken,
    instance_data: Json<EdgeInstanceData>,
    instance_data_sending: Data<InstanceDataSending>,
    connected_instances: Data<RwLock<Vec<EdgeInstanceData>>>,
) -> EdgeResult<HttpResponse> {
    if let InstanceDataSending::SendInstanceData(_) = instance_data_sending.as_ref() {
        connected_instances
            .write()
            .await
            .push(instance_data.into_inner());
    }
    Ok(HttpResponse::Accepted().finish())
}

pub fn configure_client_api(cfg: &mut web::ServiceConfig) {
    let client_scope = web::scope("/client")
        .wrap(crate::middleware::as_async_middleware::as_async_middleware(
            crate::middleware::validate_token::validate_token,
        ))
        .wrap(crate::middleware::as_async_middleware::as_async_middleware(
            crate::middleware::consumption::connection_consumption,
        ))
        .service(get_features)
        .service(get_delta)
        .service(get_feature)
        .service(register)
        .service(metrics)
        .service(post_bulk_metrics)
        .service(stream_features)
        .service(post_edge_instance_data);

    cfg.service(client_scope);
}

pub fn configure_experimental_post_features(
    cfg: &mut web::ServiceConfig,
    post_features_enabled: bool,
) {
    if post_features_enabled {
        cfg.service(post_features);
    }
}

#[cfg(test)]
mod tests {

    use crate::metrics::client_metrics::{ApplicationKey, MetricsBatch, MetricsKey};
    use crate::types::{TokenType, TokenValidationStatus};
    use std::collections::{BTreeMap, HashMap};
    use std::path::PathBuf;
    use std::str::FromStr;
    use std::sync::Arc;

    use super::*;

    use crate::auth::token_validator::TokenValidator;
    use crate::cli::{AuthHeaders, OfflineArgs};
    use crate::delta_cache::{DeltaCache, DeltaHydrationEvent};
    use crate::delta_cache_manager::DeltaCacheManager;
    use crate::http::unleash_client::{ClientMetaInformation, UnleashClient};
    use crate::metrics::client_impact_metrics::ImpactMetricsKey;
    use crate::middleware;
    use crate::tests::{features_from_disk, upstream_server};
    use actix_http::{Request, StatusCode};
    use actix_web::{
        App, ResponseError,
        http::header::ContentType,
        test,
        web::{self, Data},
    };
    use chrono::{DateTime, Duration, TimeZone, Utc};
    use maplit::hashmap;
    use ulid::Ulid;
    use unleash_types::client_features::{
        ClientFeature, Constraint, DeltaEvent, Operator, Strategy, StrategyVariant,
    };
    use unleash_types::client_metrics::SdkType::Backend;
    use unleash_types::client_metrics::{
        ClientMetricsEnv, ConnectViaBuilder, ImpactMetric, MetricBucket, MetricSample, MetricType,
        MetricsMetadata, ToggleStats,
    };
    use unleash_yggdrasil::EngineState;

    async fn make_metrics_post_request() -> Request {
        test::TestRequest::post()
            .uri("/api/client/metrics")
            .insert_header(ContentType::json())
            .insert_header((
                "Authorization",
                "*:development.03fa5f506428fe80ed5640c351c7232e38940814d2923b08f5c05fa7",
            ))
            .set_json(Json(ClientMetrics {
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
            }))
            .to_request()
    }

    async fn make_bulk_metrics_post_request(authorization: Option<String>) -> Request {
        let mut req = test::TestRequest::post()
            .uri("/api/client/metrics/bulk")
            .insert_header(ContentType::json());
        req = match authorization {
            Some(auth) => req.insert_header(("Authorization", auth)),
            None => req,
        };
        req.set_json(Json(BatchMetricsRequestBody {
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
        }))
        .to_request()
    }

    async fn make_register_post_request(application: ClientApplication) -> Request {
        test::TestRequest::post()
            .uri("/api/client/register")
            .insert_header(ContentType::json())
            .insert_header((
                "Authorization",
                "*:development.03fa5f506428fe80ed5640c351c7232e38940814d2923b08f5c05fa7",
            ))
            .set_json(Json(application))
            .to_request()
    }

    async fn make_features_request_with_token(token: EdgeToken) -> Request {
        test::TestRequest::get()
            .uri("/api/client/features")
            .insert_header(("Authorization", token.token))
            .to_request()
    }

    async fn make_delta_request_with_token(token: EdgeToken) -> Request {
        test::TestRequest::get()
            .uri("/api/client/delta")
            .insert_header(("Authorization", token.token))
            .to_request()
    }

    async fn make_delta_request_with_token_and_etag(token: EdgeToken, etag: &str) -> Request {
        test::TestRequest::get()
            .uri("/api/client/delta")
            .insert_header(("Authorization", token.token))
            .insert_header(("If-None-Match", etag))
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
                .service(web::scope("/api/client").service(metrics)),
        )
        .await;

        let req = make_metrics_post_request().await;
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

    fn cached_client_features() -> ClientFeatures {
        ClientFeatures {
            version: 2,
            features: vec![
                ClientFeature {
                    name: "feature_one".into(),
                    feature_type: Some("release".into()),
                    description: Some("test feature".into()),
                    created_at: Some(Utc::now()),
                    dependencies: None,
                    last_seen_at: None,
                    enabled: true,
                    stale: Some(false),
                    impression_data: Some(false),
                    project: Some("default".into()),
                    strategies: Some(vec![
                        Strategy {
                            variants: Some(vec![StrategyVariant {
                                name: "test".into(),
                                payload: None,
                                weight: 7,
                                stickiness: Some("sticky-on-something".into()),
                            }]),
                            name: "standard".into(),
                            sort_order: Some(500),
                            segments: None,
                            constraints: None,
                            parameters: None,
                        },
                        Strategy {
                            variants: None,
                            name: "gradualRollout".into(),
                            sort_order: Some(100),
                            segments: None,
                            constraints: None,
                            parameters: None,
                        },
                    ]),
                    variants: None,
                },
                ClientFeature {
                    name: "feature_two_no_strats".into(),
                    feature_type: None,
                    dependencies: None,
                    description: None,
                    created_at: Some(Utc.with_ymd_and_hms(2022, 12, 5, 12, 31, 0).unwrap()),
                    last_seen_at: None,
                    enabled: true,
                    stale: None,
                    impression_data: None,
                    project: Some("default".into()),
                    strategies: None,
                    variants: None,
                },
                ClientFeature {
                    name: "feature_three".into(),
                    feature_type: Some("release".into()),
                    description: None,
                    dependencies: None,
                    created_at: None,
                    last_seen_at: None,
                    enabled: true,
                    stale: None,
                    impression_data: None,
                    project: Some("default".into()),
                    strategies: Some(vec![
                        Strategy {
                            name: "gradualRollout".to_string(),
                            sort_order: None,
                            segments: None,
                            variants: None,
                            constraints: Some(vec![Constraint {
                                context_name: "version".to_string(),
                                operator: Operator::SemverGt,
                                case_insensitive: false,
                                inverted: false,
                                values: None,
                                value: Some("1.5.0".into()),
                            }]),
                            parameters: None,
                        },
                        Strategy {
                            name: "".to_string(),
                            sort_order: None,
                            segments: None,
                            constraints: None,
                            parameters: None,
                            variants: None,
                        },
                    ]),
                    variants: None,
                },
            ],
            segments: None,
            query: None,
            meta: None,
        }
    }

    #[tokio::test]
    async fn response_includes_variant_stickiness_for_strategy_variants() {
        let features_cache = Arc::new(FeatureCache::default());
        let token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let app = test::init_service(
            App::new()
                .app_data(Data::from(features_cache.clone()))
                .app_data(Data::from(token_cache.clone()))
                .service(web::scope("/api/client").service(get_features)),
        )
        .await;

        features_cache.insert("production".into(), cached_client_features());
        let mut production_token = EdgeToken::try_from(
            "*:production.03fa5f506428fe80ed5640c351c7232e38940814d2923b08f5c05fa7".to_string(),
        )
        .unwrap();
        production_token.token_type = Some(TokenType::Client);
        production_token.status = TokenValidationStatus::Validated;
        token_cache.insert(production_token.token.clone(), production_token.clone());
        let req = make_features_request_with_token(production_token.clone()).await;
        let res: ClientFeatures = test::call_and_read_body_json(&app, req).await;

        assert_eq!(res.features.len(), cached_client_features().features.len());
        let strategy_variant_stickiness = res
            .features
            .iter()
            .find(|f| f.name == "feature_one")
            .unwrap()
            .strategies
            .clone()
            .unwrap()
            .iter()
            .find(|s| s.name == "standard")
            .unwrap()
            .variants
            .clone()
            .unwrap()
            .iter()
            .find(|v| v.name == "test")
            .unwrap()
            .stickiness
            .clone();
        assert!(strategy_variant_stickiness.is_some());
    }

    #[tokio::test]
    async fn register_endpoint_correctly_aggregates_applications() {
        let metrics_cache = Arc::new(MetricsCache::default());
        let our_app = ConnectVia {
            app_name: "test".into(),
            instance_id: Ulid::new().to_string(),
        };
        let app = test::init_service(
            App::new()
                .app_data(Data::new(our_app.clone()))
                .app_data(Data::from(metrics_cache.clone()))
                .service(web::scope("/api/client").service(register)),
        )
        .await;
        let mut client_app = ClientApplication::new("test_application", 15);
        client_app.instance_id = Some("test_instance".into());
        let req = make_register_post_request(client_app.clone()).await;
        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), actix_http::StatusCode::ACCEPTED);
        assert_eq!(metrics_cache.applications.len(), 1);
        let application_key = ApplicationKey {
            app_name: client_app.app_name.clone(),
            instance_id: client_app.instance_id.unwrap(),
        };
        let saved_app = metrics_cache
            .applications
            .get(&application_key)
            .unwrap()
            .value()
            .clone();
        assert_eq!(saved_app.app_name, client_app.app_name);
        assert_eq!(saved_app.connect_via, Some(vec![our_app]));
    }

    #[tokio::test]
    async fn bulk_metrics_endpoint_correctly_accepts_data() {
        let metrics_cache = MetricsCache::default();
        let connect_via = ConnectViaBuilder::default()
            .app_name("unleash-edge".into())
            .instance_id("test".into())
            .build()
            .unwrap();
        let app = test::init_service(
            App::new()
                .app_data(Data::new(connect_via))
                .app_data(web::Data::new(metrics_cache))
                .service(web::scope("/api/client").service(post_bulk_metrics)),
        )
        .await;
        let token = EdgeToken::from_str("*:development.somestring").unwrap();
        let req = make_bulk_metrics_post_request(Some(token.token.clone())).await;
        let call = test::call_service(&app, req).await;
        assert_eq!(call.status(), StatusCode::ACCEPTED);
    }
    #[tokio::test]
    async fn bulk_metrics_endpoint_correctly_refuses_metrics_without_auth_header() {
        let mut token = EdgeToken::from_str("*:development.somestring").unwrap();
        token.status = TokenValidationStatus::Validated;
        token.token_type = Some(TokenType::Client);
        let upstream_token_cache = Arc::new(DashMap::default());
        let upstream_features_cache = Arc::new(FeatureCache::default());
        let upstream_delta_cache_manager = Arc::new(DeltaCacheManager::new());
        let upstream_engine_cache = Arc::new(DashMap::default());
        upstream_token_cache.insert(token.token.clone(), token.clone());
        let srv = upstream_server(
            upstream_token_cache,
            upstream_features_cache,
            upstream_delta_cache_manager,
            upstream_engine_cache,
        )
        .await;
        let req = reqwest::Client::new();
        let status = req
            .post(srv.url("/api/client/metrics/bulk").as_str())
            .body(
                serde_json::to_string(&crate::types::BatchMetricsRequestBody {
                    applications: vec![],
                    metrics: vec![],
                    impact_metrics: None,
                })
                .unwrap(),
            )
            .send()
            .await;
        assert!(status.is_ok());
        assert_eq!(
            status.unwrap().status().as_u16(),
            StatusCode::FORBIDDEN.as_u16()
        );
        let client = UnleashClient::new(srv.url("/").as_str(), None).unwrap();
        let successful = client
            .send_bulk_metrics_to_client_endpoint(MetricsBatch::default(), &token.token)
            .await;
        assert!(successful.is_ok());
    }

    #[tokio::test]
    async fn bulk_metrics_endpoint_correctly_refuses_metrics_with_frontend_token() {
        let mut frontend_token = EdgeToken::from_str("*:development.frontend").unwrap();
        frontend_token.status = TokenValidationStatus::Validated;
        frontend_token.token_type = Some(TokenType::Frontend);
        let upstream_token_cache = Arc::new(DashMap::default());
        let upstream_features_cache = Arc::new(FeatureCache::default());
        let upstream_delta_cache_manager = Arc::new(DeltaCacheManager::new());
        let upstream_engine_cache = Arc::new(DashMap::default());
        upstream_token_cache.insert(frontend_token.token.clone(), frontend_token.clone());
        let srv = upstream_server(
            upstream_token_cache,
            upstream_features_cache,
            upstream_delta_cache_manager,
            upstream_engine_cache,
        )
        .await;
        let client = UnleashClient::new(srv.url("/").as_str(), None).unwrap();
        let status = client
            .send_bulk_metrics_to_client_endpoint(MetricsBatch::default(), &frontend_token.token)
            .await;
        assert_eq!(status.expect_err("").status_code(), StatusCode::FORBIDDEN);
    }
    #[tokio::test]
    async fn register_endpoint_returns_version_header() {
        let metrics_cache = Arc::new(MetricsCache::default());
        let our_app = ConnectVia {
            app_name: "test".into(),
            instance_id: Ulid::new().to_string(),
        };
        let app = test::init_service(
            App::new()
                .app_data(Data::new(our_app.clone()))
                .app_data(Data::from(metrics_cache.clone()))
                .service(web::scope("/api/client").service(register)),
        )
        .await;
        let mut client_app = ClientApplication::new("test_application", 15);
        client_app.instance_id = Some("test_instance".into());
        let req = make_register_post_request(client_app.clone()).await;
        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), StatusCode::ACCEPTED);
        assert_eq!(
            res.headers().get("X-Edge-Version").unwrap(),
            types::EDGE_VERSION
        );
    }

    #[tokio::test]
    async fn client_features_endpoint_correctly_returns_cached_features() {
        let features_cache = Arc::new(FeatureCache::default());
        let token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let app = test::init_service(
            App::new()
                .app_data(Data::from(features_cache.clone()))
                .app_data(Data::from(token_cache.clone()))
                .service(web::scope("/api/client").service(get_features)),
        )
        .await;
        let client_features = cached_client_features();
        let example_features = features_from_disk("../examples/features.json");
        features_cache.insert("development".into(), client_features.clone());
        features_cache.insert("production".into(), example_features.clone());
        let mut token = EdgeToken::try_from(
            "*:development.03fa5f506428fe80ed5640c351c7232e38940814d2923b08f5c05fa7".to_string(),
        )
        .unwrap();
        token.token_type = Some(TokenType::Client);
        token.status = TokenValidationStatus::Validated;
        token_cache.insert(token.token.clone(), token.clone());
        let req = make_features_request_with_token(token.clone()).await;
        let res: ClientFeatures = test::call_and_read_body_json(&app, req).await;
        assert_eq!(res.features, client_features.features);
        let mut production_token = EdgeToken::try_from(
            "*:production.03fa5f506428fe80ed5640c351c7232e38940814d2923b08f5c05fa7".to_string(),
        )
        .unwrap();
        production_token.token_type = Some(TokenType::Client);
        production_token.status = TokenValidationStatus::Validated;
        token_cache.insert(production_token.token.clone(), production_token.clone());
        let req = make_features_request_with_token(production_token.clone()).await;
        let res: ClientFeatures = test::call_and_read_body_json(&app, req).await;
        assert_eq!(res.features.len(), example_features.features.len());
    }

    #[tokio::test]
    async fn post_request_to_client_features_does_the_same_as_get_when_mounted() {
        let features_cache = Arc::new(FeatureCache::default());
        let token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let app = test::init_service(
            App::new()
                .app_data(Data::from(features_cache.clone()))
                .app_data(Data::from(token_cache.clone()))
                .service(
                    web::scope("/api/client")
                        .service(get_features)
                        .service(post_features),
                ),
        )
        .await;
        let client_features = cached_client_features();
        let example_features = features_from_disk("../examples/features.json");
        features_cache.insert("development".into(), client_features.clone());
        features_cache.insert("production".into(), example_features.clone());
        let mut token = EdgeToken::try_from(
            "*:development.03fa5f506428fe80ed5640c351c7232e38940814d2923b08f5c05fa7".to_string(),
        )
        .unwrap();
        token.token_type = Some(TokenType::Client);
        token.status = TokenValidationStatus::Validated;
        token_cache.insert(token.token.clone(), token.clone());
        let req = make_features_request_with_token(token.clone()).await;
        let res: ClientFeatures = test::call_and_read_body_json(&app, req).await;
        assert_eq!(res.features, client_features.features);
        let mut production_token = EdgeToken::try_from(
            "*:production.03fa5f506428fe80ed5640c351c7232e38940814d2923b08f5c05fa7".to_string(),
        )
        .unwrap();
        production_token.token_type = Some(TokenType::Client);
        production_token.status = TokenValidationStatus::Validated;
        token_cache.insert(production_token.token.clone(), production_token.clone());

        let post_req = test::TestRequest::post()
            .uri("/api/client/features")
            .insert_header(("Authorization", production_token.clone().token))
            .insert_header(ContentType::json())
            .to_request();

        let get_req = make_features_request_with_token(production_token.clone()).await;
        let get_res: ClientFeatures = test::call_and_read_body_json(&app, get_req).await;
        let post_res: ClientFeatures = test::call_and_read_body_json(&app, post_req).await;

        assert_eq!(get_res.features, post_res.features)
    }

    #[tokio::test]
    async fn client_features_endpoint_filters_on_project_access_in_token() {
        let features_cache = Arc::new(FeatureCache::default());
        let token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let app = test::init_service(
            App::new()
                .app_data(Data::from(features_cache.clone()))
                .app_data(Data::from(token_cache.clone()))
                .service(web::scope("/api/client").service(get_features)),
        )
        .await;
        let mut edge_token = EdgeToken::try_from(
            "demo-app:production.03fa5f506428fe80ed5640c351c7232e38940814d2923b08f5c05fa7"
                .to_string(),
        )
        .unwrap();
        edge_token.token_type = Some(TokenType::Client);
        token_cache.insert(edge_token.token.clone(), edge_token.clone());
        let example_features = features_from_disk("../examples/features.json");
        features_cache.insert("production".into(), example_features.clone());
        let req = make_features_request_with_token(edge_token.clone()).await;
        let res: ClientFeatures = test::call_and_read_body_json(&app, req).await;
        assert_eq!(res.features.len(), 5);
        assert!(
            res.features
                .iter()
                .all(|t| t.project == Some("demo-app".into()))
        );
    }

    #[tokio::test]
    async fn client_features_endpoint_filters_when_multiple_projects_in_token() {
        let features_cache = Arc::new(FeatureCache::default());
        let token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let app = test::init_service(
            App::new()
                .app_data(Data::from(features_cache.clone()))
                .app_data(Data::from(token_cache.clone()))
                .service(web::scope("/api/client").service(get_features)),
        )
        .await;
        let mut token =
            EdgeToken::try_from("[]:production.puff_the_magic_dragon".to_string()).unwrap();
        token.projects = vec!["dx".into(), "eg".into(), "unleash-cloud".into()];
        token.status = TokenValidationStatus::Validated;
        token.token_type = Some(TokenType::Client);
        token_cache.insert(token.token.clone(), token.clone());
        let example_features = features_from_disk("../examples/hostedexample.json");
        features_cache.insert("production".into(), example_features.clone());
        let req = make_features_request_with_token(token.clone()).await;
        let res: ClientFeatures = test::call_and_read_body_json(&app, req).await;
        assert_eq!(res.features.len(), 24);
        assert!(
            res.features
                .iter()
                .all(|f| token.projects.contains(&f.project.clone().unwrap()))
        );
    }

    #[tokio::test]
    async fn client_features_endpoint_filters_correctly_when_token_has_access_to_multiple_projects()
    {
        let features_cache = Arc::new(FeatureCache::default());
        let token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let app = test::init_service(
            App::new()
                .app_data(Data::from(features_cache.clone()))
                .app_data(Data::from(token_cache.clone()))
                .service(web::scope("/api/client").service(get_features)),
        )
        .await;

        let mut token_a =
            EdgeToken::try_from("[]:production.puff_the_magic_dragon".to_string()).unwrap();
        token_a.projects = vec!["dx".into(), "eg".into()];
        token_a.status = TokenValidationStatus::Validated;
        token_a.token_type = Some(TokenType::Client);
        token_cache.insert(token_a.token.clone(), token_a.clone());

        let mut token_b =
            EdgeToken::try_from("[]:production.biff_the_magic_flagon".to_string()).unwrap();
        token_b.projects = vec!["unleash-cloud".into()];
        token_b.status = TokenValidationStatus::Validated;
        token_b.token_type = Some(TokenType::Client);
        token_cache.insert(token_b.token.clone(), token_b.clone());

        let example_features = features_from_disk("../examples/hostedexample.json");
        features_cache.insert("production".into(), example_features.clone());

        let req_1 = make_features_request_with_token(token_a.clone()).await;
        let res_1: ClientFeatures = test::call_and_read_body_json(&app, req_1).await;
        assert!(
            res_1
                .features
                .iter()
                .all(|f| token_a.projects.contains(&f.project.clone().unwrap()))
        );

        let req_2 = make_features_request_with_token(token_b.clone()).await;
        let res_2: ClientFeatures = test::call_and_read_body_json(&app, req_2).await;
        assert!(
            res_2
                .features
                .iter()
                .all(|f| token_b.projects.contains(&f.project.clone().unwrap()))
        );

        let req_3 = make_features_request_with_token(token_a.clone()).await;
        let res_3: ClientFeatures = test::call_and_read_body_json(&app, req_3).await;
        assert!(
            res_3
                .features
                .iter()
                .all(|f| token_a.projects.contains(&f.project.clone().unwrap()))
        );
    }

    #[tokio::test]
    async fn when_running_in_offline_mode_with_proxy_key_should_not_filter_features() {
        let features_cache = Arc::new(FeatureCache::default());
        let token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let app = test::init_service(
            App::new()
                .app_data(Data::from(features_cache.clone()))
                .app_data(Data::from(token_cache.clone()))
                .app_data(Data::new(crate::cli::EdgeMode::Offline(OfflineArgs {
                    bootstrap_file: Some(PathBuf::from("../examples/features.json")),
                    tokens: vec!["secret_123".into()],
                    client_tokens: vec![],
                    frontend_tokens: vec![],
                    reload_interval: 0,
                })))
                .service(web::scope("/api/client").service(get_features)),
        )
        .await;
        let token = EdgeToken::offline_token("secret-123");
        token_cache.insert(token.token.clone(), token.clone());
        let example_features = features_from_disk("../examples/features.json");
        features_cache.insert(token.token.clone(), example_features.clone());
        let req = make_features_request_with_token(token.clone()).await;
        let res: ClientFeatures = test::call_and_read_body_json(&app, req).await;
        assert_eq!(res.features.len(), example_features.features.len());
    }

    #[tokio::test]
    async fn calling_client_features_endpoint_with_new_token_hydrates_from_upstream_when_dynamic() {
        let upstream_features_cache = Arc::new(FeatureCache::default());
        let upstream_token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let delta_cache_manager: Arc<DeltaCacheManager> = Arc::new(DeltaCacheManager::new());
        let upstream_engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let server = upstream_server(
            upstream_token_cache.clone(),
            upstream_features_cache.clone(),
            delta_cache_manager.clone(),
            upstream_engine_cache.clone(),
        )
        .await;
        let upstream_features = features_from_disk("../examples/hostedexample.json");
        let mut upstream_known_token = EdgeToken::from_str("dx:development.secret123").unwrap();
        upstream_known_token.status = TokenValidationStatus::Validated;
        upstream_known_token.token_type = Some(TokenType::Client);
        upstream_token_cache.insert(
            upstream_known_token.token.clone(),
            upstream_known_token.clone(),
        );
        upstream_features_cache.insert(cache_key(&upstream_known_token), upstream_features.clone());
        let unleash_client = Arc::new(UnleashClient::new(server.url("/").as_str(), None).unwrap());
        let features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
        let delta_cache_manager: Arc<DeltaCacheManager> = Arc::new(DeltaCacheManager::new());
        let token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let feature_refresher = Arc::new(FeatureRefresher {
            unleash_client: unleash_client.clone(),
            tokens_to_refresh: Arc::new(Default::default()),
            features_cache: features_cache.clone(),
            delta_cache_manager: delta_cache_manager.clone(),
            engine_cache: engine_cache.clone(),
            refresh_interval: Duration::seconds(6000),
            persistence: None,
            strict: false,
            streaming: false,
            client_meta_information: ClientMetaInformation::test_config(),
            delta: false,
            delta_diff: false,
        });
        let token_validator = Arc::new(TokenValidator {
            unleash_client: unleash_client.clone(),
            token_cache: token_cache.clone(),
            persistence: None,
        });
        let local_app = test::init_service(
            App::new()
                .app_data(Data::from(token_validator.clone()))
                .app_data(Data::from(features_cache.clone()))
                .app_data(Data::from(engine_cache.clone()))
                .app_data(Data::from(token_cache.clone()))
                .app_data(Data::from(feature_refresher.clone()))
                .wrap(middleware::as_async_middleware::as_async_middleware(
                    middleware::validate_token::validate_token,
                ))
                .service(web::scope("/api").configure(configure_client_api)),
        )
        .await;
        let req = test::TestRequest::get()
            .uri("/api/client/features")
            .insert_header(ContentType::json())
            .insert_header(("Authorization", upstream_known_token.token.clone()))
            .to_request();
        let res = test::call_service(&local_app, req).await;
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn calling_client_features_endpoint_with_new_token_does_not_hydrate_when_strict() {
        let upstream_features_cache = Arc::new(FeatureCache::default());
        let upstream_token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let upstream_delta_cache_manager: Arc<DeltaCacheManager> =
            Arc::new(DeltaCacheManager::new());
        let upstream_engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let server = upstream_server(
            upstream_token_cache.clone(),
            upstream_features_cache.clone(),
            upstream_delta_cache_manager.clone(),
            upstream_engine_cache.clone(),
        )
        .await;
        let upstream_features = features_from_disk("../examples/hostedexample.json");
        let mut upstream_known_token = EdgeToken::from_str("dx:development.secret123").unwrap();
        upstream_known_token.status = TokenValidationStatus::Validated;
        upstream_known_token.token_type = Some(TokenType::Client);
        upstream_token_cache.insert(
            upstream_known_token.token.clone(),
            upstream_known_token.clone(),
        );
        upstream_features_cache.insert(cache_key(&upstream_known_token), upstream_features.clone());
        let unleash_client = Arc::new(UnleashClient::new(server.url("/").as_str(), None).unwrap());
        let features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
        let token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let feature_refresher = Arc::new(FeatureRefresher {
            unleash_client: unleash_client.clone(),
            features_cache: features_cache.clone(),
            engine_cache: engine_cache.clone(),
            refresh_interval: Duration::seconds(6000),
            ..Default::default()
        });
        let token_validator = Arc::new(TokenValidator {
            unleash_client: unleash_client.clone(),
            token_cache: token_cache.clone(),
            persistence: None,
        });
        let local_app = test::init_service(
            App::new()
                .app_data(Data::from(token_validator.clone()))
                .app_data(Data::from(features_cache.clone()))
                .app_data(Data::from(engine_cache.clone()))
                .app_data(Data::from(token_cache.clone()))
                .app_data(Data::from(feature_refresher.clone()))
                .wrap(middleware::as_async_middleware::as_async_middleware(
                    middleware::validate_token::validate_token,
                ))
                .service(web::scope("/api").configure(configure_client_api)),
        )
        .await;
        let req = test::TestRequest::get()
            .uri("/api/client/features")
            .insert_header(ContentType::json())
            .insert_header(("Authorization", upstream_known_token.token.clone()))
            .to_request();
        let res = test::call_service(&local_app, req).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    pub async fn gets_feature_by_name() {
        let features_cache = Arc::new(FeatureCache::default());
        let token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let features = features_from_disk("../examples/hostedexample.json");
        let mut dx_token = EdgeToken::from_str("dx:development.secret123").unwrap();
        dx_token.status = TokenValidationStatus::Validated;
        dx_token.token_type = Some(TokenType::Client);
        token_cache.insert(dx_token.token.clone(), dx_token.clone());
        features_cache.insert(cache_key(&dx_token), features.clone());
        let local_app = test::init_service(
            App::new()
                .app_data(Data::from(features_cache.clone()))
                .app_data(Data::from(engine_cache.clone()))
                .app_data(Data::from(token_cache.clone()))
                .wrap(middleware::as_async_middleware::as_async_middleware(
                    middleware::validate_token::validate_token,
                ))
                .service(web::scope("/api").configure(configure_client_api)),
        )
        .await;
        let desired_toggle = "projectStatusApi";
        let request = test::TestRequest::get()
            .uri(format!("/api/client/features/{desired_toggle}").as_str())
            .insert_header(ContentType::json())
            .insert_header(("Authorization", dx_token.token.clone()))
            .to_request();
        let result: ClientFeature = test::call_and_read_body_json(&local_app, request).await;
        assert_eq!(result.name, desired_toggle);
    }

    #[tokio::test]
    pub async fn token_with_no_access_to_named_feature_yields_404() {
        let features_cache = Arc::new(FeatureCache::default());
        let token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let features = features_from_disk("../examples/hostedexample.json");
        let mut dx_token = EdgeToken::from_str("dx:development.secret123").unwrap();
        dx_token.status = TokenValidationStatus::Validated;
        dx_token.token_type = Some(TokenType::Client);
        token_cache.insert(dx_token.token.clone(), dx_token.clone());
        features_cache.insert(cache_key(&dx_token), features.clone());
        let local_app = test::init_service(
            App::new()
                .app_data(Data::from(features_cache.clone()))
                .app_data(Data::from(engine_cache.clone()))
                .app_data(Data::from(token_cache.clone()))
                .wrap(middleware::as_async_middleware::as_async_middleware(
                    middleware::validate_token::validate_token,
                ))
                .service(web::scope("/api").configure(configure_client_api)),
        )
        .await;
        let desired_toggle = "serviceAccounts";
        let request = test::TestRequest::get()
            .uri(format!("/api/client/features/{desired_toggle}").as_str())
            .insert_header(ContentType::json())
            .insert_header(("Authorization", dx_token.token.clone()))
            .to_request();
        let result = test::call_service(&local_app, request).await;
        assert_eq!(result.status(), StatusCode::NOT_FOUND);
    }
    #[tokio::test]
    pub async fn still_subsumes_tokens_after_moving_registration_to_initial_hydration_when_dynamic()
    {
        let upstream_features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
        let upstream_token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let upstream_delta_cache_manager: Arc<DeltaCacheManager> =
            Arc::new(DeltaCacheManager::new());
        let upstream_engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let server = upstream_server(
            upstream_token_cache.clone(),
            upstream_features_cache.clone(),
            upstream_delta_cache_manager.clone(),
            upstream_engine_cache.clone(),
        )
        .await;
        let upstream_features = features_from_disk("../examples/hostedexample.json");
        let mut upstream_dx_token = EdgeToken::from_str("dx:development.secret123").unwrap();
        upstream_dx_token.status = TokenValidationStatus::Validated;
        upstream_dx_token.token_type = Some(TokenType::Client);
        upstream_token_cache.insert(upstream_dx_token.token.clone(), upstream_dx_token.clone());
        let mut upstream_eg_token = EdgeToken::from_str("eg:development.secret321").unwrap();
        upstream_eg_token.status = TokenValidationStatus::Validated;
        upstream_eg_token.token_type = Some(TokenType::Client);
        upstream_token_cache.insert(upstream_eg_token.token.clone(), upstream_eg_token.clone());
        upstream_features_cache.insert(cache_key(&upstream_dx_token), upstream_features.clone());
        let unleash_client = Arc::new(UnleashClient::new(server.url("/").as_str(), None).unwrap());
        let features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
        let token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let feature_refresher = Arc::new(FeatureRefresher {
            unleash_client: unleash_client.clone(),
            features_cache: features_cache.clone(),
            engine_cache: engine_cache.clone(),
            refresh_interval: Duration::seconds(6000),
            strict: false,
            ..Default::default()
        });
        let token_validator = Arc::new(TokenValidator {
            unleash_client: unleash_client.clone(),
            token_cache: token_cache.clone(),
            persistence: None,
        });
        let local_app = test::init_service(
            App::new()
                .app_data(Data::from(token_validator.clone()))
                .app_data(Data::from(features_cache.clone()))
                .app_data(Data::from(engine_cache.clone()))
                .app_data(Data::from(token_cache.clone()))
                .app_data(Data::from(feature_refresher.clone()))
                .wrap(middleware::as_async_middleware::as_async_middleware(
                    middleware::validate_token::validate_token,
                ))
                .service(web::scope("/api").configure(configure_client_api)),
        )
        .await;
        let dx_req = test::TestRequest::get()
            .uri("/api/client/features")
            .insert_header(ContentType::json())
            .insert_header(("Authorization", upstream_dx_token.token.clone()))
            .to_request();
        let res: ClientFeatures = test::call_and_read_body_json(&local_app, dx_req).await;
        assert!(!res.features.is_empty());
        let eg_req = test::TestRequest::get()
            .uri("/api/client/features")
            .insert_header(ContentType::json())
            .insert_header(("Authorization", upstream_eg_token.token.clone()))
            .to_request();
        let eg_res: ClientFeatures = test::call_and_read_body_json(&local_app, eg_req).await;
        assert!(!eg_res.features.is_empty());
        assert_eq!(feature_refresher.tokens_to_refresh.len(), 2);
        assert_eq!(features_cache.len(), 1);
    }

    #[tokio::test]
    pub async fn can_filter_features_list_by_name_prefix() {
        let features_cache = Arc::new(FeatureCache::default());
        let token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let features = features_from_disk("../examples/hostedexample.json");
        let mut dx_token = EdgeToken::from_str("dx:development.secret123").unwrap();
        dx_token.status = TokenValidationStatus::Validated;
        dx_token.token_type = Some(TokenType::Client);
        token_cache.insert(dx_token.token.clone(), dx_token.clone());
        features_cache.insert(cache_key(&dx_token), features.clone());
        let local_app = test::init_service(
            App::new()
                .app_data(Data::from(features_cache.clone()))
                .app_data(Data::from(engine_cache.clone()))
                .app_data(Data::from(token_cache.clone()))
                .wrap(middleware::as_async_middleware::as_async_middleware(
                    middleware::validate_token::validate_token,
                ))
                .service(web::scope("/api").configure(configure_client_api)),
        )
        .await;
        let request = test::TestRequest::get()
            .uri("/api/client/features?namePrefix=embed")
            .insert_header(ContentType::json())
            .insert_header(("Authorization", dx_token.token.clone()))
            .to_request();
        let result: ClientFeatures = test::call_and_read_body_json(&local_app, request).await;
        assert_eq!(result.features.len(), 2);
        assert_eq!(result.query.unwrap().name_prefix.unwrap(), "embed");
    }

    #[tokio::test]
    pub async fn only_gets_correct_feature_by_name() {
        let features_cache = Arc::new(FeatureCache::default());
        let token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let features = ClientFeatures {
            version: 2,
            query: None,
            features: vec![
                ClientFeature {
                    name: "edge-flag-1".into(),
                    feature_type: None,
                    dependencies: None,
                    description: None,
                    created_at: None,
                    last_seen_at: None,
                    enabled: true,
                    stale: None,
                    impression_data: None,
                    project: Some("dx".into()),
                    strategies: None,
                    variants: None,
                },
                ClientFeature {
                    name: "edge-flag-3".into(),
                    feature_type: None,
                    dependencies: None,
                    description: None,
                    created_at: None,
                    last_seen_at: None,
                    enabled: true,
                    stale: None,
                    impression_data: None,
                    project: Some("eg".into()),
                    strategies: None,
                    variants: None,
                },
            ],
            segments: None,
            meta: None,
        };
        let mut dx_token = EdgeToken::from_str("dx:development.secret123").unwrap();
        dx_token.status = TokenValidationStatus::Validated;
        dx_token.token_type = Some(TokenType::Client);
        let mut eg_token = EdgeToken::from_str("eg:development.secret123").unwrap();
        eg_token.status = TokenValidationStatus::Validated;
        eg_token.token_type = Some(TokenType::Client);
        token_cache.insert(dx_token.token.clone(), dx_token.clone());
        token_cache.insert(eg_token.token.clone(), eg_token.clone());
        features_cache.insert(cache_key(&dx_token), features.clone());
        let local_app = test::init_service(
            App::new()
                .app_data(Data::from(features_cache.clone()))
                .app_data(Data::from(engine_cache.clone()))
                .app_data(Data::from(token_cache.clone()))
                .wrap(middleware::as_async_middleware::as_async_middleware(
                    middleware::validate_token::validate_token,
                ))
                .service(web::scope("/api").configure(configure_client_api)),
        )
        .await;
        let successful_request = test::TestRequest::get()
            .uri("/api/client/features/edge-flag-3")
            .insert_header(ContentType::json())
            .insert_header(("Authorization", eg_token.token.clone()))
            .to_request();
        let res = test::call_service(&local_app, successful_request).await;
        assert_eq!(res.status(), StatusCode::OK);
        let request = test::TestRequest::get()
            .uri("/api/client/features/edge-flag-3")
            .insert_header(ContentType::json())
            .insert_header(("Authorization", dx_token.token.clone()))
            .to_request();
        let res = test::call_service(&local_app, request).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn client_features_endpoint_works_with_overridden_token_header() {
        let features_cache = Arc::new(FeatureCache::default());
        let token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let token_header = AuthHeaders::from_str("NeedsToBeTested").unwrap();
        let app = test::init_service(
            App::new()
                .app_data(Data::from(features_cache.clone()))
                .app_data(Data::from(token_cache.clone()))
                .app_data(Data::new(token_header.clone()))
                .service(web::scope("/api/client").service(get_features)),
        )
        .await;
        let client_features = cached_client_features();
        let example_features = features_from_disk("../examples/features.json");
        features_cache.insert("development".into(), client_features.clone());
        features_cache.insert("production".into(), example_features.clone());
        let mut production_token = EdgeToken::try_from(
            "*:production.03fa5f506428fe80ed5640c351c7232e38940814d2923b08f5c05fa7".to_string(),
        )
        .unwrap();
        production_token.token_type = Some(TokenType::Client);
        production_token.status = TokenValidationStatus::Validated;
        token_cache.insert(production_token.token.clone(), production_token.clone());

        let request = test::TestRequest::get()
            .uri("/api/client/features")
            .insert_header(ContentType::json())
            .insert_header(("NeedsToBeTested", production_token.token.clone()))
            .to_request();
        let res = test::call_service(&app, request).await;
        assert_eq!(res.status(), StatusCode::OK);
        let request = test::TestRequest::get()
            .uri("/api/client/features")
            .insert_header(ContentType::json())
            .insert_header(("ShouldNotWork", production_token.token.clone()))
            .to_request();
        let res = test::call_service(&app, request).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    async fn setup_delta_test(
        initial_event_id: u32,
    ) -> (
        Arc<FeatureRefresher>,
        Arc<DashMap<String, EdgeToken>>,
        EdgeToken,
        DeltaHydrationEvent,
        impl actix_web::dev::Service<
            actix_http::Request,
            Response = actix_web::dev::ServiceResponse,
            Error = actix_web::Error,
        >,
    ) {
        let unleash_client = Arc::new(UnleashClient::new("http://localhost:9999/", None).unwrap());
        let delta_cache_manager = Arc::new(DeltaCacheManager::default());
        let token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());

        let mut token = EdgeToken::try_from(
            "dx:development.03fa5f506428fe80ed5640c351c7232e38940814d2923b08f5c05fa7".to_string(),
        )
        .unwrap();
        token.token_type = Some(TokenType::Client);
        token.status = TokenValidationStatus::Validated;
        token_cache.insert(token.token.clone(), token.clone());

        let feature_refresher = Arc::new(FeatureRefresher {
            unleash_client: unleash_client.clone(),
            delta_cache_manager: delta_cache_manager.clone(),
            tokens_to_refresh: Arc::new(Default::default()),
            refresh_interval: Duration::seconds(0),
            strict: false,
            delta: true,
            ..Default::default()
        });

        let delta_hydration_event = DeltaHydrationEvent {
            event_id: initial_event_id,
            features: vec![ClientFeature {
                name: "feature1".to_string(),
                project: Some("dx".to_string()),
                enabled: false,
                ..Default::default()
            }],
            segments: vec![],
        };

        let app = test::init_service(
            App::new()
                .app_data(Data::from(feature_refresher.clone()))
                .app_data(Data::from(token_cache.clone()))
                .service(web::scope("/api/client").service(get_delta)),
        )
        .await;

        delta_cache_manager.insert_cache(
            "development",
            DeltaCache::new(delta_hydration_event.clone(), 10),
        );

        (
            feature_refresher,
            token_cache,
            token,
            delta_hydration_event,
            app,
        )
    }

    #[tokio::test]
    async fn test_delta_endpoint_returns_hydration_event() {
        let (_, _, token, delta_hydration_event, app) = setup_delta_test(10).await;

        let req = make_delta_request_with_token(token.clone()).await;
        let res: ClientFeaturesDelta = test::call_and_read_body_json(&app, req).await;

        assert_eq!(
            res.events.first().unwrap(),
            &DeltaEvent::Hydration {
                event_id: delta_hydration_event.event_id,
                features: delta_hydration_event.features.clone(),
                segments: delta_hydration_event.segments.clone()
            }
        );
    }

    #[tokio::test]
    async fn test_delta_endpoint_returns_not_modified_for_matching_etag() {
        let (_, _, token, _, app) = setup_delta_test(10).await;

        let res = test::call_service(
            &app,
            make_delta_request_with_token_and_etag(token.clone(), "10").await,
        )
        .await;

        assert_eq!(res.status(), StatusCode::NOT_MODIFIED);
    }

    #[tokio::test]
    async fn test_delta_endpoint_returns_not_modified_for_newer_etag() {
        let (_, _, token, _, app) = setup_delta_test(10).await;

        let res = test::call_service(
            &app,
            make_delta_request_with_token_and_etag(token.clone(), "11").await,
        )
        .await;

        assert_eq!(res.status(), StatusCode::NOT_MODIFIED);
    }

    #[tokio::test]
    async fn test_delta_endpoint_returns_delta_events_after_update() {
        let (feature_refresher, _, token, _, app) = setup_delta_test(10).await;

        let delta_event = DeltaEvent::FeatureRemoved {
            event_id: 11,
            feature_name: "test".to_string(),
            project: "dx".to_string(),
        };

        feature_refresher
            .delta_cache_manager
            .update_cache("development", &vec![delta_event.clone()]);

        let res: ClientFeaturesDelta = test::call_and_read_body_json(
            &app,
            make_delta_request_with_token_and_etag(token.clone(), "10").await,
        )
        .await;

        assert_eq!(res.events.first().unwrap(), &delta_event);
        assert_eq!(res.events.len(), 1);

        let res = test::call_service(
            &app,
            make_delta_request_with_token_and_etag(token.clone(), "11").await,
        )
        .await;

        assert_eq!(res.status(), StatusCode::NOT_MODIFIED);

        let delta_event = DeltaEvent::SegmentRemoved {
            event_id: 12,
            segment_id: 1,
        };

        feature_refresher
            .delta_cache_manager
            .update_cache("development", &vec![delta_event.clone()]);

        let res = test::call_service(
            &app,
            make_delta_request_with_token_and_etag(token.clone(), "12").await,
        )
        .await;

        assert_eq!(res.status(), StatusCode::NOT_MODIFIED);
    }

    #[tokio::test]
    async fn test_delta_endpoint_returns_hydration_event_when_unknown_etag_lower_than_current_event_id()
     {
        let (_, _, token, delta_hydration_event, app) = setup_delta_test(10).await;

        let res: ClientFeaturesDelta = test::call_and_read_body_json(
            &app,
            make_delta_request_with_token_and_etag(token.clone(), "8").await,
        )
        .await;

        assert_eq!(
            res.events.first().unwrap(),
            &DeltaEvent::Hydration {
                event_id: delta_hydration_event.event_id,
                features: delta_hydration_event.features.clone(),
                segments: delta_hydration_event.segments.clone()
            }
        );
    }
}
