use actix_http::HttpMessage;
use actix_http::body::MessageBody;
use actix_service::ServiceFactory;
use itertools::Itertools;
use std::collections::HashMap;

use actix_web::dev::{Payload, ServiceRequest, ServiceResponse};
use actix_web::{
    HttpRequest, HttpResponse, Scope, get, post,
    web::{self, Data, Json, Path},
};
use chrono::Utc;
use dashmap::DashMap;
use serde_qs::actix::QsQuery;
use tracing::debug;
use unleash_types::client_features::Context;
use unleash_types::client_metrics::{ClientApplication, ConnectVia, MetricsMetadata, SdkType};
use unleash_types::{
    client_metrics::ClientMetrics,
    frontend::{EvaluatedToggle, EvaluatedVariant, FrontendResult},
};
use unleash_yggdrasil::{EngineState, ResolvedToggle};

use crate::types::ClientIp;
use crate::{
    error::{EdgeError, FrontendHydrationMissing},
    metrics::client_metrics::MetricsCache,
    tokens::{self, cache_key},
    types::{EdgeJsonResult, EdgeResult, EdgeToken},
};

use actix_web::FromRequest;
use std::future::{Ready, ready};

#[derive(Debug, Clone)]
pub struct UnleashSdkHeader(pub Option<String>);

impl FromRequest for UnleashSdkHeader {
    type Error = EdgeError;
    type Future = Ready<EdgeResult<Self>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        let sdk_version = req
            .headers()
            .get("unleash-sdk")
            .and_then(|val| val.to_str().ok())
            .map(str::to_owned);
        ready(Ok(UnleashSdkHeader(sdk_version)))
    }
}

///
/// Returns all evaluated toggles for the key used
#[utoipa::path(
context_path = "/api/proxy",
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
#[get("/all")]
pub async fn get_proxy_all_features(
    edge_token: EdgeToken,
    engine_cache: Data<DashMap<String, EngineState>>,
    token_cache: Data<DashMap<String, EdgeToken>>,
    context: QsQuery<Context>,
    req: HttpRequest,
) -> EdgeJsonResult<FrontendResult> {
    get_all_features(
        edge_token,
        engine_cache,
        token_cache,
        &context.into_inner().into(),
        req.extensions().get::<ClientIp>(),
    )
}

#[utoipa::path(
context_path = "/api/frontend",
responses(
(status = 200, description = "Return all known feature toggles for this token in evaluated (true|false) state", body = FrontendResult),
(status = 403, description = "Was not allowed to access features")
),
params(Context),
security(
("Authorization" = [])
)
)]
#[get("/all")]
pub async fn get_frontend_all_features(
    edge_token: EdgeToken,
    engine_cache: Data<DashMap<String, EngineState>>,
    token_cache: Data<DashMap<String, EdgeToken>>,
    context: QsQuery<Context>,
    req: HttpRequest,
) -> EdgeJsonResult<FrontendResult> {
    get_all_features(
        edge_token,
        engine_cache,
        token_cache,
        &context.into_inner().into(),
        req.extensions().get::<ClientIp>(),
    )
}

#[utoipa::path(
context_path = "/api/proxy",
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
#[post("/all")]
async fn post_proxy_all_features(
    edge_token: EdgeToken,
    engine_cache: Data<DashMap<String, EngineState>>,
    token_cache: Data<DashMap<String, EdgeToken>>,
    context: Json<Context>,
    req: HttpRequest,
) -> EdgeJsonResult<FrontendResult> {
    post_all_features(
        edge_token,
        engine_cache,
        token_cache,
        context,
        req.extensions().get::<ClientIp>(),
    )
}

#[utoipa::path(
    context_path = "/api/frontend",
    responses(
    (status = 202, description = "Accepted client metrics"),
    (status = 403, description = "Was not allowed to post metrics"),
    ),
    request_body = ClientMetrics,
    security(
    ("Authorization" = [])
    )
    )]
#[post("/all/client/metrics")]
async fn post_all_proxy_metrics(
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
    context_path = "/api/frontend",
    responses(
    (status = 202, description = "Accepted client metrics"),
    (status = 403, description = "Was not allowed to post metrics"),
    ),
    request_body = ClientMetrics,
    security(
    ("Authorization" = [])
    )
    )]
