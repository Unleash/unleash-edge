use axum::response::{Sse, sse::Event};
use dashmap::DashMap;
use futures::future;
use futures::{Stream, StreamExt};
use prometheus::{IntGauge, register_int_gauge};
use serde_json::to_string;
use std::{
    convert::Infallible,
    sync::{Arc, LazyLock},
    time::Duration,
};
use tokio::{
    sync::mpsc::{self, Receiver},
    time::interval,
};
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, warn};
use unleash_edge_delta::cache_manager::{DeltaCacheManager, DeltaCacheUpdate};
use unleash_edge_feature_filters::{
    FeatureFilterSet,
    delta_filters::{DeltaFilterSet, combined_filter, filter_delta_events},
    name_prefix_filter, project_filter_from_projects,
};
use unleash_edge_types::{EdgeResult, errors::EdgeError, tokens::EdgeToken};
use unleash_types::client_features::{ClientFeaturesDelta, Query};

static CONNECTED_STREAMING_CLIENTS: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!(
        "connected_streaming_clients",
        "Number of connected streaming clients",
    )
    .unwrap()
});

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct StreamingQuery {
    pub projects: Vec<String>,
    pub name_prefix: Option<String>,
    pub environment: String,
}

#[derive(Debug, Clone)]
pub struct ClientData {
    pub token: String,
    pub sender: mpsc::Sender<Event>,
    pub current_revision: u32,
}

#[derive(Debug)]
struct ClientGroup {
    clients: Vec<ClientData>,
}

