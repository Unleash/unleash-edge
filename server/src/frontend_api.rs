use actix_web::{
    get,
    web::{self, Json},
};
use unleash_types::{
    client_features::{ClientFeatures, Payload},
    frontend::{EvaluatedToggle, EvaluatedVariant, FrontendResult},
};
use unleash_yggdrasil::{Context, EngineState};

use crate::types::{EdgeJsonResult, EdgeProvider, EdgeToken};

#[get("/proxy/all")]
async fn get_frontend_features(
    edge_token: EdgeToken,
    features_source: web::Data<dyn EdgeProvider>,
    context: web::Query<Context>,
) -> EdgeJsonResult<FrontendResult> {
    let client_features = features_source.get_client_features(edge_token);
    let context = context.into_inner();

    let evaluated_features = resolve_frontend_features(client_features, context);

    Ok(Json(FrontendResult {
        toggles: evaluated_features,
    }))
}

#[actix_web::post("/proxy/all")]
async fn post_frontend_features(
    edge_token: EdgeToken,
    features_source: web::Data<dyn EdgeProvider>,
    context: web::Json<Context>,
) -> EdgeJsonResult<FrontendResult> {
    let client_features = features_source.get_client_features(edge_token);
    let context = context.into_inner();

    let evaluated_features = resolve_frontend_features(client_features, context);

    Ok(Json(FrontendResult {
        toggles: evaluated_features,
    }))
}

fn resolve_frontend_features(
    client_features: ClientFeatures,
    context: Context,
) -> Vec<EvaluatedToggle> {
    let mut engine = EngineState::default();
    engine.take_state(client_features.clone());

    client_features
        .features
        .into_iter()
        .map(|toggle| {
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
        .collect()
}

pub fn configure_frontend_api(cfg: &mut web::ServiceConfig) {
    cfg.service(get_frontend_features);
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::types::{EdgeProvider, FeaturesProvider, TokenProvider};
    use actix_web::{
        http::header::ContentType,
        test,
        web::{self, Bytes},
        App,
    };
    use serde_json::json;
    use unleash_types::{
        client_features::{ClientFeature, ClientFeatures, Variant},
        frontend::{EvaluatedToggle, EvaluatedVariant, FrontendResult},
    };

    #[derive(Clone)]
    struct MockDataSource {}

    impl FeaturesProvider for MockDataSource {
        fn get_client_features(&self, _token: crate::types::EdgeToken) -> ClientFeatures {
            return ClientFeatures {
                version: 1,
                features: vec![ClientFeature {
                    name: "test".into(),
                    enabled: true,
                    ..ClientFeature::default()
                }],
                segments: None,
                query: None,
            };
        }
    }

    impl TokenProvider for MockDataSource {
        fn get_known_tokens(&self) -> Vec<crate::types::EdgeToken> {
            todo!()
        }

        fn secret_is_valid(&self, _secret: &str) -> bool {
            true
        }

        fn token_details(&self, _secret: String) -> Option<crate::types::EdgeToken> {
            todo!()
        }
    }

    impl EdgeProvider for MockDataSource {}

    #[actix_web::test]
    async fn calling_post_requests_resolves_context_values_correctly() {
        env_logger::init();
        let mock_data = MockDataSource {};
        let client_provider_arc: Arc<dyn EdgeProvider> = Arc::new(mock_data.clone());
        let client_provider_data = web::Data::from(client_provider_arc);

        let app = test::init_service(
            App::new()
                .app_data(client_provider_data)
                .service(web::scope("/api").service(super::post_frontend_features)),
        )
        .await;
        let req = test::TestRequest::post()
            .uri("/api/proxy/all")
            .insert_header(ContentType::json())
            .insert_header((
                "Authorization",
                "*:development.03fa5f506428fe80ed5640c351c7232e38940814d2923b08f5c05fa7",
            ))
            .set_json(json!({}))
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
}