#[post("/all/client/metrics")]
async fn post_all_frontend_metrics(
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
context_path = "/api/frontend",
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
#[post("/all")]
async fn post_frontend_all_features(
    edge_token: EdgeToken,
    engine_cache: Data<DashMap<String, EngineState>>,
    token_cache: Data<DashMap<String, EdgeToken>>,
    context: Json<Context>,
    req: HttpRequest,
) -> EdgeJsonResult<FrontendResult> {
    post_all_features(
        edge_token,
        engine_cache,
        token_cache,
        context,
        req.extensions().get::<ClientIp>(),
    )
}

fn post_all_features(
    edge_token: EdgeToken,
    engine_cache: Data<DashMap<String, EngineState>>,
    token_cache: Data<DashMap<String, EdgeToken>>,
    incoming_context: Json<Context>,
    client_ip: Option<&ClientIp>,
) -> EdgeJsonResult<FrontendResult> {
    let context: Context = incoming_context.into_inner().into();
    let context_with_ip = if context.remote_address.is_none() {
        Context {
            remote_address: client_ip.map(|ip| ip.to_string()),
            ..context
        }
    } else {
        context
    };
    let token = token_cache
        .get(&edge_token.token)
        .map(|e| e.value().clone())
        .unwrap_or_else(|| edge_token.clone());
    let key = cache_key(&token);
    let engine = engine_cache.get(&key).ok_or_else(|| {
        EdgeError::FrontendNotYetHydrated(FrontendHydrationMissing::from(&edge_token))
    })?;
    let feature_results = engine.resolve_all(&context_with_ip, &None).ok_or_else(|| {
        EdgeError::FrontendExpectedToBeHydrated(
            "Feature cache has not been hydrated yet, but it was expected to be. This can be due to a race condition from calling edge before it's ready. This error might auto resolve as soon as edge is able to fetch from upstream".into(),
        )
    })?;
    Ok(Json(frontend_from_yggdrasil(feature_results, true, &token)))
}

#[utoipa::path(
context_path = "/api/proxy",
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
#[get("")]
async fn get_enabled_proxy(
    edge_token: EdgeToken,
    engine_cache: Data<DashMap<String, EngineState>>,
    token_cache: Data<DashMap<String, EdgeToken>>,
    context: QsQuery<Context>,
    req: HttpRequest,
) -> EdgeJsonResult<FrontendResult> {
    get_enabled_features(
        edge_token,
        engine_cache,
        token_cache,
        context.into_inner(),
        req.extensions().get::<ClientIp>().cloned(),
    )
}

#[utoipa::path(
context_path = "/api/frontend",
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
#[get("")]
async fn get_enabled_frontend(
    edge_token: EdgeToken,
    engine_cache: Data<DashMap<String, EngineState>>,
    token_cache: Data<DashMap<String, EdgeToken>>,
    context: QsQuery<Context>,
    req: HttpRequest,
) -> EdgeJsonResult<FrontendResult> {
    debug!("getting enabled features");
    let client_ip = req.extensions().get::<ClientIp>().cloned();
    get_enabled_features(
        edge_token,
        engine_cache,
        token_cache,
        context.into_inner(),
        client_ip,
    )
}

fn get_enabled_features(
    edge_token: EdgeToken,
    engine_cache: Data<DashMap<String, EngineState>>,
    token_cache: Data<DashMap<String, EdgeToken>>,
    incoming_context: Context,
    client_ip: Option<ClientIp>,
) -> EdgeJsonResult<FrontendResult> {
    let context: Context = incoming_context.into();
    let context_with_ip = if context.remote_address.is_none() {
        Context {
            remote_address: client_ip.map(|ip| ip.to_string()),
            ..context
        }
    } else {
        context
    };
    let token = token_cache
        .get(&edge_token.token)
        .map(|e| e.value().clone())
        .unwrap_or_else(|| edge_token.clone());
    let key = cache_key(&token);
    let engine = engine_cache.get(&key).ok_or_else(|| {
        EdgeError::FrontendNotYetHydrated(FrontendHydrationMissing::from(&edge_token))
    })?;
    let feature_results = engine.resolve_all(&context_with_ip, &None).ok_or_else(|| {
        EdgeError::FrontendExpectedToBeHydrated(
            "Feature cache has not been hydrated yet, but it was expected to be. This can be due to a race condition from calling edge before it's ready. This error might auto resolve as soon as edge is able to fetch from upstream".into(),
        )
    })?;
    Ok(Json(frontend_from_yggdrasil(
        feature_results,
        false,
        &token,
    )))
}

#[utoipa::path(
context_path = "/api/proxy",
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
#[post("")]
async fn post_proxy_enabled_features(
    edge_token: EdgeToken,
    engine_cache: Data<DashMap<String, EngineState>>,
    token_cache: Data<DashMap<String, EdgeToken>>,
    context: Json<Context>,
    req: HttpRequest,
) -> EdgeJsonResult<FrontendResult> {
    let client_ip = req.extensions().get::<ClientIp>().cloned();
    post_enabled_features(edge_token, engine_cache, token_cache, context, client_ip).await
}

#[utoipa::path(
context_path = "/api/frontend",
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
#[post("")]
async fn post_frontend_enabled_features(
    edge_token: EdgeToken,
    engine_cache: Data<DashMap<String, EngineState>>,
    token_cache: Data<DashMap<String, EdgeToken>>,
    context: Json<Context>,
    req: HttpRequest,
) -> EdgeJsonResult<FrontendResult> {
    let client_ip = req.extensions().get::<ClientIp>().cloned();
    post_enabled_features(edge_token, engine_cache, token_cache, context, client_ip).await
}

#[utoipa::path(
context_path = "/api/frontend",
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
#[post("/features/{feature_name}")]
pub async fn post_frontend_evaluate_single_feature(
    edge_token: EdgeToken,
    feature_name: Path<String>,
    context: Json<Context>,
    engine_cache: Data<DashMap<String, EngineState>>,
    token_cache: Data<DashMap<String, EdgeToken>>,
    req: HttpRequest,
) -> EdgeJsonResult<EvaluatedToggle> {
    evaluate_feature(
        edge_token,
        feature_name.into_inner(),
        &context.into_inner().into(),
        token_cache,
        engine_cache,
        req.extensions().get::<ClientIp>().cloned(),
    )
    .map(Json)
}

#[utoipa::path(
context_path = "/api/frontend",
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
#[get("/features/{feature_name}")]
pub async fn get_frontend_evaluate_single_feature(
    edge_token: EdgeToken,
    feature_name: Path<String>,
    context: QsQuery<Context>,
    engine_cache: Data<DashMap<String, EngineState>>,
    token_cache: Data<DashMap<String, EdgeToken>>,
    req: HttpRequest,
) -> EdgeJsonResult<EvaluatedToggle> {
    evaluate_feature(
        edge_token,
        feature_name.into_inner(),
        &context.into_inner().into(),
        token_cache,
        engine_cache,
        req.extensions().get::<ClientIp>().cloned(),
    )
    .map(Json)
}

pub fn evaluate_feature(
    edge_token: EdgeToken,
    feature_name: String,
    incoming_context: &Context,
    token_cache: Data<DashMap<String, EdgeToken>>,
    engine_cache: Data<DashMap<String, EngineState>>,
    client_ip: Option<ClientIp>,
) -> EdgeResult<EvaluatedToggle> {
    let context: Context = incoming_context.clone();
    let context_with_ip = if context.remote_address.is_none() {
        Context {
            remote_address: client_ip.map(|ip| ip.to_string()),
            ..context
        }
    } else {
        context
    };
    let validated_token = token_cache
        .get(&edge_token.token)
        .ok_or(EdgeError::EdgeTokenError)?
        .value()
        .clone();
    engine_cache
        .get(&cache_key(&validated_token))
        .and_then(|engine| engine.resolve(&feature_name, &context_with_ip, &None))
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
            impressionData: r.impression_data,
        })
        .ok_or_else(|| EdgeError::FeatureNotFound(feature_name.clone()))
}

