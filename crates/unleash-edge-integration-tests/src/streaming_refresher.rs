#[cfg(test)]
mod tests {
    use axum::Router;
    use axum_test::TestServer;
    use eventsource_stream::Eventsource;
    use std::{sync::Arc, time::Duration};
    use tokio::time::timeout;
    use tokio_stream::StreamExt as _;
    use unleash_edge_appstate::AppState;
    use unleash_edge_delta::{
        cache::{DeltaCache, DeltaHydrationEvent},
        cache_manager::DeltaCacheManager,
    };
    use unleash_edge_feature_cache::FeatureCache;
    use unleash_types::client_features::{
        ClientFeature, ClientFeaturesDelta, DeltaEvent, Strategy,
    };

    use unleash_edge_types::{
        EngineCache, TokenCache, TokenType, TokenValidationStatus, tokens::EdgeToken,
    };

    async fn client_api_test_server(
        upstream_token_cache: Arc<TokenCache>,
        upstream_features_cache: Arc<FeatureCache>,
        upstream_engine_cache: Arc<EngineCache>,
        upstream_delta_cache_manager: Arc<DeltaCacheManager>,
    ) -> TestServer {
        let app_state = AppState::builder()
            .with_token_cache(upstream_token_cache.clone())
            .with_features_cache(upstream_features_cache.clone())
            .with_engine_cache(upstream_engine_cache.clone())
            .with_delta_cache_manager(upstream_delta_cache_manager.clone())
            .build();
        let router = Router::new()
            .nest("/api/client", unleash_edge_client_api::router())
            .with_state(app_state);
        TestServer::builder()
            .http_transport()
            .build(router)
            .expect("Failed to build client api test server")
    }

    #[tokio::test]
    pub async fn streaming_compresses_multiple_updates_into_hydration_event() {
        let token = EdgeToken {
            token: "*:development.hashhasin".into(),
            token_type: Some(TokenType::Client),
            environment: Some("development".into()),
            projects: vec!["*".into()],
            status: TokenValidationStatus::Validated,
        };

        let token_cache = Arc::new(TokenCache::default());
        let features_cache = Arc::new(FeatureCache::default());
        let engine_cache = Arc::new(EngineCache::default());
        let delta_cache_manager = Arc::new(DeltaCacheManager::new());

        delta_cache_manager.insert_cache(
            &token.environment.clone().unwrap(),
            DeltaCache::new(
                DeltaHydrationEvent {
                    event_id: 0,
                    features: vec![ClientFeature {
                        name: "Inigo Montoya".into(),
                        project: Some("Princess bride".into()),
                        strategies: Some(vec![]),
                        ..ClientFeature::default()
                    }],
                    segments: vec![],
                },
                10,
            ),
        );

        delta_cache_manager.update_cache(
            &token.environment.clone().unwrap(),
            &vec![DeltaEvent::FeatureUpdated {
                event_id: 1,
                feature: ClientFeature {
                    name: "Inigo Montoya".into(),
                    project: Some("Princess bride".into()),
                    strategies: Some(vec![Strategy {
                        name: "prepare to die".into(),
                        constraints: None,
                        parameters: None,
                        segments: None,
                        sort_order: Some(1),
                        variants: None,
                    }]),
                    ..ClientFeature::default()
                },
            }],
        );

        delta_cache_manager.update_cache(
            &token.environment.clone().unwrap(),
            &vec![DeltaEvent::FeatureUpdated {
                event_id: 1,
                feature: ClientFeature {
                    name: "Westley".into(),
                    project: Some("Princess bride".into()),
                    strategies: Some(vec![Strategy {
                        name: "preparing to die".into(),
                        constraints: None,
                        parameters: None,
                        segments: None,
                        sort_order: Some(1),
                        variants: None,
                    }]),
                    ..ClientFeature::default()
                },
            }],
        );

        token_cache.insert(token.token.clone(), token.clone());

        let test_server = client_api_test_server(
            token_cache,
            features_cache,
            engine_cache,
            delta_cache_manager.clone(),
        )
        .await;
        let url = test_server.server_url("/").unwrap();

        let mut event_stream = reqwest::Client::new()
            .get(format!("{url}api/client/streaming"))
            .header("Authorization", "*:development.hashhasin")
            .send()
            .await
            .unwrap()
            .bytes_stream()
            .eventsource();

        let mut event_data: Vec<ClientFeaturesDelta> = vec![];

        timeout(Duration::from_secs(5), async {
            while let Some(event) = event_stream.next().await {
                match event {
                    Ok(event) => {
                        event_data.push(
                            serde_json::from_str::<ClientFeaturesDelta>(&event.data).unwrap(),
                        );

                        if event_data.len() == 1 {
                            break;
                        }
                    }
                    Err(_) => {
                        panic!("Error in event stream");
                    }
                }
            }
        })
        .await
        .expect("Failed to complete");

        assert!(event_data.len() == 1);

        let DeltaEvent::Hydration {
            event_id,
            features,
            segments,
        } = &event_data[0].events[0]
        else {
            panic!("expected DeltaEvent::Hydration");
        };

        assert!(segments.is_empty());
        assert!(event_id == &1);
        assert!(
            features
                == &vec![
                    ClientFeature {
                        name: "Inigo Montoya".into(),
                        project: Some("Princess bride".into()),
                        strategies: Some(vec![Strategy {
                            name: "prepare to die".into(),
                            constraints: None,
                            parameters: None,
                            segments: None,
                            sort_order: Some(1),
                            variants: None,
                        }]),
                        ..ClientFeature::default()
                    },
                    ClientFeature {
                        name: "Westley".into(),
                        project: Some("Princess bride".into()),
                        strategies: Some(vec![Strategy {
                            name: "preparing to die".into(),
                            constraints: None,
                            parameters: None,
                            segments: None,
                            sort_order: Some(1),
                            variants: None,
                        }]),
                        ..ClientFeature::default()
                    },
                ]
        );
    }

