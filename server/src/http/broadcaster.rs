use std::{
    hash::{Hash, Hasher},
    sync::Arc,
    time::Duration,
};

use actix_web::{rt::time::interval, web::Json};
use actix_web_lab::{
    sse::{self, Event, Sse},
    util::InfallibleStream,
};
use dashmap::DashMap;
use futures::future;
use prometheus::{register_int_gauge, IntGauge};
use serde::Serialize;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, warn};
use unleash_types::client_features::{ClientFeatures, Query};

use crate::{
    error::EdgeError,
    feature_cache::FeatureCache,
    filters::{filter_client_features, name_prefix_filter, project_filter, FeatureFilterSet},
    tokens::cache_key,
    types::{EdgeJsonResult, EdgeResult, EdgeToken},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct QueryWrapper {
    query: Query,
}

impl Hash for QueryWrapper {
    fn hash<H: Hasher>(&self, state: &mut H) {
        serde_json::to_string(&self.query).unwrap().hash(state);
    }
}

#[derive(Clone, Debug)]
struct ClientGroup {
    clients: Vec<mpsc::Sender<sse::Event>>,
    token: EdgeToken,
    // last_hash: u64
}

pub struct Broadcaster {
    active_connections: DashMap<QueryWrapper, ClientGroup>,
    features_cache: Arc<FeatureCache>,
}

lazy_static::lazy_static! {
    pub static ref CONNECTED_STREAMING_CLIENTS: IntGauge = register_int_gauge!(
        "connected_streaming_clients",
        "Number of connected streaming clients",
    )
    .unwrap();
}

impl Broadcaster {
    /// Constructs new broadcaster and spawns ping loop.
    pub fn new(features: Arc<FeatureCache>) -> Arc<Self> {
        let broadcaster = Arc::new(Broadcaster {
            active_connections: DashMap::new(),
            features_cache: features.clone(),
        });

        Broadcaster::spawn_heartbeat(broadcaster.clone());
        Broadcaster::spawn_feature_cache_subscriber(broadcaster.clone());

        broadcaster
    }

    /// Pings clients every 30 seconds to see if they are alive and remove them from the broadcast
    /// list if not.
    fn spawn_heartbeat(this: Arc<Self>) {
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(30));

            loop {
                interval.tick().await;
                this.heartbeat().await;
            }
        });
    }

    fn spawn_feature_cache_subscriber(this: Arc<Self>) {
        let mut rx = this.features_cache.subscribe();
        tokio::spawn(async move {
            while let Ok(key) = rx.recv().await {
                debug!("Received update for key: {:?}", key);
                // we can hand this off to an external system here to make it testable
                // e.g.
                // let env_groups = get_connections_for_env(&key);
                // for env_group => get flags, compare hash; if updated, send updates
                this.broadcast().await;
            }
        });
    }

    /// Removes all non-responsive clients from broadcast list.
    async fn heartbeat(&self) {
        let mut active_connections = 0i64;
        for mut group in self.active_connections.iter_mut() {
            let mut ok_clients = Vec::new();

            for client in &group.clients {
                if client
                    .send(sse::Event::Comment("keep-alive".into()))
                    .await
                    .is_ok()
                {
                    ok_clients.push(client.clone());
                }
            }

            active_connections += ok_clients.len() as i64;
            group.clients = ok_clients;
        }
        CONNECTED_STREAMING_CLIENTS.set(active_connections)
    }

    pub async fn connect(
        &self,
        token: EdgeToken,
        query: Query,
    ) -> EdgeResult<Sse<InfallibleStream<ReceiverStream<sse::Event>>>> {
        let rx = self.create_connection(token, query).await?;
        Ok(Sse::from_infallible_receiver(rx))
    }

    async fn create_connection(
        &self,
        token: EdgeToken,
        query: Query,
    ) -> EdgeResult<mpsc::Receiver<sse::Event>> {
        let (tx, rx) = mpsc::channel(10);

        let features = self.resolve_features(&token, query.clone()).await?;
        tx.send(
            sse::Data::new_json(&features)?
                .event("unleash-connected")
                .into(),
        )
        .await?;

        self.active_connections
            .entry(QueryWrapper { query })
            .and_modify(|group| {
                group.clients.push(tx.clone());
            })
            .or_insert(ClientGroup {
                clients: vec![tx.clone()],
                token,
            });

        Ok(rx)
    }

    fn get_query_filters(query: &Query, token: &EdgeToken) -> FeatureFilterSet {
        let filter_set = if let Some(name_prefix) = &query.name_prefix {
            FeatureFilterSet::from(Box::new(name_prefix_filter(name_prefix.clone())))
        } else {
            FeatureFilterSet::default()
        }
        .with_filter(project_filter(token));
        filter_set
    }

    async fn resolve_features(
        &self,
        validated_token: &EdgeToken,
        query: Query,
    ) -> EdgeJsonResult<ClientFeatures> {
        let filter_set = Broadcaster::get_query_filters(&query, validated_token);

        let features = self
            .features_cache
            .get(&cache_key(validated_token))
            .map(|client_features| filter_client_features(&client_features, &filter_set));

        match features {
            Some(features) => Ok(Json(ClientFeatures {
                query: Some(query),
                ..features
            })),
            // Note: this is a simplification for now, using the following assumptions:
            // 1. We'll only allow streaming in strict mode
            // 2. We'll check whether the token is subsumed *before* trying to add it to the broadcaster
            // If both of these are true, then we should never hit this case (if Thomas's understanding is correct).
            None => Err(EdgeError::AuthorizationDenied),
        }
    }

    /// Broadcast new features to all clients.
    pub async fn broadcast(
        &self,
        // connections_to_update: &DashMap<QueryWrapper, ClientGroup>
    ) {
        let mut client_events = Vec::new();
        for entry in self.active_connections.iter() {
            let (query, group) = entry.pair();

            let event_data = self
                .resolve_features(&group.token, query.query.clone())
                .await
                .and_then(|features| sse::Data::new_json(&features).map_err(|e| e.into()));

            match event_data {
                Ok(sse_data) => {
                    let event: Event = sse_data.event("unleash-updated").into();

                    for client in &group.clients {
                        client_events.push((client.clone(), event.clone()));
                    }
                }
                Err(e) => {
                    warn!("Failed to broadcast features: {:?}", e);
                }
            }
        }
        // try to send to all clients, ignoring failures
        // disconnected clients will get swept up by `remove_stale_clients`
        let send_events = client_events
            .iter()
            .map(|(client, event)| client.send(event.clone()));

        let _ = future::join_all(send_events).await;
    }
}