async fn post_enabled_features(
    edge_token: EdgeToken,
    engine_cache: Data<DashMap<String, EngineState>>,
    token_cache: Data<DashMap<String, EdgeToken>>,
    context: Json<Context>,
    client_ip: Option<ClientIp>,
) -> EdgeJsonResult<FrontendResult> {
    let context: Context = context.into_inner();
    let context_with_ip = if context.remote_address.is_none() {
        Context {
            remote_address: client_ip.map(|ip| ip.to_string()),
            ..context
        }
    } else {
        context
    };
    let token = token_cache
        .get(&edge_token.token)
        .map(|e| e.value().clone())
        .unwrap_or_else(|| edge_token.clone());
    let engine = engine_cache
        .get(&tokens::cache_key(&edge_token))
        .ok_or_else(|| {
            EdgeError::FrontendNotYetHydrated(FrontendHydrationMissing::from(&edge_token))
        })?;
    let feature_results = engine.resolve_all(&context_with_ip, &None).ok_or_else(|| {
        EdgeError::FrontendExpectedToBeHydrated(
            "Feature cache has not been hydrated yet, but it was expected to be. This can be due to a race condition from calling edge before it's ready. This error might auto resolve as soon as edge is able to fetch from upstream".into(),
        )
    })?;

    Ok(Json(frontend_from_yggdrasil(
        feature_results,
        false,
        &token,
    )))
}

#[utoipa::path(
context_path = "/api/proxy",
responses(
(status = 202, description = "Accepted client metrics"),
(status = 403, description = "Was not allowed to post metrics"),
),
request_body = ClientMetrics,
security(
("Authorization" = [])
)
)]
#[post("/client/metrics")]
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
context_path = "/api/frontend",
responses(
(status = 202, description = "Accepted client metrics"),
(status = 403, description = "Was not allowed to post metrics"),
),
request_body = ClientMetrics,
security(
("Authorization" = [])
)
)]
#[post("/client/metrics")]
async fn post_frontend_metrics(
    edge_token: EdgeToken,
    connect_via: Data<ConnectVia>,
    metrics: Json<ClientMetrics>,
    metrics_cache: Data<MetricsCache>,
    sdk_version: UnleashSdkHeader,
) -> EdgeResult<HttpResponse> {
    if let Some(version) = sdk_version.0 {
        crate::metrics::client_metrics::register_client_application(
            edge_token.clone(),
            &connect_via,
            ClientApplication {
                app_name: metrics.app_name.clone(),
                environment: metrics.environment.clone(),
                projects: Some(edge_token.projects.clone()),
                instance_id: metrics.instance_id.clone(),
                connect_via: None,
                connection_id: None,
                interval: 15000,
                started: Utc::now(),
                strategies: vec![],
                metadata: MetricsMetadata {
                    sdk_version: Some(version),
                    sdk_type: Some(SdkType::Frontend),
                    platform_name: None,
                    platform_version: None,
                    yggdrasil_version: None,
                },
            },
            metrics_cache.clone(),
        );
    }

    crate::metrics::client_metrics::register_client_metrics(
        edge_token,
        metrics.into_inner(),
        metrics_cache,
    );

    Ok(HttpResponse::Accepted().finish())
}

