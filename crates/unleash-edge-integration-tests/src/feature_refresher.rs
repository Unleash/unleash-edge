#[cfg(test)]
mod tests {
    use crate::features_from_disk;
    use axum::Router;
    use axum_test::TestServer;
    use chrono::Duration;
    use dashmap::DashMap;
    use pretty_assertions::assert_eq;
    use std::str::FromStr;
    use std::sync::Arc;
    use unleash_edge_appstate::AppState;
    use unleash_edge_feature_cache::{FeatureCache, update_projects_from_feature_update};
    use unleash_edge_feature_filters::{FeatureFilterSet, project_filter};
    use unleash_edge_feature_refresh::{FeatureRefresher, frontend_token_is_covered_by_tokens};
    use unleash_edge_http_client::UnleashClient;
    use unleash_edge_types::TokenValidationStatus::Validated;
    use unleash_edge_types::tokens::{EdgeToken, cache_key};
    use unleash_edge_types::{EngineCache, TokenCache, TokenRefresh, TokenType};
    use unleash_types::client_features::ClientFeature;
    use unleash_yggdrasil::{EngineState, UpdateMessage};

    async fn client_api_test_server(
        upstream_token_cache: Arc<TokenCache>,
        upstream_features_cache: Arc<FeatureCache>,
        upstream_engine_cache: Arc<EngineCache>,
    ) -> TestServer {
        let app_state = AppState::builder()
            .with_token_cache(upstream_token_cache.clone())
            .with_features_cache(upstream_features_cache.clone())
            .with_engine_cache(upstream_engine_cache.clone())
            .build();
        let router = Router::new()
            .nest("/api/client", unleash_edge_client_api::router())
            .nest("/edge", unleash_edge_edge_api::router())
            .with_state(app_state);
        TestServer::builder()
            .http_transport()
            .build(router)
            .expect("Failed to build client api test server")
    }
    #[tokio::test]
    pub async fn getting_403_when_refreshing_features_will_remove_token() {
        let upstream_features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
        let upstream_engine_cache: Arc<EngineCache> = Arc::new(DashMap::default());
        let upstream_token_cache: Arc<TokenCache> = Arc::new(DashMap::default());
        let server = client_api_test_server(
            upstream_token_cache,
            upstream_features_cache,
            upstream_engine_cache,
        )
        .await;
        let unleash_client =
            UnleashClient::from_url(server.server_url("/").unwrap(), None).unwrap();
        let features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
        let engine_cache: Arc<EngineCache> = Arc::new(DashMap::default());
        let feature_refresher = FeatureRefresher {
            unleash_client: Arc::new(unleash_client),
            features_cache,
            engine_cache,
            refresh_interval: Duration::seconds(60),
            ..Default::default()
        };
        let mut token = EdgeToken::try_from("*:development.secret123".to_string()).unwrap();
        token.status = Validated;
        token.token_type = Some(TokenType::Client);
        feature_refresher
            .register_token_for_refresh(token, None)
            .await;
        assert!(!feature_refresher.tokens_to_refresh.is_empty());
        feature_refresher.refresh_features().await;
        assert!(feature_refresher.tokens_to_refresh.is_empty());
        assert!(feature_refresher.features_cache.is_empty());
        assert!(feature_refresher.engine_cache.is_empty());
    }

