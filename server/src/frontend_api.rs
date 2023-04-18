use std::collections::HashMap;

use actix_web::{
    get, post,
    web::{self, Data, Json, Path},
    HttpRequest, HttpResponse,
};
use dashmap::DashMap;
use serde_qs::actix::QsQuery;
use unleash_types::client_features::Context;
use unleash_types::client_metrics::{ClientApplication, ConnectVia};
use unleash_types::{
    client_metrics::ClientMetrics,
    frontend::{EvaluatedToggle, EvaluatedVariant, FrontendResult},
};
use unleash_yggdrasil::{EngineState, ResolvedToggle};

use crate::error::EdgeError::ContextParseError;
use crate::{
    error::{EdgeError, FrontendHydrationMissing},
    metrics::client_metrics::MetricsCache,
    tokens::{self, cache_key},
    types::{EdgeJsonResult, EdgeResult, EdgeToken},
};

///
/// Returns all evaluated toggles for the key used
#[utoipa::path(
context_path = "/api",
responses(
(status = 200, description = "Return all known feature toggles for this token in evaluated (true|false) state", body = FrontendResult),
(status = 400, description = "Bad data in query parameters"),
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
    engine_cache: Data<DashMap<String, EngineState>>,
    token_cache: Data<DashMap<String, EdgeToken>>,
    req: HttpRequest,
) -> EdgeJsonResult<FrontendResult> {
    get_all_features(edge_token, engine_cache, token_cache, req.query_string())
}

#[utoipa::path(
context_path = "/api",
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
    engine_cache: Data<DashMap<String, EngineState>>,
    token_cache: Data<DashMap<String, EdgeToken>>,
    req: HttpRequest,
) -> EdgeJsonResult<FrontendResult> {
    get_all_features(edge_token, engine_cache, token_cache, req.query_string())
}

#[utoipa::path(
context_path = "/api",
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
    engine_cache: Data<DashMap<String, EngineState>>,
    token_cache: Data<DashMap<String, EdgeToken>>,
    context: Json<Context>,
) -> EdgeJsonResult<FrontendResult> {
    post_all_features(edge_token, engine_cache, token_cache, context)
}

#[utoipa::path(
context_path = "/api",
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
    engine_cache: Data<DashMap<String, EngineState>>,
    token_cache: Data<DashMap<String, EdgeToken>>,
    context: Json<Context>,
) -> EdgeJsonResult<FrontendResult> {
    post_all_features(edge_token, engine_cache, token_cache, context)
}

fn post_all_features(
    edge_token: EdgeToken,
    engine_cache: Data<DashMap<String, EngineState>>,
    token_cache: Data<DashMap<String, EdgeToken>>,
    context: Json<Context>,
) -> EdgeJsonResult<FrontendResult> {
    let context = context.into_inner();
    let token = token_cache
        .get(&edge_token.token)
        .map(|e| e.value().clone())
        .unwrap_or_else(|| edge_token.clone());
    let key = cache_key(&token);
    let engine = engine_cache.get(&key).ok_or_else(|| {
        EdgeError::FrontendNotYetHydrated(FrontendHydrationMissing::from(&edge_token))
    })?;
    let feature_results = engine.resolve_all(&context).unwrap();
    Ok(Json(frontend_from_yggdrasil(feature_results, true, &token)))
}

#[utoipa::path(
context_path = "/api",
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
    engine_cache: Data<DashMap<String, EngineState>>,
    token_cache: Data<DashMap<String, EdgeToken>>,
    context: QsQuery<Context>,
) -> EdgeJsonResult<FrontendResult> {
    get_enabled_features(edge_token, engine_cache, token_cache, context.into_inner())
}

#[utoipa::path(
context_path = "/api",
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
    engine_cache: Data<DashMap<String, EngineState>>,
    token_cache: Data<DashMap<String, EdgeToken>>,
    context: QsQuery<Context>,
) -> EdgeJsonResult<FrontendResult> {
    get_enabled_features(edge_token, engine_cache, token_cache, context.into_inner())
}

