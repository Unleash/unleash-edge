#[cfg(test)]
mod tests {
    use axum::{Router, extract::FromRef};
    use axum_test::TestServer;
    use eventsource_stream::Eventsource;
    use std::{sync::Arc, time::Duration};
    use tokio::time::timeout;
    use tokio_stream::StreamExt as _;
    use unleash_edge_appstate::edge_token_extractor::AuthState;
    use unleash_edge_cli::AuthHeaders;
    use unleash_edge_client_api::streaming::{StreamingState, streaming_router_for};
    use unleash_edge_delta::{
        cache::{DeltaCache, DeltaHydrationEvent},
        cache_manager::DeltaCacheManager,
    };
    use unleash_types::client_features::{
        ClientFeature, ClientFeaturesDelta, DeltaEvent, Strategy,
    };

    use unleash_edge_types::{TokenCache, TokenType, TokenValidationStatus, tokens::EdgeToken};

    #[derive(Clone)]
    struct TestState {
        auth_headers: AuthHeaders,
        token_cache: Arc<TokenCache>,
        delta_cache_manager: Option<Arc<DeltaCacheManager>>,
    }

    impl FromRef<TestState> for AuthState {
        fn from_ref(s: &TestState) -> Self {
            AuthState {
                auth_headers: s.auth_headers.clone(),
                token_cache: Arc::clone(&s.token_cache),
            }
        }
    }

    impl FromRef<TestState> for StreamingState {
        fn from_ref(s: &TestState) -> Self {
            StreamingState {
                delta_cache_manager: s.delta_cache_manager.clone(),
                token_cache: s.token_cache.clone(),
            }
        }
    }

    async fn client_api_test_server(
        upstream_token_cache: Arc<TokenCache>,
        upstream_delta_cache_manager: Arc<DeltaCacheManager>,
    ) -> TestServer {
        let app_state = TestState {
            auth_headers: AuthHeaders::default(),
            token_cache: upstream_token_cache,
            delta_cache_manager: Some(upstream_delta_cache_manager),
        };
        let router = Router::new()
            .nest("/api/client", streaming_router_for::<TestState>())
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
            token_type: Some(TokenType::Backend),
            environment: Some("development".into()),
            projects: vec!["*".into()],
            status: TokenValidationStatus::Validated,
        };

        let token_cache = Arc::new(TokenCache::default());
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
            &[DeltaEvent::FeatureUpdated {
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
            &[DeltaEvent::FeatureUpdated {
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

        let test_server = client_api_test_server(token_cache, delta_cache_manager.clone()).await;
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
            token_type: Some(TokenType::Backend),
            environment: Some("development".into()),
            projects: vec!["*".into()],
            status: TokenValidationStatus::Validated,
        };

        let token_cache = Arc::new(TokenCache::default());
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
            &[DeltaEvent::FeatureUpdated {
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

        let test_server = client_api_test_server(token_cache, delta_cache_manager.clone()).await;
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
                &[DeltaEvent::FeatureUpdated {
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
    pub async fn streaming_includes_envelope_id() {
        let token = EdgeToken {
            token: "*:development.hashhasin".into(),
            token_type: Some(TokenType::Backend),
            environment: Some("development".into()),
            projects: vec!["*".into()],
            status: TokenValidationStatus::Validated,
        };

        let token_cache = Arc::new(TokenCache::default());
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
            &[DeltaEvent::FeatureUpdated {
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

        let test_server = client_api_test_server(token_cache, delta_cache_manager.clone()).await;
        let url = test_server.server_url("/").unwrap();

        let mut event_stream = reqwest::Client::new()
            .get(format!("{url}api/client/streaming"))
            .header("Authorization", "*:development.hashhasin")
            .send()
            .await
            .unwrap()
            .bytes_stream()
            .eventsource();

        let stream_updates = timeout(Duration::from_secs(5), async {
            let mut ids: Vec<String> = vec![];
            while let Some(event) = event_stream.next().await {
                match event {
                    Ok(event) => {
                        ids.push(event.id);
                        if ids.len() == 2 {
                            return ids;
                        }
                    }
                    Err(_) => {
                        panic!("Error in event stream");
                    }
                }
            }
            ids
        });

        let inject_event = async move || {
            delta_cache_manager.update_cache(
                &token.environment.clone().unwrap(),
                &[DeltaEvent::FeatureUpdated {
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

        let (ids, _) = tokio::join!(stream_updates, inject_event());
        let ids = ids.expect("Failed to complete");

        assert_eq!(ids, vec!["1".to_string(), "2".to_string()]);
    }

    #[tokio::test]
    async fn streaming_is_terminated_if_token_becomes_invalidated() {
        let token = EdgeToken {
            token: "*:development.hashhasin".into(),
            token_type: Some(TokenType::Backend),
            environment: Some("development".into()),
            projects: vec!["*".into()],
            status: TokenValidationStatus::Validated,
        };

        let token_cache = Arc::new(TokenCache::default());
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
            &[DeltaEvent::FeatureUpdated {
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

        let test_server =
            client_api_test_server(token_cache.clone(), delta_cache_manager.clone()).await;
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
                &[DeltaEvent::FeatureUpdated {
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

    #[tokio::test]
    async fn streaming_does_not_receive_updates_from_unrelated_environments() {
        let dev_token = EdgeToken {
            token: "*:development.hashhasin".into(),
            token_type: Some(TokenType::Backend),
            environment: Some("development".into()),
            projects: vec!["*".into()],
            status: TokenValidationStatus::Validated,
        };

        let prod_token = EdgeToken {
            token: "*:production.hashhasin".into(),
            token_type: Some(TokenType::Backend),
            environment: Some("production".into()),
            projects: vec!["*".into()],
            status: TokenValidationStatus::Validated,
        };

        let token_cache = Arc::new(TokenCache::default());
        let delta_cache_manager = Arc::new(DeltaCacheManager::new());

        delta_cache_manager.insert_cache(
            &dev_token.environment.clone().unwrap(),
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
            &dev_token.environment.clone().unwrap(),
            &[DeltaEvent::FeatureUpdated {
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

        token_cache.insert(dev_token.token.clone(), dev_token.clone());
        token_cache.insert(prod_token.token.clone(), prod_token.clone());

        let test_server = client_api_test_server(token_cache, delta_cache_manager.clone()).await;
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
            delta_cache_manager.insert_cache(
                &prod_token.environment.clone().unwrap(),
                DeltaCache::new(
                    DeltaHydrationEvent {
                        event_id: 1,
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
                &prod_token.environment.clone().unwrap(),
                &[DeltaEvent::FeatureUpdated {
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
        };

        let (_, _) = tokio::join!(stream_updates, inject_event());

        assert!(event_data.len() == 1);
    }
}