#[utoipa::path(
context_path = "/api/proxy",
responses(
(status = 202, description = "Accepted client application registration"),
(status = 403, description = "Was not allowed to register client"),
),
request_body = ClientApplication,
security(
("Authorization" = [])
)
)]
#[post("/client/register")]
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
context_path = "/api/frontend",
responses(
(status = 202, description = "Accepted client application registration"),
(status = 403, description = "Was not allowed to register client"),
),
request_body = ClientApplication,
security(
("Authorization" = [])
)
)]
#[post("/client/register")]
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

fn configure_frontend_endpoints(cfg: &mut web::ServiceConfig, disable_all_endpoint: bool) {
    if !disable_all_endpoint {
        cfg.service(
            scope_with_auth("/frontend")
                .service(get_frontend_all_features)
                .service(post_frontend_all_features)
                .service(get_enabled_frontend)
                .service(post_frontend_metrics)
                .service(post_frontend_enabled_features)
                .service(post_frontend_register)
                .service(post_frontend_evaluate_single_feature)
                .service(get_frontend_evaluate_single_feature)
                .service(post_all_frontend_metrics),
        );
    } else {
        cfg.service(
            scope_with_auth("/frontend")
                .service(get_enabled_frontend)
                .service(post_frontend_metrics)
                .service(post_frontend_enabled_features)
                .service(post_frontend_register)
                .service(post_frontend_evaluate_single_feature)
                .service(get_frontend_evaluate_single_feature),
        );
    }
}

fn scope_with_auth(
    path: &str,
) -> Scope<
    impl ServiceFactory<
        ServiceRequest,
        Config = (),
        Response = ServiceResponse<impl MessageBody>,
        Error = actix_web::Error,
        InitError = (),
    >,
> {
    web::scope(path)
        .wrap(crate::middleware::as_async_middleware::as_async_middleware(
            crate::middleware::enrich_with_client_ip::enrich_with_client_ip,
        ))
        .wrap(crate::middleware::as_async_middleware::as_async_middleware(
            crate::middleware::client_token_from_frontend_token::client_token_from_frontend_token,
        ))
        .wrap(crate::middleware::as_async_middleware::as_async_middleware(
            crate::middleware::validate_token::validate_token,
        ))
        .wrap(crate::middleware::as_async_middleware::as_async_middleware(
            crate::middleware::consumption::request_consumption,
        ))
}

fn configure_proxy_endpoints(cfg: &mut web::ServiceConfig, disable_all_endpoint: bool) {
    if !disable_all_endpoint {
        cfg.service(
            scope_with_auth("/proxy")
                .service(get_proxy_all_features)
                .service(post_proxy_all_features)
                .service(get_enabled_proxy)
                .service(post_proxy_metrics)
                .service(post_proxy_enabled_features)
                .service(post_proxy_register)
                .service(post_all_proxy_metrics),
        );
    } else {
        cfg.service(
            scope_with_auth("/proxy")
                .service(get_enabled_proxy)
                .service(post_proxy_metrics)
                .service(post_proxy_enabled_features)
                .service(post_proxy_register),
        );
    }
}

pub fn configure_frontend_api(cfg: &mut web::ServiceConfig, disable_all_endpoint: bool) {
    configure_proxy_endpoints(cfg, disable_all_endpoint);
    configure_frontend_endpoints(cfg, disable_all_endpoint);
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
            impressionData: resolved.impression_data,
        })
        .sorted_by_key(|toggle| toggle.name.clone())
        .collect::<Vec<EvaluatedToggle>>();
    FrontendResult { toggles }
}

pub fn get_all_features(
    edge_token: EdgeToken,
    engine_cache: Data<DashMap<String, EngineState>>,
    token_cache: Data<DashMap<String, EdgeToken>>,
    context: &Context,
    client_ip: Option<&ClientIp>,
) -> EdgeJsonResult<FrontendResult> {
    let context_with_ip = if context.remote_address.is_none() {
        &Context {
            remote_address: client_ip.map(|ip| ip.to_string()),
            ..context.clone()
        }
    } else {
        context
    };
    let token = token_cache
        .get(&edge_token.token)
        .map(|e| e.value().clone())
        .unwrap_or_else(|| edge_token.clone());
    let key = cache_key(&token);
    let engine = engine_cache.get(&key).ok_or_else(|| {
        EdgeError::FrontendNotYetHydrated(FrontendHydrationMissing::from(&edge_token))
    })?;
    let feature_results = engine.resolve_all(context_with_ip, &None).ok_or_else(|| {
        EdgeError::FrontendExpectedToBeHydrated(
            "Feature cache has not been hydrated yet, but it was expected to be. This can be due to a race condition from calling edge before it's ready. This error might auto resolve as soon as edge is able to fetch from upstream".into(),
        )
    })?;
    Ok(Json(frontend_from_yggdrasil(feature_results, true, &token)))
}