    #[tokio::test]
    pub async fn streaming_sends_multiple_messages() {
        let token = EdgeToken {
            token: "*:development.hashhasin".into(),
            token_type: Some(TokenType::Client),
            environment: Some("development".into()),
            projects: vec!["*".into()],
            status: TokenValidationStatus::Validated,
        };

        let token_cache = Arc::new(TokenCache::default());
        let features_cache = Arc::new(FeatureCache::default());
        let engine_cache = Arc::new(EngineCache::default());
        let delta_cache_manager = Arc::new(DeltaCacheManager::new());

        delta_cache_manager.insert_cache(
            &token.environment.clone().unwrap(),
            DeltaCache::new(
                DeltaHydrationEvent {
                    event_id: 0,
                    features: vec![ClientFeature {
                        name: "Inigo Montoya".into(),
                        project: Some("Princess bride".into()),
                        strategies: Some(vec![]),
                        ..ClientFeature::default()
                    }],
                    segments: vec![],
                },
                10,
            ),
        );

        delta_cache_manager.update_cache(
            &token.environment.clone().unwrap(),
            &vec![DeltaEvent::FeatureUpdated {
                event_id: 1,
                feature: ClientFeature {
                    name: "Inigo Montoya".into(),
                    project: Some("Princess bride".into()),
                    strategies: Some(vec![Strategy {
                        name: "prepare to die".into(),
                        constraints: None,
                        parameters: None,
                        segments: None,
                        sort_order: Some(1),
                        variants: None,
                    }]),
                    ..ClientFeature::default()
                },
            }],
        );

        token_cache.insert(token.token.clone(), token.clone());

        let test_server = client_api_test_server(
            token_cache,
            features_cache,
            engine_cache,
            delta_cache_manager.clone(),
        )
        .await;
        let url = test_server.server_url("/").unwrap();

        let mut event_stream = reqwest::Client::new()
            .get(format!("{url}api/client/streaming"))
            .header("Authorization", "*:development.hashhasin")
            .send()
            .await
            .unwrap()
            .bytes_stream()
            .eventsource();

        let mut event_data: Vec<ClientFeaturesDelta> = vec![];

        let stream_updates = timeout(Duration::from_secs(5), async {
            while let Some(event) = event_stream.next().await {
                match event {
                    Ok(event) => {
                        event_data.push(
                            serde_json::from_str::<ClientFeaturesDelta>(&event.data).unwrap(),
                        );

                        if event_data.len() == 2 {
                            break;
                        }
                    }
                    Err(_) => {
                        panic!("Error in event stream");
                    }
                }
            }
        });

        let inject_event = async move || {
            delta_cache_manager.update_cache(
                &token.environment.clone().unwrap(),
                &vec![DeltaEvent::FeatureUpdated {
                    event_id: 2,
                    feature: ClientFeature {
                        name: "Westley".into(),
                        project: Some("Princess bride".into()),
                        strategies: Some(vec![Strategy {
                            name: "preparing to die".into(),
                            constraints: None,
                            parameters: None,
                            segments: None,
                            sort_order: Some(1),
                            variants: None,
                        }]),
                        ..ClientFeature::default()
                    },
                }],
            );
        };

        let (_, _) = tokio::join!(stream_updates, inject_event());

        assert!(event_data.len() == 2);

        let DeltaEvent::Hydration {
            event_id,
            features,
            segments,
        } = &event_data[0].events[0]
        else {
            panic!("expected DeltaEvent::Hydration");
        };

        assert!(segments.is_empty());
        assert!(event_id == &1);
        assert!(
            features
                == &vec![ClientFeature {
                    name: "Inigo Montoya".into(),
                    project: Some("Princess bride".into()),
                    strategies: Some(vec![Strategy {
                        name: "prepare to die".into(),
                        constraints: None,
                        parameters: None,
                        segments: None,
                        sort_order: Some(1),
                        variants: None,
                    }]),
                    ..ClientFeature::default()
                }]
        );

        let DeltaEvent::FeatureUpdated { event_id, feature } = &event_data[1].events[0] else {
            panic!("expected DeltaEvent::FeatureUpdated");
        };

        assert!(event_id == &2);
        assert!(
            feature
                == &ClientFeature {
                    name: "Westley".into(),
                    project: Some("Princess bride".into()),
                    strategies: Some(vec![Strategy {
                        name: "preparing to die".into(),
                        constraints: None,
                        parameters: None,
                        segments: None,
                        sort_order: Some(1),
                        variants: None,
                    }]),
                    ..ClientFeature::default()
                }
        );
    }

