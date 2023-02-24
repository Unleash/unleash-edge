use actix_web::{
    get, post,
    web::{self, Json},
    HttpResponse,
};
use unleash_types::{
    client_features::{ClientFeatures, Payload},
    client_metrics::{from_bucket_app_name_and_env, ClientMetrics},
    frontend::{EvaluatedToggle, EvaluatedVariant, FrontendResult},
};
use unleash_yggdrasil::{Context, EngineState};

use crate::{
    metrics::client_metrics::MetricsCache,
    types::{EdgeJsonResult, EdgeResult, EdgeSource, EdgeToken},
};

///
/// Returns all evaluated toggles for the key used
#[utoipa::path(
    path = "/api/proxy/all",
    responses(
        (status = 200, description = "Return all known feature toggles for this token in evaluated (true|false) state", body = FrontendResult),
        (status = 403, description = "Was not allowed to access features")
    ),
    params(Context),
    security(
        ("Authorization" = [])
    )
)]
#[get("/proxy/all")]
pub async fn get_proxy_all_features(
    edge_token: EdgeToken,
    features_source: web::Data<dyn EdgeSource>,
    context: web::Query<Context>,
) -> EdgeJsonResult<FrontendResult> {
    get_all_features(edge_token, features_source, context).await
}

#[utoipa::path(
    path = "/api/frontend/all",
    responses(
        (status = 200, description = "Return all known feature toggles for this token in evaluated (true|false) state", body = FrontendResult),
        (status = 403, description = "Was not allowed to access features")
    ),
    params(Context),
    security(
        ("Authorization" = [])
    )
)]
#[get("/frontend/all")]
pub async fn get_frontend_all_features(
    edge_token: EdgeToken,
    features_source: web::Data<dyn EdgeSource>,
    context: web::Query<Context>,
) -> EdgeJsonResult<FrontendResult> {
    get_all_features(edge_token, features_source, context).await
}

pub async fn get_all_features(
    edge_token: EdgeToken,
    features_source: web::Data<dyn EdgeSource>,
    context: web::Query<Context>,
) -> EdgeJsonResult<FrontendResult> {
    let client_features = features_source.get_client_features(&edge_token).await;
    let context = context.into_inner();

    let toggles = resolve_frontend_features(client_features?, context).collect();

    Ok(Json(FrontendResult { toggles }))
}

#[utoipa::path(
    path = "/api/proxy/all",
    responses(
        (status = 200, description = "Return all known feature toggles for this token in evaluated (true|false) state", body = FrontendResult),
        (status = 403, description = "Was not allowed to access features"),
        (status = 400, description = "Invalid parameters used")
    ),
    request_body = Context,
    security(
        ("Authorization" = [])
    )
)]
#[post("/proxy/all")]
async fn post_proxy_all_features(
    edge_token: EdgeToken,
    features_source: web::Data<dyn EdgeSource>,
    context: web::Json<Context>,
) -> EdgeJsonResult<FrontendResult> {
    post_all_features(edge_token, features_source, context).await
}

#[utoipa::path(
    path = "/api/frontend/all",
    responses(
        (status = 200, description = "Return all known feature toggles for this token in evaluated (true|false) state", body = FrontendResult),
        (status = 403, description = "Was not allowed to access features"),
        (status = 400, description = "Invalid parameters used")
    ),
    request_body = Context,
    security(
        ("Authorization" = [])
    )
)]
#[post("/frontend/all")]
async fn post_frontend_all_features(
    edge_token: EdgeToken,
    features_source: web::Data<dyn EdgeSource>,
    context: web::Json<Context>,
) -> EdgeJsonResult<FrontendResult> {
    post_all_features(edge_token, features_source, context).await
}

async fn post_all_features(
    edge_token: EdgeToken,
    features_source: web::Data<dyn EdgeSource>,
    context: web::Json<Context>,
) -> EdgeJsonResult<FrontendResult> {
    let client_features = features_source.get_client_features(&edge_token).await;
    let context = context.into_inner();

    let toggles = resolve_frontend_features(client_features?, context).collect();

    Ok(Json(FrontendResult { toggles }))
}

