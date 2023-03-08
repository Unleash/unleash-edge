use crate::error::EdgeError;
use crate::metrics::client_metrics::{ApplicationKey, MetricsCache};
use crate::tokens::cache_key;
use crate::types::{EdgeJsonResult, EdgeResult, EdgeToken, ProjectFilter};
use actix_web::web::{self, Json};
use actix_web::{get, post, HttpResponse};
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
    token_cache: web::Data<DashMap<String, EdgeToken>>,
) -> EdgeJsonResult<ClientFeatures> {
    let validated_token = token_cache
        .get(&edge_token.token)
        .map(|e| e.value().clone())
        .ok_or(EdgeError::AuthorizationDenied)?;
    features_cache
        .get(&cache_key(&edge_token))
        .map(|features| features.clone())
        .map(|client_features| ClientFeatures {
            features: client_features
                .features
                .filter_by_projects(&validated_token),
            ..client_features
        })
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
    client_application: Json<ClientApplication>,
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

    use std::io::BufReader;
    use std::path::PathBuf;
    use std::{collections::HashMap, fs, sync::Arc};

    use crate::metrics::client_metrics::MetricsKey;
    use crate::types::{TokenType, TokenValidationStatus};

    use super::*;

    use actix_http::Request;
    use actix_web::{
        http::header::ContentType,
        test,
        web::{self, Data},
        App,
    };
    use chrono::{DateTime, TimeZone, Utc};
    use maplit::hashmap;
    use reqwest::StatusCode;
    use ulid::Ulid;
    use unleash_types::client_features::{ClientFeature, Constraint, Operator, Strategy};
    use unleash_types::client_metrics::{ClientMetricsEnv, MetricBucket, ToggleStats};

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
                .service(web::scope("/api").service(metrics)),
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

    fn cached_client_features() -> ClientFeatures {
        ClientFeatures {
            version: 2,
            features: vec![
                ClientFeature {
                    name: "feature_one".into(),
                    feature_type: Some("release".into()),
                    description: Some("test feature".into()),
                    created_at: Some(Utc::now()),
                    last_seen_at: None,
                    enabled: true,
                    stale: Some(false),
                    impression_data: Some(false),
                    project: Some("default".into()),
                    strategies: Some(vec![
                        Strategy {
                            name: "standard".into(),
                            sort_order: Some(500),
                            segments: None,
                            constraints: None,
                            parameters: None,
                        },
                        Strategy {
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
                        },
                    ]),
                    variants: None,
                },
            ],
            segments: None,
            query: None,
        }
    }

    fn features_from_disk(path: PathBuf) -> ClientFeatures {
        let file = fs::File::open(path).unwrap();
        let reader = BufReader::new(file);
        serde_json::from_reader(reader).unwrap()
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
                .service(web::scope("/api").service(super::register)),
        )
        .await;
        let mut client_app = ClientApplication::new("test_application", 15);
        client_app.instance_id = Some("test_instance".into());
        let req = make_register_post_request(client_app.clone()).await;
        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), StatusCode::ACCEPTED);
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
    async fn client_features_endpoint_correctly_returns_cached_features() {
        let features_cache: Arc<DashMap<String, ClientFeatures>> = Arc::new(DashMap::default());
        let token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let app = test::init_service(
            App::new()
                .app_data(Data::from(features_cache.clone()))
                .app_data(Data::from(token_cache.clone()))
                .service(web::scope("/api").service(features)),
        )
        .await;
        let client_features = cached_client_features();
        let example_features = features_from_disk(PathBuf::from("../examples/features.json"));
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
    async fn client_features_endpoint_filters_on_project_access_in_token() {
        let features_cache: Arc<DashMap<String, ClientFeatures>> = Arc::new(DashMap::default());
        let token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let app = test::init_service(
            App::new()
                .app_data(Data::from(features_cache.clone()))
                .app_data(Data::from(token_cache.clone()))
                .service(web::scope("/api").service(features)),
        )
        .await;
        let mut edge_token = EdgeToken::try_from(
            "demo-app:production.03fa5f506428fe80ed5640c351c7232e38940814d2923b08f5c05fa7"
                .to_string(),
        )
        .unwrap();
        edge_token.token_type = Some(TokenType::Client);
        token_cache.insert(edge_token.token.clone(), edge_token.clone());
        let example_features = features_from_disk(PathBuf::from("../examples/features.json"));
        features_cache.insert("production".into(), example_features.clone());
        let req = make_features_request_with_token(edge_token.clone()).await;
        let res: ClientFeatures = test::call_and_read_body_json(&app, req).await;
        assert_eq!(res.features.len(), 5);
        assert!(res
            .features
            .iter()
            .all(|t| t.project == Some("demo-app".into())));
    }

    #[tokio::test]
    async fn client_features_endpoint_filters_when_multiple_projects_in_token() {
        let features_cache: Arc<DashMap<String, ClientFeatures>> = Arc::new(DashMap::default());
        let token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let app = test::init_service(
            App::new()
                .app_data(Data::from(features_cache.clone()))
                .app_data(Data::from(token_cache.clone()))
                .service(web::scope("/api").service(features)),
        )
        .await;
        let mut token =
            EdgeToken::try_from("[]:production.puff_the_magic_dragon".to_string()).unwrap();
        token.projects = vec!["dx".into(), "eg".into(), "unleash-cloud".into()];
        token.status = TokenValidationStatus::Validated;
        token.token_type = Some(TokenType::Client);
        token_cache.insert(token.token.clone(), token.clone());
        let example_features = features_from_disk(PathBuf::from("../examples/hostedexample.json"));
        features_cache.insert("production".into(), example_features.clone());
        let req = make_features_request_with_token(token.clone()).await;
        let res: ClientFeatures = test::call_and_read_body_json(&app, req).await;
        assert_eq!(res.features.len(), 24);
        assert!(res
            .features
            .iter()
            .all(|f| token.projects.contains(&f.project.clone().unwrap())));
    }
}