fn get_enabled_features(
    edge_token: EdgeToken,
    engine_cache: Data<DashMap<String, EngineState>>,
    token_cache: Data<DashMap<String, EdgeToken>>,
    context: Context,
) -> EdgeJsonResult<FrontendResult> {
    let token = token_cache
        .get(&edge_token.token)
        .map(|e| e.value().clone())
        .unwrap_or_else(|| edge_token.clone());
    let key = cache_key(&token);
    let engine = engine_cache.get(&key).ok_or_else(|| {
        EdgeError::FrontendNotYetHydrated(FrontendHydrationMissing::from(&edge_token))
    })?;
    let feature_results = engine.resolve_all(&context).unwrap();
    Ok(Json(frontend_from_yggdrasil(
        feature_results,
        false,
        &token,
    )))
}

#[utoipa::path(
context_path = "/api",
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
    engine_cache: Data<DashMap<String, EngineState>>,
    token_cache: Data<DashMap<String, EdgeToken>>,
    context: Json<Context>,
) -> EdgeJsonResult<FrontendResult> {
    post_enabled_features(edge_token, engine_cache, token_cache, context).await
}

#[utoipa::path(
context_path = "/api",
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
    engine_cache: Data<DashMap<String, EngineState>>,
    token_cache: Data<DashMap<String, EdgeToken>>,
    context: Json<Context>,
) -> EdgeJsonResult<FrontendResult> {
    post_enabled_features(edge_token, engine_cache, token_cache, context).await
}

#[utoipa::path(
context_path = "/api",
params(("feature_name" = String, Path, description = "Name of the feature")),
responses(
(status = 200, description = "Return the feature toggle with name `name`", body = EvaluatedToggle),
(status = 403, description = "Was not allowed to access features"),
(status = 404, description = "Feature was not found"),
(status = 400, description = "Invalid parameters used")
),
request_body = Context,
security(
("Authorization" = [])
)
)]
#[post("/frontend/features/{feature_name}")]
pub async fn post_frontend_evaluate_single_feature(
    edge_token: EdgeToken,
    feature_name: Path<String>,
    context: Json<Context>,
    engine_cache: Data<DashMap<String, EngineState>>,
    token_cache: Data<DashMap<String, EdgeToken>>,
) -> EdgeJsonResult<EvaluatedToggle> {
    evaluate_feature(
        edge_token,
        feature_name.into_inner(),
        &context.into_inner(),
        token_cache,
        engine_cache,
    )
    .map(Json)
}

#[utoipa::path(
context_path = "/api",
params(
    Context,
    ("feature_name" = String, Path, description = "Name of the feature"), 
),
responses(
(status = 200, description = "Return the feature toggle with name `name`", body = EvaluatedToggle),
(status = 403, description = "Was not allowed to access features"),
(status = 404, description = "Feature was not found"),
(status = 400, description = "Invalid parameters used")
),
security(
("Authorization" = [])
)
)]
#[get("/frontend/features/{feature_name}")]
pub async fn get_frontend_evaluate_single_feature(
    edge_token: EdgeToken,
    feature_name: Path<String>,
    context: QsQuery<Context>,
    engine_cache: Data<DashMap<String, EngineState>>,
    token_cache: Data<DashMap<String, EdgeToken>>,
) -> EdgeJsonResult<EvaluatedToggle> {
    evaluate_feature(
        edge_token,
        feature_name.into_inner(),
        &context.into_inner(),
        token_cache,
        engine_cache,
    )
    .map(Json)
}

pub fn evaluate_feature(
    edge_token: EdgeToken,
    feature_name: String,
    context: &Context,
    token_cache: Data<DashMap<String, EdgeToken>>,
    engine_cache: Data<DashMap<String, EngineState>>,
) -> EdgeResult<EvaluatedToggle> {
    let validated_token = token_cache
        .get(&edge_token.token)
        .ok_or(EdgeError::EdgeTokenError)?
        .value()
        .clone();
    engine_cache
        .get(&cache_key(&validated_token))
        .and_then(|engine| engine.resolve_all(context))
        .and_then(|toggles| toggles.get(&feature_name).cloned())
        .and_then(|resolved_toggle| {
            if validated_token.projects.contains(&"*".into())
                || validated_token.projects.contains(&resolved_toggle.project)
            {
                Some(resolved_toggle)
            } else {
                None
            }
        })
        .map(|r| EvaluatedToggle {
            name: feature_name.clone(),
            enabled: r.enabled,
            variant: EvaluatedVariant {
                name: r.variant.name,
                enabled: r.variant.enabled,
                payload: r.variant.payload,
            },
            impression_data: r.impression_data,
        })
        .ok_or_else(|| EdgeError::FeatureNotFound(feature_name.clone()))
}