#[utoipa::path(
    path = "/api/proxy",
    responses(
        (status = 200, description = "Return feature toggles for this token that evaluated to true", body = FrontendResult),
        (status = 403, description = "Was not allowed to access features"),
        (status = 400, description = "Invalid parameters used")
    ),
    params(Context),
    security(
        ("Authorization" = [])
    )
)]
#[get("/proxy")]
async fn get_enabled_proxy(
    edge_token: EdgeToken,
    features_source: web::Data<dyn EdgeSource>,
    context: web::Query<Context>,
) -> EdgeJsonResult<FrontendResult> {
    get_enabled_features(edge_token, features_source, context).await
}

#[utoipa::path(
    path = "/api/frontend",
    responses(
        (status = 200, description = "Return feature toggles for this token that evaluated to true", body = FrontendResult),
        (status = 403, description = "Was not allowed to access features"),
        (status = 400, description = "Invalid parameters used")
    ),
    params(Context),
    security(
        ("Authorization" = [])
    )
)]
#[get("/frontend")]
async fn get_enabled_frontend(
    edge_token: EdgeToken,
    features_source: web::Data<dyn EdgeSource>,
    context: web::Query<Context>,
) -> EdgeJsonResult<FrontendResult> {
    get_enabled_features(edge_token, features_source, context).await
}
async fn get_enabled_features(
    edge_token: EdgeToken,
    features_source: web::Data<dyn EdgeSource>,
    context: web::Query<Context>,
) -> EdgeJsonResult<FrontendResult> {
    let client_features = features_source.get_client_features(&edge_token).await;
    let context = context.into_inner();

    let toggles: Vec<EvaluatedToggle> = resolve_frontend_features(client_features?, context)
        .filter(|toggle| toggle.enabled)
        .collect();

    Ok(Json(FrontendResult { toggles }))
}

#[utoipa::path(
    path = "/api/proxy",
    responses(
        (status = 200, description = "Return feature toggles for this token that evaluated to true", body = FrontendResult),
        (status = 403, description = "Was not allowed to access features"),
        (status = 400, description = "Invalid parameters used")
    ),
    request_body = Context,
    security(
        ("Authorization" = [])
    )
)]
#[post("/proxy")]
async fn post_proxy_enabled_features(
    edge_token: EdgeToken,
    features_source: web::Data<dyn EdgeSource>,
    context: web::Query<Context>,
) -> EdgeJsonResult<FrontendResult> {
    post_enabled_features(edge_token, features_source, context).await
}

#[utoipa::path(
    path = "/api/frontend",
    responses(
        (status = 200, description = "Return feature toggles for this token that evaluated to true", body = FrontendResult),
        (status = 403, description = "Was not allowed to access features"),
        (status = 400, description = "Invalid parameters used")
    ),
    request_body = Context,
    security(
        ("Authorization" = [])
    )
)]
#[post("/frontend")]
async fn post_frontend_enabled_features(
    edge_token: EdgeToken,
    features_source: web::Data<dyn EdgeSource>,
    context: web::Query<Context>,
) -> EdgeJsonResult<FrontendResult> {
    post_enabled_features(edge_token, features_source, context).await
}

async fn post_enabled_features(
    edge_token: EdgeToken,
    features_source: web::Data<dyn EdgeSource>,
    context: web::Query<Context>,
) -> EdgeJsonResult<FrontendResult> {
    let client_features = features_source.get_client_features(&edge_token).await;
    let context = context.into_inner();

    let toggles: Vec<EvaluatedToggle> = resolve_frontend_features(client_features?, context)
        .filter(|toggle| toggle.enabled)
        .collect();

    Ok(Json(FrontendResult { toggles }))
}

