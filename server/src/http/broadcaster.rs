use std::{
    hash::{Hash, Hasher},
    sync::Arc,
    time::Duration,
};

use actix_web::{
    rt::time::interval,
    web::{Json, Query},
};
use actix_web_lab::{
    sse::{self, Event, Sse},
    util::InfallibleStream,
};
use aws_config::imds::Client;
use dashmap::DashMap;
use futures::future;
use serde::Serialize;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::warn;
use unleash_types::client_features::{ClientFeatures, Query as FlagQuery};

use crate::{
    error::EdgeError,
    feature_cache::FeatureCache,
    filters::{filter_client_features, name_prefix_filter, project_filter, FeatureFilterSet},
    tokens::cache_key,
    types::{EdgeJsonResult, EdgeResult, EdgeToken, FeatureFilters},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct QueryWrapper {
    query: FlagQuery,
}

impl Hash for QueryWrapper {
    fn hash<H: Hasher>(&self, state: &mut H) {
        serde_json::to_string(&self.query).unwrap().hash(state);
    }
}

#[derive(Clone, Debug)]
struct ClientGroup {
    clients: Vec<mpsc::Sender<sse::Event>>,
    filter_set: Query<FeatureFilters>,
    token: EdgeToken,
}

pub struct Broadcaster {
    active_connections: DashMap<QueryWrapper, ClientGroup>,
    features_cache: Arc<FeatureCache>,
}

impl Broadcaster {
    /// Constructs new broadcaster and spawns ping loop.
    pub fn new(features: Arc<FeatureCache>) -> Arc<Self> {
        let broadcaster = Arc::new(Broadcaster {
            active_connections: DashMap::new(),
            features_cache: features.clone(),
        });

        if let Some(mut rx) = features.subscribe() {
            let this = broadcaster.clone();
            tokio::spawn(async move {
                while let Ok(key) = rx.recv().await {
                    println!("Received update for key: {:?}", key);
                    this.broadcast().await;
                }
            });
        }

        Broadcaster::spawn_heartbeat(Arc::clone(&broadcaster));

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

    /// Removes all non-responsive clients from broadcast list.
    async fn heartbeat(&self) {
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

            // validate tokens here?
            // ok_clients.iter().filter(|client| client.token_is_valid())

            group.clients = ok_clients;
        }
    }

    /// Registers client with broadcaster, returning an SSE response body.
    pub async fn connect(
        &self,
        token: EdgeToken,
        filter_set: Query<FeatureFilters>,
        query: unleash_types::client_features::Query,
    ) -> EdgeResult<Sse<InfallibleStream<ReceiverStream<sse::Event>>>> {
        let (tx, rx) = mpsc::channel(10);

        let features = &self
            .resolve_features(&token, filter_set.clone(), query.clone())
            .await?;

        tx.send(
            sse::Data::new_json(features)?
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
                filter_set,
                token,
            });

        Ok(Sse::from_infallible_receiver(rx))
    }

    fn get_query_filters(
        filter_query: Query<FeatureFilters>,
        token: &EdgeToken,
    ) -> FeatureFilterSet {
        let query_filters = filter_query.into_inner();

        let filter_set = if let Some(name_prefix) = query_filters.name_prefix {
            FeatureFilterSet::from(Box::new(name_prefix_filter(name_prefix)))
        } else {
            FeatureFilterSet::default()
        }
        .with_filter(project_filter(token));
        filter_set
    }

    async fn resolve_features(
        &self,
        validated_token: &EdgeToken,
        filter_set: Query<FeatureFilters>,
        query: FlagQuery,
    ) -> EdgeJsonResult<ClientFeatures> {
        let filter_set = Broadcaster::get_query_filters(filter_set.clone(), validated_token);

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
            None => Err(EdgeError::ClientCacheError),
        }
    }

    /// Broadcast new features to all clients.
    pub async fn broadcast(&self) {
        let mut client_events = Vec::new();
        for entry in self.active_connections.iter() {
            let (query, group) = entry.pair();

            let event_data = self
                .resolve_features(&group.token, group.filter_set.clone(), query.query.clone())
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
