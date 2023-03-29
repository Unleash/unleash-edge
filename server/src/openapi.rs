use utoipa::{
    openapi::security::{ApiKey, ApiKeyValue, SecurityScheme},
    Modify, OpenApi,
};

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::frontend_api::get_enabled_proxy,
        crate::frontend_api::get_enabled_frontend,
        crate::frontend_api::post_proxy_enabled_features,
        crate::frontend_api::post_frontend_enabled_features,
        crate::frontend_api::get_proxy_all_features,
        crate::frontend_api::get_frontend_all_features,
        crate::frontend_api::post_proxy_all_features,
        crate::frontend_api::post_frontend_all_features,
        crate::client_api::get_features,
        crate::client_api::register,
        crate::client_api::metrics,
        crate::client_api::get_feature,
        crate::edge_api::validate,
        crate::edge_api::metrics
    ),
    components(schemas(
        unleash_types::frontend::FrontendResult,
        unleash_types::frontend::EvaluatedToggle,
        unleash_types::frontend::EvaluatedVariant,
        unleash_types::client_features::Payload,
        unleash_types::client_features::ClientFeatures,
        unleash_types::client_features::Context,
        unleash_types::client_features::ClientFeature,
        unleash_types::client_features::Query,
        unleash_types::client_features::Segment,
        unleash_types::client_features::Strategy,
        unleash_types::client_features::Variant,
        unleash_types::client_features::Constraint,
        unleash_types::client_features::Override,
        unleash_types::client_features::WeightType,
        unleash_types::client_features::Operator,
        unleash_types::client_metrics::ClientApplication,
        unleash_types::client_metrics::ClientMetrics,
        unleash_types::client_metrics::ClientMetricsEnv,
        unleash_types::client_metrics::ConnectVia,
        crate::types::TokenStrings,
        crate::types::ValidatedTokens,
        crate::types::BatchMetricsRequestBody,
        crate::types::EdgeToken,
        crate::types::TokenValidationStatus,
        crate::types::TokenType
    )),
    modifiers(&SecurityAddon)
)]
pub struct ApiDoc;

pub struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.as_mut().unwrap();
        components.add_security_scheme(
            "Authorization",
            SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::new("Authorization"))),
        )
    }
}