impl From<StreamingQuery> for Query {
    fn from(value: StreamingQuery) -> Self {
        Self {
            tags: None,
            name_prefix: value.name_prefix,
            environment: Some(value.environment),
            inline_segment_constraints: Some(false),
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

fn spawn_delta_cache_manager_subscriber(broadcaster: Arc<Broadcaster>) {
    let mut rx = broadcaster.delta_cache_manager.subscribe();
    tokio::spawn(async move {
        while let Ok(key) = rx.recv().await {
            debug!("Received update for key: {:?}", key);
            match key {
                DeltaCacheUpdate::Update(env) => {
                    broadcaster.broadcast(Some(env.clone())).await;
                }
                DeltaCacheUpdate::Deletion(_env) | DeltaCacheUpdate::Full(_env) => {
                    broadcaster.broadcast(None).await;
                }
            }
        }
    });
}

/// Pings clients every 30 seconds to see if they are alive and remove them from the broadcast
/// list if not.
fn spawn_heartbeat(this: Arc<Broadcaster>) {
    tokio::spawn(async move {
        let mut interval = interval(Duration::from_secs(30));

        loop {
            interval.tick().await;
            this.heartbeat().await;
        }
    });
}

pub struct Broadcaster {
    delta_cache_manager: Arc<DeltaCacheManager>,
    active_connections: DashMap<StreamingQuery, ClientGroup>,
}

impl Broadcaster {
    pub fn new(delta_cache_manager: Arc<DeltaCacheManager>) -> Arc<Self> {
        let broadcaster = Arc::new(Broadcaster {
            active_connections: DashMap::new(),
            delta_cache_manager,
        });

        spawn_heartbeat(broadcaster.clone());
        spawn_delta_cache_manager_subscriber(broadcaster.clone());

        broadcaster
    }

    pub async fn connect(
        &self,
        token: EdgeToken,
        query: Query,
    ) -> EdgeResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
        let rx = self
            .create_connection(StreamingQuery::from((&query, &token)), &token.token)
            .await?;

        let stream = ReceiverStream::new(rx).map(Ok);
        Ok(Sse::new(stream))
    }

    pub async fn create_connection(
        &self,
        query: StreamingQuery,
        token: &str,
    ) -> EdgeResult<Receiver<Event>> {
        let (tx, rx) = mpsc::channel(10);

        let event_data = self
            .build_hydration_events(0, &query)
            .await
            .and_then(|features| {
                to_string(&features)
                    .map(|json| Event::default().event("unleash-connected").data(json))
                    .map_err(EdgeError::from)
            })?;

        tx.send(event_data)
            .await
            .map_err(|e| EdgeError::SseError(e.to_string()))?;

        let current_revision = self.resolve_last_event_id(&query).await.unwrap_or(0);

        let client = ClientData {
            token: token.to_string(),
            sender: tx.clone(),
            current_revision,
        };

        self.active_connections
            .entry(query)
            .and_modify(|group| group.clients.push(client.clone()))
            .or_insert_with(|| ClientGroup {
                clients: vec![client],
            });

        Ok(rx)
    }

    async fn build_hydration_events(
        &self,
        last_event_id: u32,
        query: &StreamingQuery,
    ) -> EdgeResult<ClientFeaturesDelta> {
        let filter_set = Broadcaster::get_query_filters(&query);
        let delta_filter_set = DeltaFilterSet::default().with_filter(combined_filter(
            last_event_id,
            query.projects.clone(),
            query.name_prefix.clone(),
        ));
        let delta_cache = self.delta_cache_manager.get(&query.environment);
        match delta_cache {
            Some(delta_cache) => Ok(filter_delta_events(
                &delta_cache,
                &filter_set,
                &delta_filter_set,
                last_event_id,
            )),
            None => {
                // Note: this is a simplification for now, using the following assumptions:
                // 1. We'll only allow streaming in strict mode
                // 2. We'll check whether the token is subsumed *before* trying to add it to the broadcaster
                // If both of these are true, then we should never hit this case (if Thomas's understanding is correct).
                Err(EdgeError::AuthorizationDenied)
            }
        }
    }

    /// Removes all non-responsive clients from broadcast list.
    async fn heartbeat(&self) {
        let mut active_connections = 0i64;
        for mut group in self.active_connections.iter_mut() {
            let mut ok_clients = Vec::new();

            for ClientData {
                token,
                sender,
                current_revision,
            } in &group.clients
            {
                if sender
                    .send(Event::default().comment("keep-alive"))
                    .await
                    .is_ok()
                {
                    ok_clients.push(ClientData {
                        token: token.clone(),
                        sender: sender.clone(),
                        current_revision: current_revision.to_owned(),
                    });
                }
            }

            active_connections += ok_clients.len() as i64;
            group.clients = ok_clients;
        }
        CONNECTED_STREAMING_CLIENTS.set(active_connections)
    }

    fn get_query_filters(query: &StreamingQuery) -> FeatureFilterSet {
        if let Some(name_prefix) = &query.name_prefix {
            FeatureFilterSet::from(name_prefix_filter(name_prefix.clone()))
        } else {
            FeatureFilterSet::default()
        }
        .with_filter(project_filter_from_projects(query.projects.clone()))
    }

    async fn resolve_last_event_id(&self, query: &StreamingQuery) -> Option<u32> {
        let delta_cache = self.delta_cache_manager.get(&query.environment);
        match delta_cache {
            Some(delta_cache) => delta_cache
                .get_events()
                .last()
                .map(|event| event.get_event_id()),
            None => None,
        }
    }

    /// Broadcast new event deltas to all clients.
    pub async fn broadcast(&self, environment: Option<String>) {
        let mut client_events = Vec::new();

        for mut entry in self.active_connections.iter_mut().filter(|entry| {
            if let Some(env) = &environment {
                entry.key().environment == *env
            } else {
                true
            }
        }) {
            let (query, group) = entry.pair_mut();

            for client in &mut group.clients {
                let event_data = self
                    .build_hydration_events(client.current_revision, query)
                    .await
                    .and_then(|features| {
                        to_string(&features)
                            .map(|json| Event::default().event("unleash-update").data(json))
                            .map_err(EdgeError::from)
                    });

                let last_event_id = self.resolve_last_event_id(query).await;

                if let Ok(sse_data) = event_data {
                    let event: Event = sse_data.event("unleash-updated").into();
                    client_events.push((client.clone(), event.clone()));

                    if let Some(new_id) = last_event_id {
                        client.current_revision = new_id;
                    }
                } else if let Err(e) = event_data {
                    warn!("Failed to broadcast features: {:?}", e);
                }
            }
        }

        // Try to send to all clients, ignoring failures
        // Disconnected clients will get swept up by the heartbeat cleanup
        let send_events = client_events
            .iter()
            .map(|(client, event)| client.sender.send(event.clone()));

        let _ = future::join_all(send_events).await;
    }
}