#[post("/proxy/client/metrics")]
async fn post_frontend_metrics(
    edge_token: EdgeToken,
    metrics: web::Json<ClientMetrics>,
    metrics_cache: web::Data<MetricsCache>,
) -> EdgeResult<HttpResponse> {
    let metrics = metrics.into_inner();

    let metrics = from_bucket_app_name_and_env(
        metrics.bucket,
        metrics.app_name,
        edge_token.environment.unwrap(),
    );

    metrics_cache.sink_metrics(&metrics);

    Ok(HttpResponse::Accepted().finish())
}

fn resolve_frontend_features(
    client_features: ClientFeatures,
    context: Context,
) -> impl Iterator<Item = EvaluatedToggle> {
    let mut engine = EngineState::default();
    engine.take_state(client_features.clone());

    client_features.features.into_iter().map(move |toggle| {
        let variant = engine.get_variant(toggle.name.clone(), &context);
        EvaluatedToggle {
            name: toggle.name.clone(),
            enabled: engine.is_enabled(toggle.name, &context),
            variant: EvaluatedVariant {
                name: variant.name,
                enabled: variant.enabled,
                payload: variant.payload.map(|succ| Payload {
                    payload_type: succ.payload_type,
                    value: succ.value,
                }),
            },
            impression_data: false,
        }
    })
}