// probably not worth taking this out of the broadcaster. It relies on resolving features etc, which is part of the broadcaster
// async fn broadcast(active_connections: &DashMap<QueryWrapper, ClientGroup>) {
//     let mut client_events = Vec::new();
//     for entry in active_connections.iter() {
//         let (query, group) = entry.pair();

//         let event_data = self
//             .resolve_features(&group.token, group.filter_set.clone(), query.query.clone())
//             .await
//             .and_then(|features| sse::Data::new_json(&features).map_err(|e| e.into()));

//         match event_data {
//             Ok(sse_data) => {
//                 let event: Event = sse_data.event("unleash-updated").into();

//                 for client in &group.clients {
//                     client_events.push((client.clone(), event.clone()));
//                 }
//             }
//             Err(e) => {
//                 warn!("Failed to broadcast features: {:?}", e);
//             }
//         }
//     }
//     // try to send to all clients, ignoring failures
//     // disconnected clients will get swept up by `remove_stale_clients`
//     let send_events = client_events
//         .iter()
//         .map(|(client, event)| client.send(event.clone()));

//     let _ = future::join_all(send_events).await;
// }

//
// fn filter_client_groups(
//     update_type: UpdateType,
//     all_connections: &DashMap<QueryWrapper, ClientGroup>,
// ) -> std::iter::Filter<
//     dashmap::iter::Iter<'_, QueryWrapper, ClientGroup>,
//     impl FnMut(&dashmap::mapref::multiple::RefMulti<'_, QueryWrapper, ClientGroup>) -> bool,
// > {
//     all_connections
//         .iter()
//         .filter(|entry| *entry.key)
//     // match update_type {
//     //     UpdateType::Full(environment) |
//     //     UpdateType::Update(environment) => all_connections
//     //         .iter()
//     //         .filter(|entry| entry.value().token.project == key)

//     // }
// }

#[cfg(test)]
mod test {
    use tokio::time::timeout;
    use tokio_stream::StreamExt;

    use crate::{
        feature_cache::FeatureCache,
        tests::features_from_disk,
        types::{TokenType, TokenValidationStatus},
    };

    use super::*;

    #[actix_web::test]
    async fn only_updates_clients_in_same_env() {
        let feature_cache = Arc::new(FeatureCache::default());
        let broadcaster = Broadcaster::new(feature_cache.clone());

        let env_with_updates = "production";

        feature_cache.insert(
            env_with_updates.into(),
            ClientFeatures {
                version: 0,
                features: vec![],
                query: None,
                segments: None,
            },
        );

        let mut rx = broadcaster
            .create_connection(
                EdgeToken {
                    token: "test".to_string(),
                    projects: vec!["dx".to_string()],
                    environment: Some(env_with_updates.into()),
                    token_type: Some(TokenType::Client),
                    status: TokenValidationStatus::Validated,
                },
                Query {
                    tags: None,
                    name_prefix: None,
                    environment: Some(env_with_updates.into()),
                    inline_segment_constraints: None,
                    projects: Some(vec!["dx".to_string()]),
                },
            )
            .await
            .expect("Failed to connect");

        // Drain any initial events to start with a clean state
        while let Ok(Some(event)) = timeout(Duration::from_secs(1), rx.recv()).await {
            println!("Discarding initial event: {:?}", event);
        }

        feature_cache.insert(
            "development".to_string(),
            features_from_disk("../examples/features.json"),
        );

        let mut stream = ReceiverStream::new(rx);

        if tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if let Some(event) = stream.next().await {
                    match event {
                        Event::Data(_) => {
                            // the only kind of data events we send at the moment are unleash-updated events. So if we receive a data event, we've got the update.
                            break;
                        }
                        _ => {
                            // ignore other events
                        }
                    }
                }
            }
        })
        .await
        .is_err()
        {
            // If the test times out, kill the app process and fail the test
            panic!("Test timed out waiting for update event");
        }
    }
}