    #[tokio::test]
    async fn streaming_is_terminated_if_token_becomes_invalidated() {
        let token = EdgeToken {
            token: "*:development.hashhasin".into(),
            token_type: Some(TokenType::Client),
            environment: Some("development".into()),
            projects: vec!["*".into()],
            status: TokenValidationStatus::Validated,
        };

        let token_cache = Arc::new(TokenCache::default());
        let features_cache = Arc::new(FeatureCache::default());
        let engine_cache = Arc::new(EngineCache::default());
        let delta_cache_manager = Arc::new(DeltaCacheManager::new());

        delta_cache_manager.insert_cache(
            &token.environment.clone().unwrap(),
            DeltaCache::new(
                DeltaHydrationEvent {
                    event_id: 0,
                    features: vec![ClientFeature {
                        name: "Inigo Montoya".into(),
                        project: Some("Princess bride".into()),
                        strategies: Some(vec![]),
                        ..ClientFeature::default()
                    }],
                    segments: vec![],
                },
                10,
            ),
        );

        delta_cache_manager.update_cache(
            &token.environment.clone().unwrap(),
            &vec![DeltaEvent::FeatureUpdated {
                event_id: 1,
                feature: ClientFeature {
                    name: "Inigo Montoya".into(),
                    project: Some("Princess bride".into()),
                    strategies: Some(vec![Strategy {
                        name: "prepare to die".into(),
                        constraints: None,
                        parameters: None,
                        segments: None,
                        sort_order: Some(1),
                        variants: None,
                    }]),
                    ..ClientFeature::default()
                },
            }],
        );

        token_cache.insert(token.token.clone(), token.clone());

        let test_server = client_api_test_server(
            token_cache.clone(),
            features_cache,
            engine_cache,
            delta_cache_manager.clone(),
        )
        .await;
        let url = test_server.server_url("/").unwrap();

        let mut event_stream = reqwest::Client::new()
            .get(format!("{url}api/client/streaming"))
            .header("Authorization", "*:development.hashhasin")
            .send()
            .await
            .unwrap()
            .bytes_stream()
            .eventsource();

        let mut event_data: Vec<ClientFeaturesDelta> = vec![];

        let stream_updates = timeout(Duration::from_secs(5), async {
            while let Some(event) = event_stream.next().await {
                match event {
                    Ok(event) => {
                        event_data.push(
                            serde_json::from_str::<ClientFeaturesDelta>(&event.data).unwrap(),
                        );
                    }
                    Err(_) => {
                        panic!("Error in event stream");
                    }
                }
            }
        });

        let update_cache_and_invalidate_token = async move || {
            token_cache.insert(
                token.token.clone(),
                EdgeToken {
                    status: TokenValidationStatus::Invalid,
                    ..token.clone()
                },
            );

            delta_cache_manager.update_cache(
                &token.environment.clone().unwrap(),
                &vec![DeltaEvent::FeatureUpdated {
                    event_id: 2,
                    feature: ClientFeature {
                        name: "Westley".into(),
                        project: Some("Princess bride".into()),
                        strategies: Some(vec![Strategy {
                            name: "preparing to die".into(),
                            constraints: None,
                            parameters: None,
                            segments: None,
                            sort_order: Some(1),
                            variants: None,
                        }]),
                        ..ClientFeature::default()
                    },
                }],
            );
        };

        let (_, _) = tokio::join!(stream_updates, update_cache_and_invalidate_token());

        assert!(event_data.len() == 1);
    }
}