#[cfg(test)]
mod tests {
    use actix_http::{Request, StatusCode};
    use actix_middleware_etag::Etag;
    use actix_web::{
        App,
        http::header::ContentType,
        test,
        web::{self, Data},
    };
    use chrono::{DateTime, Utc};
    use dashmap::DashMap;
    use serde_json::json;
    use std::collections::HashMap;
    use std::net::SocketAddr;
    use std::str::FromStr;
    use std::sync::Arc;
    use tracing_test::traced_test;
    use unleash_types::client_metrics::{ClientMetricsEnv, MetricsMetadata};
    use unleash_types::{
        client_features::{ClientFeature, ClientFeatures, Constraint, Operator, Strategy},
        frontend::{EvaluatedToggle, EvaluatedVariant, FrontendResult},
    };
    use unleash_yggdrasil::EngineState;

    use crate::cli::{EdgeMode, OfflineArgs, TrustProxy};
    use crate::metrics::client_metrics::MetricsCache;
    use crate::metrics::client_metrics::MetricsKey;
    use crate::middleware;
    use crate::types::{EdgeToken, TokenType, TokenValidationStatus};
    use crate::{builder::build_offline_mode, feature_cache::FeatureCache};

    async fn make_test_request() -> Request {
        make_test_request_to("/api/proxy/client/metrics").await
    }

