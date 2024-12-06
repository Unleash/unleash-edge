/// copied from https://github.com/actix/examples/blob/master/server-sent-events/src/broadcast.rs
use std::{
    collections::HashMap,
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
use parking_lot::Mutex;
use serde::Serialize;
use tokio::{net::unix::pipe::Sender, sync::mpsc};
use tokio_stream::wrappers::ReceiverStream;
use unleash_types::client_features::{ClientFeatures, Query as FlagQuery};

use crate::{
    cli,
    filters::{filter_client_features, name_prefix_filter, project_filter, FeatureFilterSet},
    tokens::cache_key,
    types::{EdgeResult, EdgeToken, FeatureFilters},
};

// this doesn't work because filter_set isn't clone. However, we can probably
// find a way around that. For instance, we can create a hash map / dash map of
// some client identifier to each filter set, so that we don't need to clone the
// filter set.

// I'd thought at first that we could map the token to the filter set, but I
// think that might not be enough, as the filter set may also contain query
// param information, which can vary between uses of the same token.

// It might be that the easiest way is to create an ID per client and use that.
// Then, when we drop clients, also drop their corresponding entries from the
// map.

#[derive(Debug, Clone)]

struct StreamClient {
    stream: mpsc::Sender<sse::Event>,
    id: String,
}

struct QueryStuff {
    token: EdgeToken,
    filter_set: Query<FeatureFilters>,
    query: FlagQuery,
}

impl std::fmt::Debug for QueryStuff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "QueryStuff")
    }
}

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

#[derive(Default)]
struct BroadcasterInner {
    active_connections: HashMap<QueryWrapper, ClientGroup>,
    clients: Vec<StreamClient>,
    filters: HashMap<String, QueryStuff>,
}

pub struct Broadcaster {
    inner: Mutex<BroadcasterInner>,
    features_cache: Arc<DashMap<String, ClientFeatures>>,
}

impl Broadcaster {
    /// Constructs new broadcaster and spawns ping loop.
    pub fn new(features: Arc<DashMap<String, ClientFeatures>>) -> Arc<Self> {
        let this = Arc::new(Broadcaster {
            inner: Mutex::new(BroadcasterInner::default()),
            features_cache: features,
        });

        Broadcaster::spawn_ping(Arc::clone(&this));

        this
    }

    /// Pings clients every 30 seconds to see if they are alive and remove them from the broadcast
    /// list if not.
    fn spawn_ping(this: Arc<Self>) {
        actix_web::rt::spawn(async move {
            let mut interval = interval(Duration::from_secs(30));

            loop {
                interval.tick().await;
                this.remove_stale_clients().await;
            }
        });
    }

    /// Removes all non-responsive clients from broadcast list.
    async fn remove_stale_clients(&self) {
        let clients = self.inner.lock().clients.clone();

        let mut ok_clients = Vec::new();

        for client in clients {
            if client
                .stream
                .send(sse::Event::Comment("keep-alive".into()))
                .await
                .is_ok()
            {
                ok_clients.push(client.clone());
            }
        }

        self.inner.lock().clients = ok_clients;
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
    ) -> Sse<InfallibleStream<ReceiverStream<sse::Event>>> {
        let (tx, rx) = mpsc::channel(10);

        self.inner
            .lock()
            .active_connections
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

        Sse::from_infallible_receiver(rx)
        // we're already using remove_stale_clients to clean up disconnected
        // clients and send heartbeats. we probably don't need this.
        // .with_keep_alive(Duration::from_secs(30))
    }

    /// broadcasts a pre-formatted `data` event to all clients.
    ///
    /// The final implementation will probably not use this. Instead, it will
    /// probably use each client's filters to determine the features to send.
    /// We'll need to pass in either the full set of features or a way to filter
    /// them. Both might work.
    pub async fn rebroadcast(&self, data: Event) {
        let clients = self.inner.lock().clients.clone();

        let send_futures = clients
            .iter()
            .map(|client| client.stream.send(data.clone()));

        // try to send to all clients, ignoring failures
        // disconnected clients will get swept up by `remove_stale_clients`
        let _ = future::join_all(send_futures).await;
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
        let active_connections = self.inner.lock().active_connections.clone();

        let mut client_events = Vec::new();
        for (_query, group) in active_connections {
            let filter_set =
                Broadcaster::get_query_filters(group.filter_set.clone(), group.token.clone());
            let features = self
                .features_cache
                .get(&cache_key(&group.token))
                .map(|client_features| filter_client_features(&client_features, &filter_set));
            let event: Event = sse::Data::new_json(&features).unwrap().event("unleash-updated").into();

            for client in group.clients {
                client_events.push((client, event.clone()));
            }
        }
        // try to send to all clients, ignoring failures
        // disconnected clients will get swept up by `remove_stale_clients`
        let send_events = client_events.iter().map(|(client, event)| client.send(event.clone()));

        let _ = future::join_all(send_events).await;
    }
}
