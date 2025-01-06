use std::{hash::Hash, sync::Arc, time::Duration};

use actix_web::{rt::time::interval, web::Json};
use actix_web_lab::{
    sse::{self, Event, Sse},
    util::InfallibleStream,
};
use dashmap::DashMap;
use futures::future;
use prometheus::{register_int_gauge, IntGauge};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, warn};
use unleash_types::client_features::{ClientFeatures, Query};

use crate::{
    error::EdgeError,
    feature_cache::{FeatureCache, UpdateType},
    filters::{filter_client_features, name_prefix_filter, FeatureFilter, FeatureFilterSet},
    types::{EdgeJsonResult, EdgeResult, EdgeToken},
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct StreamingQuery {
    pub projects: Vec<String>,
    pub name_prefix: Option<String>,
    pub environment: String,
}

impl From<StreamingQuery> for Query {
    fn from(value: StreamingQuery) -> Self {
        Self {
            tags: None,
            name_prefix: value.name_prefix,
            environment: Some(value.environment),
            inline_segment_constraints: None,
            projects: Some(value.projects),
        }
    }
}

impl From<(&Query, &EdgeToken)> for StreamingQuery {
    fn from((query, token): (&Query, &EdgeToken)) -> Self {
        Self {
            projects: token.projects.clone(),
            name_prefix: query.name_prefix.clone(),
            environment: match token.environment {
                Some(ref env) => env.clone(),
                None => token.token.clone(),
            },
        }
    }
}

#[derive(Clone, Debug)]
struct ClientData {
    token: String,
    sender: mpsc::Sender<sse::Event>,
}

#[derive(Clone, Debug)]
struct ClientGroup {
    clients: Vec<ClientData>,
    // last_hash: u64
}

pub struct Broadcaster {
    active_connections: DashMap<StreamingQuery, ClientGroup>,
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
                match key {
                    UpdateType::Full(env) | UpdateType::Update(env) => {
                        this.broadcast(Some(env)).await;
                    }
                    UpdateType::Deletion => {
                        this.broadcast(None).await;
                    }
                }
            }
        });
    }

    /// Removes all non-responsive clients from broadcast list.
    async fn heartbeat(&self) {
        let mut active_connections = 0i64;
        for mut group in self.active_connections.iter_mut() {
            let mut ok_clients = Vec::new();

            for ClientData { token, sender } in &group.clients {
                if sender
                    .send(sse::Event::Comment("keep-alive".into()))
                    .await
                    .is_ok()
                {
                    ok_clients.push(ClientData {
                        token: token.clone(),
                        sender: sender.clone(),
                    });
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
        self.create_connection(StreamingQuery::from((&query, &token)), &token.token)
            .await
            .map(|rx| Sse::from_infallible_receiver(rx))
    }

    async fn create_connection(
        &self,
        query: StreamingQuery,
        token: &str,
    ) -> EdgeResult<mpsc::Receiver<sse::Event>> {
        let (tx, rx) = mpsc::channel(10);

        let features = self.resolve_features(query.clone()).await?;
        tx.send(
            sse::Data::new_json(&features)?
                .event("unleash-connected")
                .into(),
        )
        .await?;

        self.active_connections
            .entry(query)
            .and_modify(|group| {
                group.clients.push(ClientData {
                    token: token.into(),
                    sender: tx.clone(),
                });
            })
            .or_insert(ClientGroup {
                clients: vec![ClientData {
                    token: token.into(),
                    sender: tx.clone(),
                }],
            });

        Ok(rx)
    }

    fn get_query_filters(query: &StreamingQuery) -> FeatureFilterSet {
        let filter_set = if let Some(name_prefix) = &query.name_prefix {
            FeatureFilterSet::from(Box::new(name_prefix_filter(name_prefix.clone())))
        } else {
            FeatureFilterSet::default()
        }
        .with_filter(project_filter(query.projects.clone()));
        filter_set
    }

    async fn resolve_features(&self, query: StreamingQuery) -> EdgeJsonResult<ClientFeatures> {
        let filter_set = Broadcaster::get_query_filters(&query);

        let features = self
            .features_cache
            .get(&query.environment)
            .map(|client_features| filter_client_features(&client_features, &filter_set));

        match features {
            Some(features) => Ok(Json(ClientFeatures {
                query: Some(query.into()),
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
        environment: Option<String>,
        // connections_to_update: &DashMap<QueryWrapper, ClientGroup>
    ) {
        let mut client_events = Vec::new();

        for entry in self.active_connections.iter().filter(|entry| {
            if let Some(env) = &environment {
                entry.key().environment == *env
            } else {
                true
            }
        }) {
            let (query, group) = entry.pair();

            let event_data = self
                .resolve_features(query.clone())
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
            .map(|(ClientData { sender, .. }, event)| sender.send(event.clone()));

        let _ = future::join_all(send_events).await;
    }
}

fn project_filter(projects: Vec<String>) -> FeatureFilter {
    Box::new(move |feature| {
        if let Some(feature_project) = &feature.project {
            projects.is_empty()
                || projects.contains(&"*".to_string())
                || projects.contains(feature_project)
        } else {
            false
        }
    })
}

#[cfg(test)]
mod test {
    use tokio::time::timeout;
    use unleash_types::client_features::ClientFeature;

    use crate::feature_cache::FeatureCache;

    use super::*;

    #[actix_web::test]
    async fn only_updates_clients_in_same_env() {
        let feature_cache = Arc::new(FeatureCache::default());
        let broadcaster = Broadcaster::new(feature_cache.clone());

        let env_with_updates = "production";
        let env_without_updates = "development";
        for env in &[env_with_updates, env_without_updates] {
            feature_cache.insert(
                env.to_string(),
                ClientFeatures {
                    version: 0,
                    features: vec![],
                    query: None,
                    segments: None,
                },
            );
        }

        let mut rx = broadcaster
            .create_connection(
                StreamingQuery {
                    name_prefix: None,
                    environment: env_with_updates.into(),
                    projects: vec!["dx".to_string()],
                },
                "token",
            )
            .await
            .expect("Failed to connect");

        // Drain any initial events to start with a clean state
        while let Ok(Some(_)) = timeout(Duration::from_secs(1), rx.recv()).await {
            // ignored
        }

        feature_cache.insert(
            env_with_updates.to_string(),
            ClientFeatures {
                version: 0,
                features: vec![ClientFeature {
                    name: "flag-a".into(),
                    project: Some("dx".into()),
                    ..Default::default()
                }],
                segments: None,
                query: None,
            },
        );

        if tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if let Some(event) = rx.recv().await {
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

        feature_cache.insert(
            env_without_updates.to_string(),
            ClientFeatures {
                version: 0,
                features: vec![ClientFeature {
                    name: "flag-b".into(),
                    project: Some("dx".into()),
                    ..Default::default()
                }],
                segments: None,
                query: None,
            },
        );

        let result = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if let Some(event) = rx.recv().await {
                    match event {
                        Event::Data(data) => {
                            panic!("Received an update for an env I'm not subscribed to!");
                        }
                        _ => {
                            // ignore other events
                        }
                    }
                }
            }
        })
        .await;

        assert!(result.is_err());
    }
}