    #[tokio::test]
    pub async fn getting_404_removes_tokens_from_token_to_refresh_but_not_its_features() {
        let mut token = EdgeToken::try_from("*:development.secret123".to_string()).unwrap();
        token.status = Validated;
        token.token_type = Some(TokenType::Client);
        let token_cache = DashMap::default();
        token_cache.insert(token.token.clone(), token.clone());
        let upstream_features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
        let upstream_engine_cache: Arc<EngineCache> = Arc::new(DashMap::default());
        let upstream_token_cache: Arc<TokenCache> = Arc::new(token_cache);
        let example_features = features_from_disk("../../examples/features.json");
        let cache_key = cache_key(&token);
        let mut engine_state = EngineState::default();
        let warnings =
            engine_state.take_state(UpdateMessage::FullResponse(example_features.clone()));
        upstream_features_cache.insert(cache_key.clone(), example_features.clone());
        upstream_engine_cache.insert(cache_key.clone(), engine_state);
        let server = client_api_test_server(
            upstream_token_cache,
            upstream_features_cache,
            upstream_engine_cache,
        )
        .await;
        let unleash_client =
            UnleashClient::from_url(server.server_url("/").unwrap(), None).unwrap();
        let features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
        let engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let feature_refresher = FeatureRefresher {
            unleash_client: Arc::new(unleash_client),
            features_cache,
            engine_cache,
            refresh_interval: Duration::milliseconds(1),
            ..Default::default()
        };
        feature_refresher
            .register_token_for_refresh(token, None)
            .await;
        assert!(!feature_refresher.tokens_to_refresh.is_empty());
        feature_refresher.refresh_features().await;
        assert!(!feature_refresher.tokens_to_refresh.is_empty());
        assert!(!feature_refresher.features_cache.is_empty());
        assert!(!feature_refresher.engine_cache.is_empty());
        tokio::time::sleep(std::time::Duration::from_millis(5)).await; // To ensure our refresh is due
        feature_refresher.refresh_features().await;
        assert_eq!(
            feature_refresher
                .tokens_to_refresh
                .get("*:development.secret123")
                .unwrap()
                .failure_count,
            1
        );
        assert!(!feature_refresher.features_cache.is_empty());
        assert!(!feature_refresher.engine_cache.is_empty());
        assert!(warnings.is_none());
    }

    #[tokio::test]
    pub async fn when_we_have_a_cache_and_token_gets_removed_caches_are_emptied() {
        let upstream_features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
        let upstream_engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let upstream_token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let token_cache_to_modify = upstream_token_cache.clone();
        let mut valid_token = EdgeToken::try_from("*:development.secret123".to_string()).unwrap();
        valid_token.token_type = Some(TokenType::Client);
        valid_token.status = Validated;
        upstream_token_cache.insert(valid_token.token.clone(), valid_token.clone());
        let example_features = features_from_disk("../../examples/features.json");
        let cache_key = cache_key(&valid_token);
        let mut engine_state = EngineState::default();
        let warnings =
            engine_state.take_state(UpdateMessage::FullResponse(example_features.clone()));
        upstream_features_cache.insert(cache_key.clone(), example_features.clone());
        upstream_engine_cache.insert(cache_key.clone(), engine_state);
        let server = client_api_test_server(
            upstream_token_cache,
            upstream_features_cache,
            upstream_engine_cache,
        )
        .await;
        let unleash_client =
            UnleashClient::from_url(server.server_url("/").unwrap(), None).unwrap();
        let mut feature_refresher = FeatureRefresher::with_client(Arc::new(unleash_client));
        feature_refresher.refresh_interval = Duration::seconds(0);
        feature_refresher
            .register_token_for_refresh(valid_token.clone(), None)
            .await;
        assert!(!feature_refresher.tokens_to_refresh.is_empty());
        feature_refresher.refresh_features().await;
        assert!(!feature_refresher.features_cache.is_empty());
        assert!(!feature_refresher.engine_cache.is_empty());
        token_cache_to_modify.remove(&valid_token.token);
        feature_refresher.refresh_features().await;
        assert!(feature_refresher.tokens_to_refresh.is_empty());
        assert!(feature_refresher.features_cache.is_empty());
        assert!(feature_refresher.engine_cache.is_empty());
        assert!(warnings.is_none());
    }

