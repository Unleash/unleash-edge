use actix_web::{
    get, post,
    web::{self, Json},
};
use unleash_types::{
    client_features::{ClientFeatures, Payload},
    frontend::{EvaluatedToggle, EvaluatedVariant, FrontendResult},
};
use unleash_yggdrasil::{Context, EngineState};

use crate::types::{EdgeJsonResult, EdgeSource, EdgeToken};

#[get("/proxy/all")]
async fn get_frontend_features(
    edge_token: EdgeToken,
    features_source: web::Data<dyn EdgeSource>,
    context: web::Query<Context>,
) -> EdgeJsonResult<FrontendResult> {
    let client_features = features_source.get_client_features(&edge_token).await;
    let context = context.into_inner();

    let toggles = resolve_frontend_features(client_features?, context).collect();

    Ok(Json(FrontendResult { toggles }))
}

#[post("/proxy/all")]
async fn post_frontend_features(
    edge_token: EdgeToken,
    features_source: web::Data<dyn EdgeSource>,
    context: web::Json<Context>,
) -> EdgeJsonResult<FrontendResult> {
    let client_features = features_source.get_client_features(&edge_token).await;
    let context = context.into_inner();

    let toggles = resolve_frontend_features(client_features?, context).collect();

    Ok(Json(FrontendResult { toggles }))
}

#[get("/proxy")]
async fn get_enabled_frontend_features(
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

#[post("/proxy")]
async fn post_enabled_frontend_features(
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
    cfg.service(get_frontend_features);
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::types::{
        EdgeProvider, EdgeResult, EdgeSink, EdgeSource, EdgeToken, FeatureSink, FeaturesSource,
        TokenSink, TokenSource,
    };
    use actix_web::{
        http::header::ContentType,
        test,
        web::{self, Data},
        App,
    };
    use async_trait::async_trait;
    use serde_json::json;
    use tokio::sync::mpsc::Sender;
    use unleash_types::{
        client_features::{ClientFeature, ClientFeatures, Constraint, Operator, Strategy},
        frontend::{EvaluatedToggle, EvaluatedVariant, FrontendResult},
    };

    #[derive(Clone, Default)]
    struct MockDataSource {
        features: Option<ClientFeatures>,
    }

    impl MockDataSource {
        fn with(self, features: ClientFeatures) -> Self {
            MockDataSource {
                features: Some(features),
            }
        }
    }

    #[async_trait]
    impl FeaturesSource for MockDataSource {
        async fn get_client_features(&self, _token: &EdgeToken) -> EdgeResult<ClientFeatures> {
            Ok(self
                .features
                .as_ref()
                .expect("You need to populate the mock data for your test")
                .clone())
        }
    }

    #[async_trait]
    impl TokenSource for MockDataSource {
        async fn get_known_tokens(&self) -> EdgeResult<Vec<crate::types::EdgeToken>> {
            todo!()
        }

        async fn secret_is_valid(
            &self,
            _secret: &str,
            _: Arc<Sender<EdgeToken>>,
        ) -> EdgeResult<bool> {
            Ok(true)
        }

        async fn token_details(&self, _secret: String) -> EdgeResult<Option<EdgeToken>> {
            todo!()
        }
    }

    impl EdgeSource for MockDataSource {}

    impl TokenSink for MockDataSource {}

    impl EdgeSink for MockDataSource {}

    impl EdgeProvider for MockDataSource {}

    #[async_trait]
    impl FeatureSink for MockDataSource {
        async fn sink_tokens(&mut self, _token: Vec<EdgeToken>) -> EdgeResult<()> {
            todo!()
        }

        async fn sink_features(
            &mut self,
            _token: &EdgeToken,
            _features: ClientFeatures,
        ) -> EdgeResult<()> {
            todo!()
        }
    }

    impl From<MockDataSource> for Data<dyn EdgeProvider> {
        fn from(val: MockDataSource) -> Self {
            let client_provider_arc: Arc<dyn EdgeProvider> = Arc::new(val);
            Data::from(client_provider_arc)
        }
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
        let client_provider: Data<dyn EdgeProvider> = MockDataSource::default()
            .with(client_features_with_constraint_requiring_user_id_of_seven())
            .into();

        let app = test::init_service(
            App::new()
                .app_data(client_provider)
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
            .set_json(json!({
                "userId": "7"
            }))
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
    async fn calling_get_requests_resolves_context_values_correctly() {
        let client_provider: Data<dyn EdgeProvider> = MockDataSource::default()
            .with(client_features_with_constraint_requiring_user_id_of_seven())
            .into();

        let app = test::init_service(
            App::new()
                .app_data(client_provider)
                .service(web::scope("/api").service(super::get_frontend_features)),
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
        let client_provider: Data<dyn EdgeProvider> = MockDataSource::default()
            .with(client_features_with_constraint_one_enabled_toggle_and_one_disabled_toggle())
            .into();

        let app = test::init_service(
            App::new()
                .app_data(client_provider)
                .service(web::scope("/api").service(super::get_enabled_frontend_features)),
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
}