pub fn configure_frontend_api(cfg: &mut web::ServiceConfig) {
    cfg.service(get_enabled_proxy)
        .service(get_enabled_frontend)
        .service(get_proxy_all_features)
        .service(get_frontend_all_features)
        .service(post_frontend_metrics)
        .service(post_frontend_all_features)
        .service(post_proxy_all_features)
        .service(post_proxy_enabled_features)
        .service(post_frontend_enabled_features);
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use crate::metrics::client_metrics::MetricsKey;
    use crate::types::EdgeSource;
    use crate::{
        data_sources::offline_provider::OfflineProvider, metrics::client_metrics::MetricsCache,
    };
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
    use unleash_types::{
        client_features::{ClientFeature, ClientFeatures, Constraint, Operator, Strategy},
        frontend::{EvaluatedToggle, EvaluatedVariant, FrontendResult},
    };

    async fn make_test_request() -> Request {
        test::TestRequest::post()
            .uri("/api/proxy/client/metrics")
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

    fn client_features_with_constraint_requiring_user_id_of_seven() -> ClientFeatures {
        ClientFeatures {
            version: 1,
            features: vec![ClientFeature {
                name: "test".into(),
                enabled: true,
                strategies: Some(vec![Strategy {
                    name: "default".into(),
                    sort_order: None,
                    segments: None,
                    constraints: Some(vec![Constraint {
                        context_name: "userId".into(),
                        operator: Operator::In,
                        case_insensitive: false,
                        inverted: false,
                        values: Some(vec!["7".into()]),
                        value: None,
                    }]),
                    parameters: None,
                }]),
                ..ClientFeature::default()
            }],
            segments: None,
            query: None,
        }
    }

    fn client_features_with_constraint_one_enabled_toggle_and_one_disabled_toggle() -> ClientFeatures
    {
        ClientFeatures {
            version: 1,
            features: vec![
                ClientFeature {
                    name: "test".into(),
                    enabled: true,
                    strategies: None,
                    ..ClientFeature::default()
                },
                ClientFeature {
                    name: "test2".into(),
                    enabled: false,
                    strategies: None,
                    ..ClientFeature::default()
                },
            ],
            segments: None,
            query: None,
        }
    }

    #[actix_web::test]
    async fn calling_post_requests_resolves_context_values_correctly() {
        let shareable_provider = Arc::new(OfflineProvider::new(
            client_features_with_constraint_requiring_user_id_of_seven(),
            vec![
                "*:development.03fa5f506428fe80ed5640c351c7232e38940814d2923b08f5c05fa7"
                    .to_string(),
            ],
        ));
        let edge_source: Arc<dyn EdgeSource> = shareable_provider.clone();

        let app = test::init_service(
            App::new()
                .app_data(Data::from(edge_source))
                .service(web::scope("/api").service(super::post_frontend_all_features)),
        )
        .await;

        let req = test::TestRequest::post()
            .uri("/api/frontend/all")
            .insert_header(ContentType::json())
            .insert_header((
                "Authorization",
                "*:development.03fa5f506428fe80ed5640c351c7232e38940814d2923b08f5c05fa7",
            ))
            .set_json(json!({
                "userId": "7"
            }))
            .to_request();
        let second_req = test::TestRequest::post()
            .uri("/api/frontend/all")
            .insert_header(ContentType::json())
            .insert_header((
                "Authorization",
                "*:development.03fa5f506428fe80ed5640c351c7232e38940814d2923b08f5c05fa7",
            ))
            .set_json(json!({
                "userId": "7"
            }))
            .to_request();

        let _result: FrontendResult = test::call_and_read_body_json(&app, req).await;
        let result: FrontendResult = test::call_and_read_body_json(&app, second_req).await;
        assert_eq!(result.toggles.len(), 1);
        assert!(result.toggles.get(0).unwrap().enabled)
    }

    #[actix_web::test]
    async fn calling_get_requests_resolves_context_values_correctly() {
        let shareable_provider = Arc::new(OfflineProvider::new(
            client_features_with_constraint_requiring_user_id_of_seven(),
            vec![
                "*:development.03fa5f506428fe80ed5640c351c7232e38940814d2923b08f5c05fa7"
                    .to_string(),
            ],
        ));
        let edge_source: Arc<dyn EdgeSource> = shareable_provider.clone();
        let app = test::init_service(
            App::new()
                .app_data(Data::from(edge_source.clone()))
                .service(web::scope("/api").service(super::get_proxy_all_features)),
        )
        .await;

        let req = test::TestRequest::get()
            .uri("/api/proxy/all?userId=7")
            .insert_header(ContentType::json())
            .insert_header((
                "Authorization",
                "*:development.03fa5f506428fe80ed5640c351c7232e38940814d2923b08f5c05fa7",
            ))
            .to_request();

        let result = test::call_and_read_body(&app, req).await;

        let expected = FrontendResult {
            toggles: vec![EvaluatedToggle {
                name: "test".into(),
                enabled: true,
                variant: EvaluatedVariant {
                    name: "disabled".into(),
                    enabled: false,
                    payload: None,
                },
                impression_data: false,
            }],
        };

        assert_eq!(result, serde_json::to_vec(&expected).unwrap());
    }

    #[actix_web::test]
    async fn calling_get_requests_resolves_context_values_correctly_with_enabled_filter() {
        let shareable_provider = Arc::new(OfflineProvider::new(
            client_features_with_constraint_one_enabled_toggle_and_one_disabled_toggle(),
            vec![
                "*:development.03fa5f506428fe80ed5640c351c7232e38940814d2923b08f5c05fa7"
                    .to_string(),
            ],
        ));
        let edge_source: Arc<dyn EdgeSource> = shareable_provider.clone();

        let app = test::init_service(
            App::new()
                .app_data(Data::from(edge_source.clone()))
                .service(web::scope("/api").service(super::get_enabled_proxy)),
        )
        .await;

        let req = test::TestRequest::get()
            .uri("/api/proxy?userId=7")
            .insert_header(ContentType::json())
            .insert_header((
                "Authorization",
                "*:development.03fa5f506428fe80ed5640c351c7232e38940814d2923b08f5c05fa7",
            ))
            .to_request();

        let result: FrontendResult = test::call_and_read_body_json(&app, req).await;

        assert_eq!(result.toggles.len(), 1);
    }

    #[actix_web::test]
    async fn frontend_metrics_endpoint_correctly_aggregates_data() {
        let metrics_cache = Arc::new(MetricsCache::default());

        let app = test::init_service(
            App::new()
                .app_data(Data::from(metrics_cache.clone()))
                .service(web::scope("/api").service(super::post_frontend_metrics)),
        )
        .await;

        let req = make_test_request().await;
        test::call_and_read_body(&app, req).await;

        let found_metric = metrics_cache
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