    #[tokio::test]
    pub async fn removing_one_of_multiple_keys_from_same_environment_does_not_remove_feature_and_engine_caches()
     {
        let upstream_features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
        let upstream_engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let upstream_token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let token_cache_to_modify = upstream_token_cache.clone();
        let mut dx_token = EdgeToken::try_from("dx:development.secret123".to_string()).unwrap();
        dx_token.token_type = Some(TokenType::Client);
        dx_token.status = Validated;
        upstream_token_cache.insert(dx_token.token.clone(), dx_token.clone());
        let mut eg_token = EdgeToken::try_from("eg:development.secret123".to_string()).unwrap();
        eg_token.token_type = Some(TokenType::Client);
        eg_token.status = Validated;
        upstream_token_cache.insert(eg_token.token.clone(), eg_token.clone());
        let example_features = features_from_disk("../../examples/hostedexample.json");
        let cache_key = cache_key(&dx_token);
        let mut engine_state = EngineState::default();
        let warnings =
            engine_state.take_state(UpdateMessage::FullResponse(example_features.clone()));
        upstream_features_cache.insert(cache_key.clone(), example_features.clone());
        upstream_engine_cache.insert(cache_key.clone(), engine_state);
        let server = client_api_test_server(
            upstream_token_cache,
            upstream_features_cache,
            upstream_engine_cache,
        )
        .await;
        let unleash_client =
            UnleashClient::from_url(server.server_url("/").unwrap(), None).unwrap();
        let mut feature_refresher = FeatureRefresher::with_client(Arc::new(unleash_client));
        feature_refresher.refresh_interval = Duration::seconds(0);
        feature_refresher
            .register_token_for_refresh(dx_token.clone(), None)
            .await;
        feature_refresher
            .register_token_for_refresh(eg_token.clone(), None)
            .await;
        assert_eq!(feature_refresher.tokens_to_refresh.len(), 2);
        assert_eq!(feature_refresher.features_cache.len(), 0);
        assert_eq!(feature_refresher.engine_cache.len(), 0);
        feature_refresher.refresh_features().await;
        assert_eq!(feature_refresher.features_cache.len(), 1);
        assert_eq!(feature_refresher.engine_cache.len(), 1);
        token_cache_to_modify.remove(&dx_token.token);
        feature_refresher.refresh_features().await;
        assert_eq!(feature_refresher.tokens_to_refresh.len(), 1);
        assert_eq!(feature_refresher.features_cache.len(), 1);
        assert_eq!(feature_refresher.engine_cache.len(), 1);
        assert!(warnings.is_none());
    }

    #[test]
    fn front_end_token_is_properly_covered_by_current_tokens() {
        let fe_token = EdgeToken {
            projects: vec!["a".into(), "b".into()],
            environment: Some("development".into()),
            ..Default::default()
        };

        let wildcard_token = EdgeToken {
            projects: vec!["*".into()],
            environment: Some("development".into()),
            ..Default::default()
        };

        let current_tokens = DashMap::new();
        let token_refresh = TokenRefresh {
            token: wildcard_token.clone(),
            etag: None,
            next_refresh: None,
            last_refreshed: None,
            last_check: None,
            failure_count: 0,
            last_feature_count: None,
        };

        current_tokens.insert(wildcard_token.token, token_refresh);

        let current_tokens_arc = Arc::new(current_tokens);
        assert!(frontend_token_is_covered_by_tokens(
            &fe_token,
            current_tokens_arc
        ));
    }

    #[tokio::test]
    async fn refetching_data_when_feature_is_archived_should_remove_archived_feature() {
        let upstream_features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
        let upstream_engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let upstream_token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let mut eg_token = EdgeToken::from_str("eg:development.devsecret").unwrap();
        eg_token.token_type = Some(TokenType::Client);
        eg_token.status = Validated;
        upstream_token_cache.insert(eg_token.token.clone(), eg_token.clone());
        let example_features = features_from_disk("../../examples/hostedexample.json");
        let cache_key = cache_key(&eg_token);
        upstream_features_cache.insert(cache_key.clone(), example_features.clone());
        let mut engine_state = EngineState::default();
        let warnings =
            engine_state.take_state(UpdateMessage::FullResponse(example_features.clone()));
        upstream_engine_cache.insert(cache_key.clone(), engine_state);
        let server = client_api_test_server(
            upstream_token_cache,
            upstream_features_cache.clone(),
            upstream_engine_cache,
        )
        .await;
        let features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
        let unleash_client =
            UnleashClient::from_url(server.server_url("/").unwrap(), None).unwrap();
        let feature_refresher = FeatureRefresher {
            unleash_client: Arc::new(unleash_client),
            features_cache: features_cache.clone(),
            refresh_interval: Duration::seconds(0),
            ..Default::default()
        };

        let _ = feature_refresher
            .register_and_hydrate_token(&eg_token)
            .await;

        // Now, let's say that all features are archived in upstream
        let empty_features = features_from_disk("../../examples/empty-features.json");
        upstream_features_cache.insert(cache_key.clone(), empty_features);

        feature_refresher.refresh_features().await;
        // Since our response was empty, our theory is that there should be no features here now.
        assert!(
            !features_cache
                .get(&cache_key)
                .unwrap()
                .features
                .iter()
                .any(|f| f.project == Some("eg".into()))
        );
        assert!(warnings.is_none());
    }