async fn post_enabled_features(
    edge_token: EdgeToken,
    engine_cache: Data<DashMap<String, EngineState>>,
    token_cache: Data<DashMap<String, EdgeToken>>,
    context: Json<Context>,
) -> EdgeJsonResult<FrontendResult> {
    let context = context.into_inner();
    let token = token_cache
        .get(&edge_token.token)
        .map(|e| e.value().clone())
        .unwrap_or_else(|| edge_token.clone());
    let engine = engine_cache
        .get(&tokens::cache_key(&edge_token))
        .ok_or_else(|| {
            EdgeError::FrontendNotYetHydrated(FrontendHydrationMissing::from(&edge_token))
        })?;
    let feature_results = engine.resolve_all(&context).unwrap();
    Ok(Json(frontend_from_yggdrasil(
        feature_results,
        false,
        &token,
    )))
}

#[utoipa::path(
context_path = "/api",
responses(
(status = 202, description = "Accepted client metrics"),
(status = 403, description = "Was not allowed to post metrics"),
),
request_body = ClientMetrics,
security(
("Authorization" = [])
)
)]
#[post("/proxy/client/metrics")]
async fn post_proxy_metrics(
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
context_path = "/api",
responses(
(status = 202, description = "Accepted client metrics"),
(status = 403, description = "Was not allowed to post metrics"),
),
request_body = ClientMetrics,
security(
("Authorization" = [])
)
)]
#[post("/frontend/client/metrics")]
async fn post_frontend_metrics(
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
context_path = "/api",
responses(
(status = 202, description = "Accepted client application registration"),
(status = 403, description = "Was not allowed to register client"),
),
request_body = ClientApplication,
security(
("Authorization" = [])
)
)]
#[post("/proxy/client/register")]
pub async fn post_proxy_register(
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
    Ok(HttpResponse::Accepted().finish())
}

#[utoipa::path(
context_path = "/api",
responses(
(status = 202, description = "Accepted client application registration"),
(status = 403, description = "Was not allowed to register client"),
),
request_body = ClientApplication,
security(
("Authorization" = [])
)
)]
#[post("/frontend/client/register")]
pub async fn post_frontend_register(
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
    Ok(HttpResponse::Accepted().finish())
}

pub fn configure_frontend_api(cfg: &mut web::ServiceConfig) {
    cfg.service(get_enabled_proxy)
        .service(get_enabled_frontend)
        .service(get_proxy_all_features)
        .service(get_frontend_all_features)
        .service(post_proxy_metrics)
        .service(post_frontend_metrics)
        .service(post_frontend_all_features)
        .service(post_proxy_all_features)
        .service(post_proxy_enabled_features)
        .service(post_frontend_enabled_features)
        .service(post_proxy_register)
        .service(post_frontend_register)
        .service(post_frontend_evaluate_single_feature)
        .service(get_frontend_evaluate_single_feature);
}

pub fn frontend_from_yggdrasil(
    res: HashMap<String, ResolvedToggle>,
    include_all: bool,
    edge_token: &EdgeToken,
) -> FrontendResult {
    let toggles: Vec<EvaluatedToggle> = res
        .iter()
        .filter(|(_, resolved)| include_all || resolved.enabled)
        .filter(|(_, resolved)| {
            edge_token.projects.is_empty()
                || edge_token.projects.contains(&"*".to_string())
                || edge_token.projects.contains(&resolved.project)
        })
        .map(|(name, resolved)| EvaluatedToggle {
            name: name.into(),
            enabled: resolved.enabled,
            variant: EvaluatedVariant {
                name: resolved.variant.name.clone(),
                enabled: resolved.variant.enabled,
                payload: resolved.variant.payload.clone(),
            },
            impression_data: resolved.impression_data,
        })
        .collect::<Vec<EvaluatedToggle>>();
    FrontendResult { toggles }
}

