#[cfg(test)]
mod streaming_tests {
    use actix_web::App;
    use chrono::Duration;
    use dashmap::DashMap;
    use eventsource_client::Client;
    use futures::TryStreamExt;
    use reqwest::Client;
    use std::{
        process::{Command, Stdio},
        str::FromStr,
        sync::Arc,
    };
    use unleash_edge::{
        http::{
            broadcaster::Broadcaster, feature_refresher::FeatureRefresher,
            unleash_client::UnleashClient,
        },
        tests::{edge_server, upstream_server},
        types::{BuildInfo, EdgeToken, TokenType, TokenValidationStatus},
    };
    use unleash_types::client_features::ClientFeatures;

    #[actix_web::test]
    async fn test_streaming() {
        let unleash_broadcaster = Broadcaster::new(Arc::new(DashMap::default()));
        let unleash_features_cache: Arc<DashMap<String, ClientFeatures>> =
            Arc::new(DashMap::default());
        let unleash_token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());

        let unleash_server = upstream_server(
            unleash_token_cache.clone(),
            unleash_features_cache.clone(),
            Arc::new(DashMap::default()),
            unleash_broadcaster.clone(),
        )
        .await;

        let edge = edge_server(&unleash_server.url("/")).await;

        let mut upstream_known_token = EdgeToken::from_str("dx:development.secret123").unwrap();
        upstream_known_token.status = TokenValidationStatus::Validated;
        upstream_known_token.token_type = Some(TokenType::Client);
        unleash_token_cache.insert(
            upstream_known_token.token.clone(),
            upstream_known_token.clone(),
        );

        let es_client =
            eventsource_client::ClientBuilder::for_url(&edge.url("/api/client/streaming"))
                .unwrap()
                .header("Authorization", &upstream_known_token.token)
                .unwrap()
                .build();

        let events = vec![];
        tokio::spawn(async move {
            let mut stream = es_client
                .stream()
                .map_ok(move |sse| async move { events.push(sse) });
        });

        print!("{events:?}")
    }
}