    #[test]
    pub fn an_update_with_one_feature_removed_from_one_project_removes_the_feature_from_the_feature_list()
     {
        let features = features_from_disk("../../examples/hostedexample.json").features;
        let mut dx_data: Vec<ClientFeature> =
            features_from_disk("../../examples/hostedexample.json")
                .features
                .iter()
                .filter(|f| f.project == Some("dx".into()))
                .cloned()
                .collect();
        dx_data.remove(0);
        let mut token = EdgeToken::from_str("[]:development.somesecret").unwrap();
        token.status = Validated;
        token.projects = vec![String::from("dx")];

        let updated = update_projects_from_feature_update(&token, &features, &dx_data);
        assert_ne!(
            features
                .iter()
                .filter(|p| p.project == Some("dx".into()))
                .count(),
            updated
                .iter()
                .filter(|p| p.project == Some("dx".into()))
                .count()
        );
        assert_eq!(
            features
                .iter()
                .filter(|p| p.project == Some("eg".into()))
                .count(),
            updated
                .iter()
                .filter(|p| p.project == Some("eg".into()))
                .count()
        );
    }

    #[test]
    pub fn project_state_from_update_should_overwrite_project_state_in_known_state() {
        let features = features_from_disk("../../examples/hostedexample.json").features;
        let mut dx_data: Vec<ClientFeature> = features
            .iter()
            .filter(|f| f.project == Some("dx".into()))
            .cloned()
            .collect();
        dx_data.remove(0);
        let mut eg_data: Vec<ClientFeature> = features
            .iter()
            .filter(|f| f.project == Some("eg".into()))
            .cloned()
            .collect();
        eg_data.remove(0);
        dx_data.extend(eg_data);
        let edge_token = EdgeToken {
            token: "".to_string(),
            token_type: Some(TokenType::Client),
            environment: None,
            projects: vec![String::from("dx"), String::from("eg")],
            status: Validated,
        };
        let update = update_projects_from_feature_update(&edge_token, &features, &dx_data);
        assert_eq!(features.len() - update.len(), 2); // We've removed two elements
    }

    #[test]
    pub fn if_project_is_removed_but_token_has_access_to_project_update_should_remove_cached_project()
     {
        let features = features_from_disk("../../examples/hostedexample.json").features;
        let edge_token = EdgeToken {
            token: "".to_string(),
            token_type: Some(TokenType::Client),
            environment: None,
            projects: vec![String::from("dx"), String::from("eg")],
            status: Validated,
        };
        let eg_data: Vec<ClientFeature> = features
            .iter()
            .filter(|f| f.project == Some("eg".into()))
            .cloned()
            .collect();
        let update = update_projects_from_feature_update(&edge_token, &features, &eg_data);
        assert!(!update.iter().any(|p| p.project == Some(String::from("dx"))));
    }
    #[test]
    pub fn if_token_does_not_have_access_to_project_no_update_happens_to_project() {
        let features = features_from_disk("../../examples/hostedexample.json").features;
        let edge_token = EdgeToken {
            token: "".to_string(),
            token_type: Some(TokenType::Client),
            environment: None,
            projects: vec![String::from("dx"), String::from("eg")],
            status: Validated,
        };
        let eg_data: Vec<ClientFeature> = features
            .iter()
            .filter(|f| f.project == Some("eg".into()))
            .cloned()
            .collect();
        let update = update_projects_from_feature_update(&edge_token, &features, &eg_data);
        assert_eq!(
            update
                .iter()
                .filter(|p| p.project == Some(String::from("unleash-cloud")))
                .count(),
            1
        );
    }

