#[cfg(test)]
mod tests {
    use axum::{Router, extract::FromRef};
    use axum_test::TestServer;
    use eventsource_stream::Eventsource;
    use std::{collections::BTreeMap, sync::Arc, time::Duration};
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
        ClientFeature, ClientFeaturesDelta, DeltaEvent, Segment, Strategy,
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
        TestServer::builder().http_transport().build(router)
    }

    #[derive(Clone, Debug, Default, PartialEq)]
    struct EffectiveState {
        features: BTreeMap<(String, String), ClientFeature>,
        segments: BTreeMap<i32, Segment>,
    }

    impl EffectiveState {
        fn from_features_and_segments(
            features: Vec<ClientFeature>,
            segments: Vec<Segment>,
        ) -> Self {
            let mut state = Self::default();
            state.replace(features, segments);
            state
        }

        fn apply_delta(&mut self, delta: &ClientFeaturesDelta) {
            for event in &delta.events {
                match event {
                    DeltaEvent::Hydration {
                        features, segments, ..
                    } => self.replace(features.clone(), segments.clone()),
                    DeltaEvent::FeatureUpdated { feature, .. } => {
                        self.features.insert(feature_key(feature), feature.clone());
                    }
                    DeltaEvent::FeatureRemoved {
                        feature_name,
                        project,
                        ..
                    } => {
                        self.features
                            .remove(&(project.clone(), feature_name.clone()));
                    }
                    DeltaEvent::SegmentUpdated { segment, .. } => {
                        self.segments.insert(segment.id, segment.clone());
                    }
                    DeltaEvent::SegmentRemoved { segment_id, .. } => {
                        self.segments.remove(segment_id);
                    }
                }
            }
        }

        fn replace(&mut self, features: Vec<ClientFeature>, segments: Vec<Segment>) {
            self.features = features
                .into_iter()
                .map(|feature| (feature_key(&feature), feature))
                .collect();
            self.segments = segments
                .into_iter()
                .map(|segment| (segment.id, segment))
                .collect();
        }
    }

    fn feature_key(feature: &ClientFeature) -> (String, String) {
        (
            feature.project.clone().unwrap_or_default(),
            feature.name.clone(),
        )
    }

    fn feature(name: &str, project: &str) -> ClientFeature {
        ClientFeature {
            name: name.into(),
            project: Some(project.into()),
            strategies: Some(vec![]),
            ..ClientFeature::default()
        }
    }

    fn apply_delta_to_state(
        mut state: EffectiveState,
        delta: &ClientFeaturesDelta,
    ) -> EffectiveState {
        state.apply_delta(delta);
        state
    }

    fn backend_token(projects: &[&str], environment: &str) -> EdgeToken {
        EdgeToken {
            token: format!("{}:{environment}.hashhasin", projects.join(",")),
            token_type: Some(TokenType::Backend),
            environment: Some(environment.into()),
            projects: projects.iter().map(|project| project.to_string()).collect(),
            status: TokenValidationStatus::Validated,
        }
    }

    fn delta_cache_with_state(event_id: u32, features: Vec<ClientFeature>) -> DeltaCache {
        DeltaCache::new(
            DeltaHydrationEvent {
                event_id,
                features,
                segments: vec![],
            },
            10,
        )
    }

    async fn first_stream_delta(
        test_server: &TestServer,
        token: &str,
        last_event_id: Option<u32>,
    ) -> (String, ClientFeaturesDelta) {
        let url = test_server.server_url("/").unwrap();
        let mut request = reqwest::Client::new()
            .get(format!("{url}api/client/streaming"))
            .header("Authorization", token);

        if let Some(last_event_id) = last_event_id {
            request = request.header("Last-Event-ID", last_event_id.to_string());
        }

        let mut event_stream = request.send().await.unwrap().bytes_stream().eventsource();

        let first_event = timeout(Duration::from_secs(5), event_stream.next())
            .await
            .expect("Failed to complete")
            .expect("stream ended unexpectedly")
            .expect("Error in event stream");

        (
            first_event.id,
            serde_json::from_str::<ClientFeaturesDelta>(&first_event.data).unwrap(),
        )
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
    pub async fn streaming_resumes_from_last_event_id_when_present() {
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
                    strategies: Some(vec![]),
                    ..ClientFeature::default()
                },
            }],
        );

        delta_cache_manager.update_cache(
            &token.environment.clone().unwrap(),
            &[DeltaEvent::FeatureUpdated {
                event_id: 2,
                feature: ClientFeature {
                    name: "Westley".into(),
                    project: Some("Princess bride".into()),
                    strategies: Some(vec![]),
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
            .header("Last-Event-ID", "1")
            .send()
            .await
            .unwrap()
            .bytes_stream()
            .eventsource();

        let first_event = timeout(Duration::from_secs(5), event_stream.next())
            .await
            .expect("Failed to complete")
            .expect("stream ended unexpectedly")
            .expect("Error in event stream");

        let delta = serde_json::from_str::<ClientFeaturesDelta>(&first_event.data).unwrap();
        assert!(!delta.events.is_empty());
        assert!(!matches!(delta.events[0], DeltaEvent::Hydration { .. }));
        assert!(delta.events.iter().all(|e| e.get_event_id() > 1));
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

    #[tokio::test]
    async fn streaming_staggered_nodes_return_same_effective_state_on_initial_connect() {
        let token = backend_token(&["*"], "development");
        let token_cache_a = Arc::new(TokenCache::default());
        let token_cache_b = Arc::new(TokenCache::default());
        let delta_cache_manager_a = Arc::new(DeltaCacheManager::new());
        let delta_cache_manager_b = Arc::new(DeltaCacheManager::new());

        token_cache_a.insert(token.token.clone(), token.clone());
        token_cache_b.insert(token.token.clone(), token.clone());

        delta_cache_manager_a.insert_cache(
            "development",
            delta_cache_with_state(0, vec![feature("Inigo Montoya", "project-a")]),
        );
        delta_cache_manager_a.update_cache(
            "development",
            &[DeltaEvent::FeatureUpdated {
                event_id: 1,
                feature: feature("Westley", "project-a"),
            }],
        );

        delta_cache_manager_b.insert_cache(
            "development",
            delta_cache_with_state(
                1,
                vec![
                    feature("Inigo Montoya", "project-a"),
                    feature("Westley", "project-a"),
                ],
            ),
        );

        let server_a = client_api_test_server(token_cache_a, delta_cache_manager_a).await;
        let server_b = client_api_test_server(token_cache_b, delta_cache_manager_b).await;

        let (id_a, delta_a) = first_stream_delta(&server_a, &token.token, None).await;
        let (id_b, delta_b) = first_stream_delta(&server_b, &token.token, None).await;

        assert_eq!(id_a, "1");
        assert_eq!(id_b, "1");
        assert!(matches!(delta_a.events[0], DeltaEvent::Hydration { .. }));
        assert!(matches!(delta_b.events[0], DeltaEvent::Hydration { .. }));
        assert_eq!(
            apply_delta_to_state(
                EffectiveState::from_features_and_segments(vec![], vec![]),
                &delta_a
            ),
            apply_delta_to_state(
                EffectiveState::from_features_and_segments(vec![], vec![]),
                &delta_b
            )
        );
    }

    #[tokio::test]
    async fn streaming_cross_node_reconnect_falls_back_to_hydration_without_losing_state() {
        let token = backend_token(&["*"], "development");
        let token_cache_a = Arc::new(TokenCache::default());
        let token_cache_b = Arc::new(TokenCache::default());
        let delta_cache_manager_a = Arc::new(DeltaCacheManager::new());
        let delta_cache_manager_b = Arc::new(DeltaCacheManager::new());

        token_cache_a.insert(token.token.clone(), token.clone());
        token_cache_b.insert(token.token.clone(), token.clone());

        delta_cache_manager_a.insert_cache(
            "development",
            delta_cache_with_state(0, vec![feature("Inigo Montoya", "project-a")]),
        );
        delta_cache_manager_a.update_cache(
            "development",
            &[DeltaEvent::FeatureUpdated {
                event_id: 1,
                feature: feature("Inigo Montoya", "project-a"),
            }],
        );
        delta_cache_manager_a.update_cache(
            "development",
            &[DeltaEvent::FeatureUpdated {
                event_id: 2,
                feature: feature("Westley", "project-a"),
            }],
        );

        delta_cache_manager_b.insert_cache(
            "development",
            delta_cache_with_state(
                2,
                vec![
                    feature("Inigo Montoya", "project-a"),
                    feature("Westley", "project-a"),
                ],
            ),
        );

        let server_a = client_api_test_server(token_cache_a, delta_cache_manager_a).await;
        let server_b = client_api_test_server(token_cache_b, delta_cache_manager_b).await;

        let (_, replay_delta) = first_stream_delta(&server_a, &token.token, Some(1)).await;
        let (_, hydration_delta) = first_stream_delta(&server_b, &token.token, Some(1)).await;

        assert!(!matches!(
            replay_delta.events[0],
            DeltaEvent::Hydration { .. }
        ));
        assert!(matches!(
            hydration_delta.events[0],
            DeltaEvent::Hydration { .. }
        ));

        let prior_state = EffectiveState::from_features_and_segments(
            vec![feature("Inigo Montoya", "project-a")],
            vec![],
        );
        let expected_state = EffectiveState::from_features_and_segments(
            vec![
                feature("Inigo Montoya", "project-a"),
                feature("Westley", "project-a"),
            ],
            vec![],
        );

        let replay_state = apply_delta_to_state(prior_state.clone(), &replay_delta);
        let hydration_state = apply_delta_to_state(prior_state, &hydration_delta);

        assert_eq!(replay_state, expected_state);
        assert_eq!(hydration_state, expected_state);
    }

    #[tokio::test]
    async fn streaming_cross_node_reconnect_preserves_project_scoped_effective_state() {
        let token = backend_token(&["project-a"], "development");
        let token_cache_a = Arc::new(TokenCache::default());
        let token_cache_b = Arc::new(TokenCache::default());
        let delta_cache_manager_a = Arc::new(DeltaCacheManager::new());
        let delta_cache_manager_b = Arc::new(DeltaCacheManager::new());

        token_cache_a.insert(token.token.clone(), token.clone());
        token_cache_b.insert(token.token.clone(), token.clone());

        delta_cache_manager_a.insert_cache(
            "development",
            delta_cache_with_state(0, vec![feature("Alpha", "project-a")]),
        );
        delta_cache_manager_a.update_cache(
            "development",
            &[DeltaEvent::FeatureUpdated {
                event_id: 1,
                feature: feature("Alpha", "project-a"),
            }],
        );
        delta_cache_manager_a.update_cache(
            "development",
            &[DeltaEvent::FeatureUpdated {
                event_id: 2,
                feature: feature("Bravo", "project-b"),
            }],
        );
        delta_cache_manager_a.update_cache(
            "development",
            &[DeltaEvent::FeatureUpdated {
                event_id: 3,
                feature: feature("Charlie", "project-a"),
            }],
        );

        delta_cache_manager_b.insert_cache(
            "development",
            delta_cache_with_state(
                3,
                vec![
                    feature("Alpha", "project-a"),
                    feature("Bravo", "project-b"),
                    feature("Charlie", "project-a"),
                ],
            ),
        );

        let server_a = client_api_test_server(token_cache_a, delta_cache_manager_a).await;
        let server_b = client_api_test_server(token_cache_b, delta_cache_manager_b).await;

        let (_, replay_delta) = first_stream_delta(&server_a, &token.token, Some(1)).await;
        let (_, hydration_delta) = first_stream_delta(&server_b, &token.token, Some(1)).await;

        let prior_state =
            EffectiveState::from_features_and_segments(vec![feature("Alpha", "project-a")], vec![]);
        let expected_state = EffectiveState::from_features_and_segments(
            vec![
                feature("Alpha", "project-a"),
                feature("Charlie", "project-a"),
            ],
            vec![],
        );

        let replay_state = apply_delta_to_state(prior_state.clone(), &replay_delta);
        let hydration_state = apply_delta_to_state(prior_state, &hydration_delta);

        assert_eq!(replay_state, expected_state);
        assert_eq!(hydration_state, expected_state);
        assert!(
            replay_state
                .features
                .keys()
                .all(|(project, _)| project == "project-a")
        );
        assert!(
            hydration_state
                .features
                .keys()
                .all(|(project, _)| project == "project-a")
        );
    }

    #[tokio::test]
    async fn streaming_stale_node_is_semantically_different_from_fresh_node() {
        let token = backend_token(&["*"], "development");
        let token_cache_a = Arc::new(TokenCache::default());
        let token_cache_b = Arc::new(TokenCache::default());
        let delta_cache_manager_a = Arc::new(DeltaCacheManager::new());
        let delta_cache_manager_b = Arc::new(DeltaCacheManager::new());

        token_cache_a.insert(token.token.clone(), token.clone());
        token_cache_b.insert(token.token.clone(), token.clone());

        delta_cache_manager_a.insert_cache(
            "development",
            delta_cache_with_state(0, vec![feature("Inigo Montoya", "project-a")]),
        );
        delta_cache_manager_a.update_cache(
            "development",
            &[DeltaEvent::FeatureUpdated {
                event_id: 1,
                feature: feature("Westley", "project-a"),
            }],
        );

        delta_cache_manager_b.insert_cache(
            "development",
            delta_cache_with_state(0, vec![feature("Inigo Montoya", "project-a")]),
        );

        let server_a = client_api_test_server(token_cache_a, delta_cache_manager_a).await;
        let server_b = client_api_test_server(token_cache_b, delta_cache_manager_b).await;

        let (_, fresh_delta) = first_stream_delta(&server_a, &token.token, None).await;
        let (_, stale_delta) = first_stream_delta(&server_b, &token.token, None).await;

        let fresh_state = apply_delta_to_state(
            EffectiveState::from_features_and_segments(vec![], vec![]),
            &fresh_delta,
        );
        let stale_state = apply_delta_to_state(
            EffectiveState::from_features_and_segments(vec![], vec![]),
            &stale_delta,
        );

        assert_ne!(fresh_state, stale_state);
    }
}