pub fn get_all_features(
    edge_token: EdgeToken,
    engine_cache: Data<DashMap<String, EngineState>>,
    token_cache: Data<DashMap<String, EdgeToken>>,
    query_string: &str,
) -> EdgeJsonResult<FrontendResult> {
    let context = serde_qs::Config::new(0, false)
        .deserialize_str(query_string)
        .map_err(|_| ContextParseError)?;
    let token = token_cache
        .get(&edge_token.token)
        .map(|e| e.value().clone())
        .unwrap_or_else(|| edge_token.clone());
    let key = cache_key(&token);
    let engine = engine_cache.get(&key).ok_or_else(|| {
        EdgeError::FrontendNotYetHydrated(FrontendHydrationMissing::from(&edge_token))
    })?;
    let feature_results = engine.resolve_all(&context).unwrap();
    Ok(Json(frontend_from_yggdrasil(feature_results, true, &token)))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use crate::builder::build_offline_mode;
    use crate::cli::{EdgeMode, OfflineArgs};
    use crate::metrics::client_metrics::MetricsCache;
    use crate::metrics::client_metrics::MetricsKey;
    use crate::middleware;
    use crate::types::{EdgeToken, TokenType, TokenValidationStatus};
    use actix_http::{Request, StatusCode};
    use actix_web::{
        http::header::ContentType,
        test,
        web::{self, Data},
        App,
    };
    use chrono::{DateTime, Utc};
    use dashmap::DashMap;
    use serde_json::json;
    use unleash_types::client_metrics::ClientMetricsEnv;
    use unleash_types::{
        client_features::{ClientFeature, ClientFeatures, Constraint, Operator, Strategy},
        frontend::{EvaluatedToggle, EvaluatedVariant, FrontendResult},
    };
    use unleash_yggdrasil::EngineState;

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
        let (token_cache, features_cache, engine_cache) = build_offline_mode(
            client_features_with_constraint_requiring_user_id_of_seven(),
            vec![
                "*:development.03fa5f506428fe80ed5640c351c7232e38940814d2923b08f5c05fa7"
                    .to_string(),
            ],
        )
        .unwrap();

        let app = test::init_service(
            App::new()
                .app_data(Data::from(token_cache))
                .app_data(Data::from(features_cache))
                .app_data(Data::from(engine_cache))
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
        let (feature_cache, token_cache, engine_cache) = build_offline_mode(
            client_features_with_constraint_requiring_user_id_of_seven(),
            vec![
                "*:development.03fa5f506428fe80ed5640c351c7232e38940814d2923b08f5c05fa7"
                    .to_string(),
            ],
        )
        .unwrap();
        let app = test::init_service(
            App::new()
                .app_data(Data::from(token_cache))
                .app_data(Data::from(feature_cache))
                .app_data(Data::from(engine_cache))
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
        let (token_cache, features_cache, engine_cache) = build_offline_mode(
            client_features_with_constraint_one_enabled_toggle_and_one_disabled_toggle(),
            vec![
                "*:development.03fa5f506428fe80ed5640c351c7232e38940814d2923b08f5c05fa7"
                    .to_string(),
            ],
        )
        .unwrap();

        let app = test::init_service(
            App::new()
                .app_data(Data::from(token_cache))
                .app_data(Data::from(features_cache))
                .app_data(Data::from(engine_cache))
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
                .service(web::scope("/api").service(super::post_proxy_metrics)),
        )
        .await;

        let req = make_test_request().await;
        test::call_and_read_body(&app, req).await;

        let found_metric = metrics_cache
            .metrics
            .get(&MetricsKey {
                app_name: "some-app".into(),
                feature_name: "some-feature".into(),
                environment: "development".into(),
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

    #[tokio::test]
    async fn when_running_in_offline_mode_with_proxy_key_should_not_filter_features() {
        let client_features = client_features_with_constraint_requiring_user_id_of_seven();
        let (token_cache, feature_cache, engine_cache) =
            build_offline_mode(client_features.clone(), vec!["secret-123".to_string()]).unwrap();
        let app = test::init_service(
            App::new()
                .app_data(Data::from(token_cache))
                .app_data(Data::from(feature_cache))
                .app_data(Data::from(engine_cache))
                .app_data(Data::new(EdgeMode::Offline(OfflineArgs {
                    bootstrap_file: None,
                    tokens: vec!["secret-123".into()],
                })))
                .service(web::scope("/api").service(super::get_frontend_all_features)),
        )
        .await;

        let req = test::TestRequest::get()
            .uri("/api/frontend/all")
            .insert_header(ContentType::json())
            .insert_header(("Authorization", "secret-123"))
            .to_request();

        let result: FrontendResult = test::call_and_read_body_json(&app, req).await;
        assert_eq!(result.toggles.len(), client_features.features.len());
    }

    #[tokio::test]
    async fn frontend_api_filters_evaluated_toggles_to_tokens_access() {
        let client_features = crate::tests::features_from_disk("../examples/hostedexample.json");
        let (token_cache, feature_cache, engine_cache) = build_offline_mode(
            client_features.clone(),
            vec!["dx:development.secret123".to_string()],
        )
        .unwrap();
        let app = test::init_service(
            App::new()
                .app_data(Data::from(token_cache))
                .app_data(Data::from(feature_cache))
                .app_data(Data::from(engine_cache))
                .service(web::scope("/api").service(super::get_frontend_all_features)),
        )
        .await;

        let req = test::TestRequest::get()
            .uri("/api/frontend/all")
            .insert_header(ContentType::json())
            .insert_header(("Authorization", "dx:development.secret123"))
            .to_request();

        let result: FrontendResult = test::call_and_read_body_json(&app, req).await;
        assert_eq!(result.toggles.len(), 16);
    }

    #[tokio::test]
    async fn frontend_token_without_matching_client_token_yields_511_when_trying_to_access_frontend_api(
    ) {
        let features_cache: Arc<DashMap<String, ClientFeatures>> = Arc::new(DashMap::default());
        let engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let app = test::init_service(
            App::new()
                .app_data(Data::from(token_cache.clone()))
                .app_data(Data::from(features_cache.clone()))
                .app_data(Data::from(engine_cache.clone()))
                .wrap(middleware::as_async_middleware::as_async_middleware(
                    middleware::validate_token::validate_token,
                ))
                .service(web::scope("/api").configure(super::configure_frontend_api)),
        )
        .await;

        let mut frontend_token =
            EdgeToken::try_from("ourtests:rocking.secret123".to_string()).unwrap();
        frontend_token.status = TokenValidationStatus::Validated;
        frontend_token.token_type = Some(TokenType::Frontend);
        token_cache.insert(frontend_token.token.clone(), frontend_token.clone());
        let req = test::TestRequest::get()
            .uri("/api/frontend/all")
            .insert_header(ContentType::json())
            .insert_header(("Authorization", frontend_token.token))
            .to_request();
        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), StatusCode::NETWORK_AUTHENTICATION_REQUIRED);
    }

    #[tokio::test]
    async fn invalid_token_is_refused_with_403() {
        let features_cache: Arc<DashMap<String, ClientFeatures>> = Arc::new(DashMap::default());
        let engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let app = test::init_service(
            App::new()
                .app_data(Data::from(token_cache.clone()))
                .app_data(Data::from(features_cache.clone()))
                .app_data(Data::from(engine_cache.clone()))
                .wrap(middleware::as_async_middleware::as_async_middleware(
                    middleware::validate_token::validate_token,
                ))
                .service(web::scope("/api").configure(super::configure_frontend_api)),
        )
        .await;
        let req = test::TestRequest::get()
            .uri("/api/frontend/all")
            .insert_header(ContentType::json())
            .insert_header(("Authorization", "dx:rocking.secret123"))
            .to_request();
        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn can_get_single_feature() {
        let client_features = crate::tests::features_from_disk("../examples/hostedexample.json");
        let (token_cache, feature_cache, engine_cache) = build_offline_mode(
            client_features.clone(),
            vec!["dx:development.secret123".to_string()],
        )
        .unwrap();
        let app = test::init_service(
            App::new()
                .app_data(Data::from(token_cache))
                .app_data(Data::from(feature_cache))
                .app_data(Data::from(engine_cache))
                .service(web::scope("/api").configure(super::configure_frontend_api)),
        )
        .await;

        let req = test::TestRequest::get()
            .uri("/api/frontend/features/batchMetrics")
            .insert_header(ContentType::json())
            .insert_header(("Authorization", "dx:development.secret123"))
            .to_request();

        let result = test::call_service(&app, req).await;
        assert_eq!(result.status(), 200);
    }

    #[tokio::test]
    async fn trying_to_evaluate_feature_you_do_not_have_access_to_will_give_not_found() {
        let client_features = crate::tests::features_from_disk("../examples/hostedexample.json");
        let (token_cache, feature_cache, engine_cache) = build_offline_mode(
            client_features.clone(),
            vec!["dx:development.secret123".to_string()],
        )
        .unwrap();
        let app = test::init_service(
            App::new()
                .app_data(Data::from(token_cache))
                .app_data(Data::from(feature_cache))
                .app_data(Data::from(engine_cache))
                .service(web::scope("/api").configure(super::configure_frontend_api)),
        )
        .await;

        let req = test::TestRequest::get()
            .uri("/api/frontend/features/variantsPerEnvironment")
            .insert_header(ContentType::json())
            .insert_header(("Authorization", "dx:development.secret123"))
            .to_request();

        let result = test::call_service(&app, req).await;
        assert_eq!(result.status(), 404);
    }

    #[tokio::test]
    async fn can_handle_custom_context_fields() {
        let client_features_with_custom_context_field =
            crate::tests::features_from_disk("../examples/with_custom_constraint.json");
        let auth_key = "default:development.secret123".to_string();
        let (token_cache, feature_cache, engine_cache) = build_offline_mode(
            client_features_with_custom_context_field.clone(),
            vec![auth_key.clone()],
        )
        .unwrap();
        let config =
            serde_qs::actix::QsQueryConfig::default().qs_config(serde_qs::Config::new(5, false));
        let app = test::init_service(
            App::new()
                .app_data(config)
                .app_data(Data::from(token_cache))
                .app_data(Data::from(feature_cache))
                .app_data(Data::from(engine_cache))
                .service(web::scope("/api").configure(super::configure_frontend_api)),
        )
        .await;
        let req = test::TestRequest::get()
            .uri("/api/frontend?properties[companyId]=bricks")
            .insert_header(ContentType::json())
            .insert_header(("Authorization", auth_key.clone()))
            .to_request();
        let no_escape: FrontendResult = test::call_and_read_body_json(&app, req).await;
        assert_eq!(no_escape.toggles.len(), 1);
        let req = test::TestRequest::get()
            .uri("/api/frontend?properties%5BcompanyId%5D=bricks")
            .insert_header(ContentType::json())
            .insert_header(("Authorization", auth_key.clone()))
            .to_request();
        let escape: FrontendResult = test::call_and_read_body_json(&app, req).await;
        assert_eq!(escape.toggles.len(), 1);
    }

    #[tokio::test]
    async fn can_handle_custom_context_fields_with_post() {
        let client_features_with_custom_context_field =
            crate::tests::features_from_disk("../examples/with_custom_constraint.json");
        let auth_key = "default:development.secret123".to_string();
        let (token_cache, feature_cache, engine_cache) = build_offline_mode(
            client_features_with_custom_context_field.clone(),
            vec![auth_key.clone()],
        )
        .unwrap();
        let app = test::init_service(
            App::new()
                .app_data(Data::from(token_cache))
                .app_data(Data::from(feature_cache))
                .app_data(Data::from(engine_cache))
                .service(web::scope("/api").configure(super::configure_frontend_api)),
        )
        .await;
        let req = test::TestRequest::post()
            .uri("/api/frontend")
            .insert_header(ContentType::json())
            .insert_header(("Authorization", auth_key.clone()))
            .set_json(json!({ "properties": {"companyId": "bricks"}}))
            .to_request();
        let result: FrontendResult = test::call_and_read_body_json(&app, req).await;
        assert_eq!(result.toggles.len(), 1);

        let req = test::TestRequest::post()
            .uri("/api/frontend")
            .insert_header(ContentType::json())
            .insert_header(("Authorization", auth_key))
            .set_json(json!({ "companyId": "bricks"}))
            .to_request();
        let result: FrontendResult = test::call_and_read_body_json(&app, req).await;
        assert!(result.toggles.is_empty());
    }
}