    #[test]
    pub fn if_token_is_wildcard_our_entire_cache_is_replaced_by_update() {
        let features = vec![
            ClientFeature {
                name: "my.first.toggle.in.default".to_string(),
                feature_type: Some("release".into()),
                description: None,
                created_at: None,
                last_seen_at: None,
                enabled: true,
                stale: None,
                impression_data: None,
                project: Some("default".into()),
                strategies: None,
                variants: None,
                dependencies: None,
            },
            ClientFeature {
                name: "my.second.toggle.in.testproject".to_string(),
                feature_type: Some("release".into()),
                description: None,
                created_at: None,
                last_seen_at: None,
                enabled: false,
                stale: None,
                impression_data: None,
                project: Some("testproject".into()),
                strategies: None,
                variants: None,
                dependencies: None,
            },
        ];
        let edge_token = EdgeToken {
            token: "".to_string(),
            token_type: Some(TokenType::Client),
            environment: None,
            projects: vec![String::from("*")],
            status: Validated,
        };
        let update: Vec<ClientFeature> = features
            .clone()
            .iter()
            .filter(|t| t.project == Some("default".into()))
            .cloned()
            .collect();
        let updated = update_projects_from_feature_update(&edge_token, &features, &update);
        assert_eq!(updated.len(), 1);
        assert!(updated.iter().all(|f| f.project == Some("default".into())))
    }

    #[test]
    pub fn token_with_access_to_different_project_than_exists_in_cache_should_never_delete_features_from_other_projects()
     {
        // Added after customer issue in May '24 when tokens unrelated to projects in cache with no actual features connected to them removed existing features in cache for unrelated projects
        let features = vec![
            ClientFeature {
                name: "my.first.toggle.in.default".to_string(),
                feature_type: Some("release".into()),
                description: None,
                created_at: None,
                last_seen_at: None,
                enabled: true,
                stale: None,
                impression_data: None,
                project: Some("testproject".into()),
                strategies: None,
                variants: None,
                dependencies: None,
            },
            ClientFeature {
                name: "my.second.toggle.in.testproject".to_string(),
                feature_type: Some("release".into()),
                description: None,
                created_at: None,
                last_seen_at: None,
                enabled: false,
                stale: None,
                impression_data: None,
                project: Some("testproject".into()),
                strategies: None,
                variants: None,
                dependencies: None,
            },
        ];
        let empty_features = vec![];
        let unrelated_token_to_existing_features = EdgeToken {
            token: "someotherproject:dev.myextralongsecretstringwithfeatures".to_string(),
            token_type: Some(TokenType::Client),
            environment: Some("dev".into()),
            projects: vec![String::from("someother")],
            status: Validated,
        };
        let updated = update_projects_from_feature_update(
            &unrelated_token_to_existing_features,
            &features,
            &empty_features,
        );
        assert_eq!(updated.len(), 2);
    }
    #[test]
    pub fn token_with_access_to_both_a_different_project_than_exists_in_cache_and_the_cached_project_should_delete_features_from_both_projects()
     {
        // Added after customer issue in May '24 when tokens unrelated to projects in cache with no actual features connected to them removed existing features in cache for unrelated projects
        let features = vec![
            ClientFeature {
                name: "my.first.toggle.in.default".to_string(),
                feature_type: Some("release".into()),
                description: None,
                created_at: None,
                last_seen_at: None,
                enabled: true,
                stale: None,
                impression_data: None,
                project: Some("testproject".into()),
                strategies: None,
                variants: None,
                dependencies: None,
            },
            ClientFeature {
                name: "my.second.toggle.in.testproject".to_string(),
                feature_type: Some("release".into()),
                description: None,
                created_at: None,
                last_seen_at: None,
                enabled: false,
                stale: None,
                impression_data: None,
                project: Some("testproject".into()),
                strategies: None,
                variants: None,
                dependencies: None,
            },
        ];
        let empty_features = vec![];
        let token_with_access_to_both_empty_and_full_project = EdgeToken {
            token: "[]:dev.myextralongsecretstringwithfeatures".to_string(),
            token_type: Some(TokenType::Client),
            environment: Some("dev".into()),
            projects: vec![String::from("testproject"), String::from("someother")],
            status: Validated,
        };
        let updated = update_projects_from_feature_update(
            &token_with_access_to_both_empty_and_full_project,
            &features,
            &empty_features,
        );
        assert_eq!(updated.len(), 0);
    }
}
