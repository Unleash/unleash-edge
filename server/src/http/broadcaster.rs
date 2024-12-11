/// copied from https://github.com/actix/examples/blob/master/server-sent-events/src/broadcast.rs
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
use dashmap::DashMap;
use futures_util::future;
use serde::Serialize;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use unleash_types::client_features::{ClientFeatures, Query as FlagQuery};

use crate::{
    filters::{filter_client_features, name_prefix_filter, project_filter, FeatureFilterSet},
    tokens::cache_key,
    types::{EdgeResult, EdgeToken, FeatureFilters},
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
    features_cache: Arc<DashMap<String, ClientFeatures>>,
}

impl Broadcaster {
    /// Constructs new broadcaster and spawns ping loop.
    pub fn new(features: Arc<DashMap<String, ClientFeatures>>) -> Arc<Self> {
        let this = Arc::new(Broadcaster {
            active_connections: DashMap::new(),
            features_cache: features,
        });

        Broadcaster::spawn_ping(Arc::clone(&this));

        this
    }

    /// Pings clients every 30 seconds to see if they are alive and remove them from the broadcast
    /// list if not.
    fn spawn_ping(this: Arc<Self>) {
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(30));

            loop {
                interval.tick().await;
                this.remove_stale_clients().await;
            }
        });
    }

    /// Removes all non-responsive clients from broadcast list.
    async fn remove_stale_clients(&self) {
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
    /// The current impl takes the feature set as input and sends it to the client as a connected event.
    ///
    /// The commented-out arguments are what we'll need to store per client so
    /// that we can properly filter / format the feature response when they get
    /// updates later.
    pub async fn connect(
        &self,
        token: EdgeToken,
        filter_set: Query<FeatureFilters>,
        query: unleash_types::client_features::Query,
        features: Json<ClientFeatures>,
    ) -> EdgeResult<Sse<InfallibleStream<ReceiverStream<sse::Event>>>> {
        let (tx, rx) = mpsc::channel(10);

        self.active_connections
            .entry(QueryWrapper {
                query: query.clone(),
            })
            .and_modify(|group| {
                group.clients.push(tx.clone());
            })
            .or_insert(ClientGroup {
                clients: vec![tx.clone()],
                filter_set,
                token,
            });

        tx.send(
            sse::Data::new_json(features)
                .unwrap()
                .event("unleash-connected")
                .into(),
        )
        .await
        .unwrap();

        Ok(Sse::from_infallible_receiver(rx))
    }

    fn get_query_filters(
        filter_query: Query<FeatureFilters>,
        token: EdgeToken,
    ) -> FeatureFilterSet {
        let query_filters = filter_query.into_inner();

        let filter_set = if let Some(name_prefix) = query_filters.name_prefix {
            FeatureFilterSet::from(Box::new(name_prefix_filter(name_prefix)))
        } else {
            FeatureFilterSet::default()
        }
        .with_filter(project_filter(&token));
        filter_set
    }

    /// Broadcast new features to all clients.
    pub async fn broadcast(&self) {
        let mut client_events = Vec::new();
        for entry in self.active_connections.iter() {
            let (_query, group) = entry.pair();
            let filter_set =
                Broadcaster::get_query_filters(group.filter_set.clone(), group.token.clone());
            let features = self
                .features_cache
                .get(&cache_key(&group.token))
                .map(|client_features| filter_client_features(&client_features, &filter_set));
            let event: Event = sse::Data::new_json(&features)
                .unwrap()
                .event("unleash-updated")
                .into();

            for client in &group.clients {
                client_events.push((client.clone(), event.clone()));
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
