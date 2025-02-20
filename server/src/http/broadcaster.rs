use std::{hash::Hash, sync::Arc, time::Duration};

use crate::delta_cache::{DeltaHydrationEvent};
use crate::delta_cache_manager::{DeltaCacheManager, DeltaCacheUpdate};
use crate::{
    error::EdgeError,
    filters::{
        filter_delta_events, name_prefix_filter, project_filter_from_projects, FeatureFilterSet,
    },
    types::{EdgeJsonResult, EdgeResult, EdgeToken},
};
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
use unleash_types::client_features::{ClientFeaturesDelta, DeltaEvent, Query};

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

#[derive(Clone, Debug)]
struct ClientData {
    token: String,
    sender: mpsc::Sender<sse::Event>,
    current_revision: u32
}

#[derive(Clone, Debug)]
struct ClientGroup {
    clients: Vec<ClientData>,
}

pub struct Broadcaster {
    active_connections: DashMap<StreamingQuery, ClientGroup>,
    delta_cache_manager: Arc<DeltaCacheManager>,
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
    pub fn new(delta_cache_manager: Arc<DeltaCacheManager>) -> Arc<Self> {
        let broadcaster = Arc::new(Broadcaster {
            active_connections: DashMap::new(),
            delta_cache_manager,
        });

        Broadcaster::spawn_heartbeat(broadcaster.clone());
        Broadcaster::spawn_delta_cache_manager_subscriber(broadcaster.clone());

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

    fn spawn_delta_cache_manager_subscriber(this: Arc<Self>) {
        let mut rx = this.delta_cache_manager.subscribe();
        tokio::spawn(async move {
            while let Ok(key) = rx.recv().await {
                debug!("Received update for key: {:?}", key);
                match key {
                    DeltaCacheUpdate::Update(env) => {
                        this.broadcast(Some(env.clone())).await;
                    }
                    DeltaCacheUpdate::Deletion(_env) | DeltaCacheUpdate::Full(_env) => {
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

            for ClientData { token, sender, current_revision } in &group.clients {
                if sender
                    .send(sse::Event::Comment("keep-alive".into()))
                    .await
                    .is_ok()
                {
                    ok_clients.push(ClientData {
                        token: token.clone(),
                        sender: sender.clone(),
                        current_revision: current_revision.clone()
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
            .map(Sse::from_infallible_receiver)
    }

    async fn create_connection(
        &self,
        query: StreamingQuery,
        token: &str,
    ) -> EdgeResult<mpsc::Receiver<sse::Event>> {
        let (tx, rx) = mpsc::channel(10);

        let hydration_event = self
            .resolve_delta_cache_hydration_event(query.clone())
            .await?;
        let event_id = hydration_event.get_event_id();
        tx.send(
            sse::Data::new_json(Json(ClientFeaturesDelta {
                events: vec![hydration_event],
            }))?
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
                    current_revision: event_id
                });
            })
            .or_insert(ClientGroup {
                clients: vec![ClientData {
                    token: token.into(),
                    sender: tx.clone(),
                    current_revision: event_id
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
        .with_filter(project_filter_from_projects(query.projects.clone()));
        filter_set
    }

    async fn resolve_last_event_id(
        &self,
        query: StreamingQuery,
    ) -> Option<u32> {
        let delta_cache = self.delta_cache_manager.get(&query.environment);
        match delta_cache {
            Some(delta_cache) => delta_cache.get_events().last().map(|event| event.get_event_id()),
            None => None
        }
    }

    async fn resolve_delta_cache_data(
        &self,
        _last_event_id: Option<u32>,
        query: StreamingQuery,
    ) -> EdgeJsonResult<ClientFeaturesDelta> {
        let filter_set = Broadcaster::get_query_filters(&query);
        let delta_cache = self.delta_cache_manager.get(&query.environment);
        match delta_cache {
            Some(delta_cache) => Ok(Json(filter_delta_events(&delta_cache, &filter_set))),
            None => {
                // Note: this is a simplification for now, using the following assumptions:
                // 1. We'll only allow streaming in strict mode
                // 2. We'll check whether the token is subsumed *before* trying to add it to the broadcaster
                // If both of these are true, then we should never hit this case (if Thomas's understanding is correct).
                Err(EdgeError::AuthorizationDenied)
            }
        }
    }

    async fn resolve_delta_cache_hydration_event(
        &self,
        query: StreamingQuery,
    ) -> Result<DeltaEvent, EdgeError> {
        // do we need filter_set for hydration event?
        let _filter_set = Broadcaster::get_query_filters(&query);
        let delta_cache = self.delta_cache_manager.get(&query.environment);
        match delta_cache {
            Some(delta_cache) => {
                let hydration_event = delta_cache.get_hydration_event();
                let DeltaHydrationEvent {
                    event_id,
                    features,
                    segments,
                } = hydration_event;
                let serialized_event = DeltaEvent::Hydration {
                    event_id: event_id.to_owned(),
                    features: features.to_owned(),
                    segments: segments.to_owned(),
                };

                Ok(serialized_event)
            }
            None => Err(EdgeError::AuthorizationDenied),
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
                    .resolve_delta_cache_data(Some(client.current_revision), query.clone())
                    .await
                    .and_then(|features| sse::Data::new_json(&features).map_err(|e| e.into()));

                let last_event_id = self.resolve_last_event_id(query.clone()).await;

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

#[cfg(test)]
mod test {
    use crate::delta_cache::{DeltaCache, DeltaHydrationEvent};
    use tokio::time::timeout;
    use unleash_types::client_features::{ClientFeature, DeltaEvent};

    use super::*;

    #[actix_web::test]
    async fn only_updates_clients_in_same_env() {
        let delta_cache_manager = Arc::new(DeltaCacheManager::new());
        let broadcaster = Broadcaster::new(delta_cache_manager.clone());

        let env_with_updates = "production";
        let env_without_updates = "development";
        let hydration = DeltaHydrationEvent {
            event_id: 1,
            features: vec![ClientFeature {
                name: "feature1".to_string(),
                ..Default::default()
            }],
            segments: vec![],
        };
        let max_length = 5;
        let delta_cache = DeltaCache::new(hydration, max_length);
        for env in &[env_with_updates, env_without_updates] {
            delta_cache_manager.insert_cache(env.to_string(), delta_cache.clone());
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

        delta_cache_manager.update_cache(
            env_with_updates,
            &vec![DeltaEvent::FeatureUpdated {
                event_id: 2,
                feature: ClientFeature {
                    name: "flag-a".into(),
                    project: Some("dx".into()),
                    ..Default::default()
                },
            }],
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
            panic!("Test timed out waiting for update event");
        }

        delta_cache_manager.update_cache(
            env_without_updates,
            &vec![DeltaEvent::FeatureUpdated {
                event_id: 2,
                feature: ClientFeature {
                    name: "flag-b".into(),
                    project: Some("dx".into()),
                    ..Default::default()
                },
            }],
        );

        let result = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if let Some(event) = rx.recv().await {
                    match event {
                        Event::Data(_) => {
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