    async fn make_test_request_to(path: &str) -> Request {
        test::TestRequest::post()
            .uri(path)
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
                    variants: None,
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
            meta: None,
        }
    }

    fn client_features_with_constraint_requiring_test_property_to_be_42() -> ClientFeatures {
        ClientFeatures {
            version: 1,
            features: vec![ClientFeature {
                name: "test".into(),
                enabled: true,
                strategies: Some(vec![Strategy {
                    name: "default".into(),
                    sort_order: None,
                    segments: None,
                    variants: None,
                    constraints: Some(vec![Constraint {
                        context_name: "test_property".into(),
                        operator: Operator::In,
                        case_insensitive: false,
                        inverted: false,
                        values: Some(vec!["42".into()]),
                        value: None,
                    }]),
                    parameters: None,
                }]),
                ..ClientFeature::default()
            }],
            segments: None,
            query: None,
            meta: None,
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
            meta: None,
        }
    }

    #[actix_web::test]
    #[traced_test]
    async fn calling_post_requests_resolves_context_values_correctly() {
        let (token_cache, features_cache, _delta_cache, engine_cache) = build_offline_mode(
            client_features_with_constraint_requiring_user_id_of_seven(),
            vec![
                "*:development.03fa5f506428fe80ed5640c351c7232e38940814d2923b08f5c05fa7"
                    .to_string(),
            ],
            vec![],
            vec![],
        )
        .unwrap();

        let app = test::init_service(
            App::new()
                .app_data(Data::from(token_cache))
                .app_data(Data::from(features_cache))
                .app_data(Data::from(engine_cache))
                .service(web::scope("/api/frontend").service(super::post_frontend_all_features)),
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
        assert!(result.toggles.first().unwrap().enabled)
    }

    #[actix_web::test]
    #[traced_test]
    async fn calling_get_requests_resolves_context_values_correctly() {
        let (token_cache, feature_cache, _delta_cache, engine_cache) = build_offline_mode(
            client_features_with_constraint_requiring_user_id_of_seven(),
            vec![
                "*:development.03fa5f506428fe80ed5640c351c7232e38940814d2923b08f5c05fa7"
                    .to_string(),
            ],
            vec![],
            vec![],
        )
        .unwrap();
        let app = test::init_service(
            App::new()
                .app_data(Data::from(token_cache))
                .app_data(Data::from(feature_cache))
                .app_data(Data::from(engine_cache))
                .service(web::scope("/api/proxy").service(super::get_proxy_all_features)),
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
                impressionData: false,
            }],
        };

        assert_eq!(result, serde_json::to_vec(&expected).unwrap());
    }

    #[actix_web::test]
    #[traced_test]
    async fn calling_get_requests_resolves_top_level_properties_correctly() {
        let (token_cache, feature_cache, _delta_cache, engine_cache) = build_offline_mode(
            client_features_with_constraint_requiring_test_property_to_be_42(),
            vec![
                "*:development.03fa5f506428fe80ed5640c351c7232e38940814d2923b08f5c05fa7"
                    .to_string(),
            ],
            vec![],
            vec![],
        )
        .unwrap();
        let app = test::init_service(
            App::new()
                .app_data(Data::from(token_cache))
                .app_data(Data::from(feature_cache))
                .app_data(Data::from(engine_cache))
                .service(web::scope("/api/frontend").service(super::get_enabled_frontend))
                .service(web::scope("/api/proxy").service(super::get_enabled_proxy))
                .service(web::scope("/api/frontend_all").service(super::get_frontend_all_features))
                .service(web::scope("/api/proxy_all").service(super::get_proxy_all_features)),
        )
        .await;

        let req = |endpoint| {
            test::TestRequest::get()
                .uri(format!("/api/{endpoint}?test_property=42").as_str())
                .insert_header((
                    "Authorization",
                    "*:development.03fa5f506428fe80ed5640c351c7232e38940814d2923b08f5c05fa7",
                ))
                .to_request()
        };

        let frontend_result = test::call_and_read_body(&app, req("frontend")).await;
        let proxy_result = test::call_and_read_body(&app, req("proxy")).await;
        let proxy_all_result = test::call_and_read_body(&app, req("proxy_all/all")).await;
        let frontend_all_result = test::call_and_read_body(&app, req("frontend_all/all")).await;
        assert_eq!(frontend_result, proxy_result);
        assert_eq!(frontend_result, frontend_all_result);
        assert_eq!(proxy_all_result, frontend_all_result);

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
                impressionData: false,
            }],
        };

        assert_eq!(frontend_result, serde_json::to_vec(&expected).unwrap());
    }

    #[actix_web::test]
    #[traced_test]
    async fn calling_post_requests_resolves_top_level_properties_correctly() {
        let (token_cache, feature_cache, _delta_cache, engine_cache) = build_offline_mode(
            client_features_with_constraint_requiring_test_property_to_be_42(),
            vec![
                "*:development.03fa5f506428fe80ed5640c351c7232e38940814d2923b08f5c05fa7"
                    .to_string(),
            ],
            vec![],
            vec![],
        )
        .unwrap();
        let app = test::init_service(
            App::new()
                .app_data(Data::from(token_cache))
                .app_data(Data::from(feature_cache))
                .app_data(Data::from(engine_cache))
                .service(web::scope("/api/frontend").service(super::post_frontend_enabled_features))
                .service(web::scope("/api/proxy").service(super::post_proxy_enabled_features)),
        )
        .await;

        let req = |endpoint| {
            test::TestRequest::post()
                .uri(format!("/api/{endpoint}").as_str())
                .insert_header(ContentType::json())
                .insert_header((
                    "Authorization",
                    "*:development.03fa5f506428fe80ed5640c351c7232e38940814d2923b08f5c05fa7",
                ))
                .set_json(json!({
                    "test_property": "42"
                }))
                .to_request()
        };

        let frontend_result = test::call_and_read_body(&app, req("frontend")).await;
        let proxy_result = test::call_and_read_body(&app, req("proxy")).await;
        assert_eq!(frontend_result, proxy_result);

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
                impressionData: false,
            }],
        };

        assert_eq!(frontend_result, serde_json::to_vec(&expected).unwrap());
    }

    #[actix_web::test]
    #[traced_test]
    async fn calling_get_requests_resolves_context_values_correctly_with_enabled_filter() {
        let (token_cache, features_cache, _delta_cache, engine_cache) = build_offline_mode(
            client_features_with_constraint_one_enabled_toggle_and_one_disabled_toggle(),
            vec![
                "*:development.03fa5f506428fe80ed5640c351c7232e38940814d2923b08f5c05fa7"
                    .to_string(),
            ],
            vec![],
            vec![],
        )
        .unwrap();

        let app = test::init_service(
            App::new()
                .app_data(Data::from(token_cache))
                .app_data(Data::from(features_cache))
                .app_data(Data::from(engine_cache))
                .service(web::scope("/api/proxy").service(super::get_enabled_proxy)),
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
                .service(web::scope("/api/proxy").service(super::post_proxy_metrics)),
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
    }

    #[actix_web::test]
    async fn metrics_all_does_the_same_thing_as_base_metrics() {
        let metrics_cache = Arc::new(MetricsCache::default());

        let app = test::init_service(
            App::new()
                .app_data(Data::from(metrics_cache.clone()))
                .service(web::scope("/api/proxy").service(super::post_proxy_metrics))
                .service(web::scope("/api/frontend").service(super::post_all_frontend_metrics)),
        )
        .await;

        let req = make_test_request_to("/api/proxy/client/metrics").await;
        test::call_and_read_body(&app, req).await;

        let req = make_test_request_to("/api/frontend/all/client/metrics").await;
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

        assert_eq!(found_metric.yes, 2);
        assert_eq!(found_metric.no, 0);
    }

    #[tokio::test]
    async fn when_running_in_offline_mode_with_proxy_key_should_not_filter_features() {
        let client_features = client_features_with_constraint_requiring_user_id_of_seven();
        let (token_cache, feature_cache, _delta_cache, engine_cache) = build_offline_mode(
            client_features.clone(),
            vec!["secret-123".to_string()],
            vec![],
            vec![],
        )
        .unwrap();
        let app = test::init_service(
            App::new()
                .app_data(Data::from(token_cache))
                .app_data(Data::from(feature_cache))
                .app_data(Data::from(engine_cache))
                .app_data(Data::new(EdgeMode::Offline(OfflineArgs {
                    bootstrap_file: None,
                    tokens: vec!["secret-123".into()],
                    reload_interval: 0,
                    client_tokens: vec![],
                    frontend_tokens: vec![],
                })))
                .service(web::scope("/api/frontend").service(super::get_frontend_all_features)),
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
        let (token_cache, feature_cache, _delta_cache, engine_cache) = build_offline_mode(
            client_features.clone(),
            vec!["dx:development.secret123".to_string()],
            vec![],
            vec![],
        )
        .unwrap();
        let app = test::init_service(
            App::new()
                .app_data(Data::from(token_cache))
                .app_data(Data::from(feature_cache))
                .app_data(Data::from(engine_cache))
                .service(web::scope("/api/frontend").service(super::get_frontend_all_features)),
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
    async fn frontend_token_without_matching_client_token_yields_511_when_trying_to_access_frontend_api()
     {
        let features_cache = Arc::new(FeatureCache::default());
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
                .service(
                    web::scope("/api").configure(|cfg| super::configure_frontend_api(cfg, false)),
                ),
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
        let features_cache = Arc::new(FeatureCache::default());
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
                .service(
                    web::scope("/api").configure(|cfg| super::configure_frontend_api(cfg, false)),
                ),
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
        let (token_cache, feature_cache, _delta_cache, engine_cache) = build_offline_mode(
            client_features.clone(),
            vec!["dx:development.secret123".to_string()],
            vec![],
            vec![],
        )
        .unwrap();
        let app = test::init_service(
            App::new()
                .app_data(Data::from(token_cache))
                .app_data(Data::from(feature_cache))
                .app_data(Data::from(engine_cache))
                .service(
                    web::scope("/api").configure(|cfg| super::configure_frontend_api(cfg, false)),
                ),
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
    async fn can_get_single_feature_with_top_level_properties() {
        let (token_cache, feature_cache, _delta_cache, engine_cache) = build_offline_mode(
            client_features_with_constraint_requiring_test_property_to_be_42(),
            vec!["*:development.secret123".to_string()],
            vec![],
            vec![],
        )
        .unwrap();
        let app = test::init_service(
            App::new()
                .app_data(Data::from(token_cache))
                .app_data(Data::from(feature_cache))
                .app_data(Data::from(engine_cache))
                .service(
                    web::scope("/api").configure(|cfg| super::configure_frontend_api(cfg, false)),
                ),
        )
        .await;

        let req = test::TestRequest::get()
            .uri("/api/frontend/features/test?test_property=42")
            .insert_header(("Authorization", "*:development.secret123"))
            .to_request();

        let result = test::call_service(&app, req).await;
        assert_eq!(result.status(), 200);
    }

    #[tokio::test]
    async fn trying_to_evaluate_feature_you_do_not_have_access_to_will_give_not_found() {
        let client_features = crate::tests::features_from_disk("../examples/hostedexample.json");
        let (token_cache, feature_cache, _delta_cache, engine_cache) = build_offline_mode(
            client_features.clone(),
            vec!["dx:development.secret123".to_string()],
            vec![],
            vec![],
        )
        .unwrap();
        let app = test::init_service(
            App::new()
                .app_data(Data::from(token_cache))
                .app_data(Data::from(feature_cache))
                .app_data(Data::from(engine_cache))
                .service(
                    web::scope("/api").configure(|cfg| super::configure_frontend_api(cfg, false)),
                ),
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
        let (token_cache, feature_cache, _delta_cache, engine_cache) = build_offline_mode(
            client_features_with_custom_context_field.clone(),
            vec![auth_key.clone()],
            vec![],
            vec![],
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
                .service(
                    web::scope("/api").configure(|cfg| super::configure_frontend_api(cfg, false)),
                ),
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
    #[traced_test]
    async fn can_handle_custom_context_fields_with_post() {
        let client_features_with_custom_context_field =
            crate::tests::features_from_disk("../examples/with_custom_constraint.json");
        let auth_key = "default:development.secret123".to_string();
        let (token_cache, feature_cache, _delta_cache, engine_cache) = build_offline_mode(
            client_features_with_custom_context_field.clone(),
            vec![auth_key.clone()],
            vec![],
            vec![],
        )
        .unwrap();
        let trust_proxy = TrustProxy {
            trust_proxy: true,
            proxy_trusted_servers: vec![],
        };
        let app = test::init_service(
            App::new()
                .app_data(Data::new(trust_proxy.clone()))
                .app_data(Data::from(token_cache))
                .app_data(Data::from(feature_cache))
                .app_data(Data::from(engine_cache))
                .service(
                    web::scope("/api").configure(|cfg| super::configure_frontend_api(cfg, false)),
                ),
        )
        .await;
        let req = test::TestRequest::post()
            .uri("/api/frontend")
            .insert_header(ContentType::json())
            .insert_header(("Authorization", auth_key.clone()))
            .set_json(json!({ "properties": {"companyId": "bricks"}}))
            .to_request();
        let result: FrontendResult = test::try_call_and_read_body_json(&app, req)
            .await
            .expect("Failed to call endpoint");
        tracing::info!("{result:?}");
        assert_eq!(result.toggles.len(), 1);
    }

    #[tokio::test]
    #[traced_test]
    async fn will_evaluate_ip_strategy_populated_from_middleware() {
        let client_features_with_custom_context_field =
            crate::tests::features_from_disk("../examples/ip_address_feature.json");
        let auth_key = "gard:development.secret123".to_string();
        let (token_cache, feature_cache, _delta_cache, engine_cache) = build_offline_mode(
            client_features_with_custom_context_field.clone(),
            vec![auth_key.clone()],
            vec![],
            vec![],
        )
        .unwrap();
        let trust_proxy = TrustProxy {
            trust_proxy: true,
            proxy_trusted_servers: vec![],
        };
        let app = test::init_service(
            App::new()
                .app_data(Data::new(trust_proxy.clone()))
                .app_data(Data::from(token_cache))
                .app_data(Data::from(feature_cache))
                .app_data(Data::from(engine_cache))
                .service(
                    web::scope("/api").configure(|cfg| super::configure_frontend_api(cfg, false)),
                ),
        )
        .await;
        let req = test::TestRequest::post()
            .uri("/api/frontend")
            .peer_addr(SocketAddr::from_str("192.168.0.1:80").unwrap())
            .insert_header(ContentType::json())
            .insert_header(("Authorization", auth_key.clone()))
            .set_json(json!({ "properties": {"companyId": "bricks"}}))
            .to_request();
        let result: FrontendResult = test::call_and_read_body_json(&app, req).await;
        let ip_addr_was_enabled = result.toggles.iter().any(|r| r.name == "ip_addr");
        assert!(ip_addr_was_enabled);
    }

    #[tokio::test]
    #[traced_test]
    async fn disabling_all_endpoints_yields_404_when_trying_to_access_them() {
        let client_features_with_custom_context_field =
            crate::tests::features_from_disk("../examples/ip_address_feature.json");
        let auth_key = "gard:development.secret123".to_string();
        let (token_cache, feature_cache, _delta_cache, engine_cache) = build_offline_mode(
            client_features_with_custom_context_field.clone(),
            vec![auth_key.clone()],
            vec![],
            vec![],
        )
        .unwrap();
        let app = test::init_service(
            App::new()
                .app_data(Data::from(token_cache))
                .app_data(Data::from(feature_cache))
                .app_data(Data::from(engine_cache))
                .service(
                    web::scope("/api").configure(|cfg| super::configure_frontend_api(cfg, true)),
                ),
        )
        .await;
        let frontend_req = test::TestRequest::post()
            .uri("/api/frontend/all")
            .peer_addr(SocketAddr::from_str("192.168.0.1:80").unwrap())
            .insert_header(ContentType::json())
            .insert_header(("Authorization", auth_key.clone()))
            .set_json(json!({ "properties": {"companyId": "bricks"}}))
            .to_request();
        let result = test::call_service(&app, frontend_req).await;
        assert_eq!(result.status(), StatusCode::NOT_FOUND);
        let proxy_req = test::TestRequest::post()
            .uri("/api/proxy/all")
            .peer_addr(SocketAddr::from_str("192.168.0.1:80").unwrap())
            .insert_header(ContentType::json())
            .insert_header(("Authorization", auth_key.clone()))
            .set_json(json!({ "properties": {"companyId": "bricks"}}))
            .to_request();
        let result = test::call_service(&app, proxy_req).await;
        assert_eq!(result.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn can_handle_custom_context_fields_on_all_endpoint() {
        let client_features_with_custom_context_field =
            crate::tests::features_from_disk("../examples/with_custom_constraint.json");
        let auth_key = "default:development.secret123".to_string();
        let (token_cache, feature_cache, _delta_cache, engine_cache) = build_offline_mode(
            client_features_with_custom_context_field.clone(),
            vec![auth_key.clone()],
            vec![],
            vec![],
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
                .service(
                    web::scope("/api").configure(|cfg| super::configure_frontend_api(cfg, false)),
                ),
        )
        .await;
        let req = test::TestRequest::get()
            .uri("/api/frontend/all?properties[companyId]=bricks")
            .insert_header(ContentType::json())
            .insert_header(("Authorization", auth_key.clone()))
            .to_request();
        let feature_results: FrontendResult = test::call_and_read_body_json(&app, req).await;
        assert!(feature_results.toggles.iter().any(|f| f.enabled));
        let req = test::TestRequest::get()
            .uri("/api/frontend?properties%5BcompanyId%5D=bricks")
            .insert_header(ContentType::json())
            .insert_header(("Authorization", auth_key.clone()))
            .to_request();
        let feature_results: FrontendResult = test::call_and_read_body_json(&app, req).await;
        assert!(feature_results.toggles.iter().any(|f| f.enabled));
    }

    #[tokio::test]
    async fn assert_frontend_sort_order_is_stable() {
        let client_features_with_custom_context_field =
            crate::tests::features_from_disk("../examples/frontend-stable-sort.json");
        let auth_key = "default:development.secret123".to_string();
        let (token_cache, feature_cache, _delta_cache, engine_cache) = build_offline_mode(
            client_features_with_custom_context_field.clone(),
            vec![auth_key.clone()],
            vec![],
            vec![],
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
                .wrap(Etag)
                .service(
                    web::scope("/api").configure(|cfg| super::configure_frontend_api(cfg, false)),
                ),
        )
        .await;
        let req = test::TestRequest::get()
            .uri("/api/frontend/all?properties[companyId]=bricks")
            .insert_header(ContentType::json())
            .insert_header(("Authorization", auth_key.clone()))
            .to_request();
        let result = test::call_service(&app, req).await;
        let etag_header = result.headers().get("ETag").unwrap();

        for _i in 1..10 {
            let another_call = test::TestRequest::get()
                .uri("/api/frontend?properties[companyId]=bricks")
                .insert_header(ContentType::json())
                .insert_header(("If-None-Match", etag_header.to_str().unwrap()))
                .insert_header(("Authorization", auth_key.clone()))
                .to_request();
            let result = test::call_service(&app, another_call).await;
            assert_eq!(result.status(), StatusCode::NOT_MODIFIED);
        }
    }
}
