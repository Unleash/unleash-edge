#[cfg(test)]
mod tests {
    use axum::Router;
    use axum_test::TestServer;
    use eventsource_stream::Eventsource;
    use std::sync::Arc;
    use tokio_stream::StreamExt as _;
    use unleash_edge_appstate::AppState;
    use unleash_edge_feature_cache::FeatureCache;

    use unleash_edge_types::{EngineCache, TokenCache};

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
            .with_state(app_state);
        TestServer::builder()
            .http_transport()
            .build(router)
            .expect("Failed to build client api test server")
    }

    #[tokio::test]
    pub async fn test_streaming_refresher() {
        let upstream_token_cache = Arc::new(TokenCache::default());
        let upstream_features_cache = Arc::new(FeatureCache::default());
        let upstream_engine_cache = Arc::new(EngineCache::default());

        let test_server = client_api_test_server(
            upstream_token_cache,
            upstream_features_cache,
            upstream_engine_cache,
        )
        .await;
        let url = test_server.server_url("/").unwrap();

        let mut event_stream = reqwest::Client::new()
            .get(format!("{url}/api/client/streaming"))
            .header("User-Agent", "integration_test")
            .send()
            .await
            .unwrap()
            .bytes_stream()
            .eventsource()
            .take(1);

        let mut event_data: Vec<String> = vec![];
        while let Some(event) = event_stream.next().await {
            match event {
                Ok(event) => {
                    if event.data == "[DONE]" {
                        break;
                    }

                    event_data.push(event.data);
                }
                Err(_) => {
                    panic!("Error in event stream");
                }
            }
        }

        assert!(event_data[0] == "hi!");
    }
}
