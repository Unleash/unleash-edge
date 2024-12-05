/// copied from https://github.com/actix/examples/blob/master/server-sent-events/src/broadcast.rs
use std::{collections::HashMap, sync::Arc, time::Duration};

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
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use unleash_types::client_features::ClientFeatures;

use crate::{
    filters::{filter_client_features, name_prefix_filter, project_filter, FeatureFilterSet},
    tokens::cache_key,
    types::{EdgeResult, EdgeToken, FeatureFilters},
};

pub struct Broadcaster {
    inner: Mutex<BroadcasterInner>,
    features_cache: Arc<DashMap<String, ClientFeatures>>,
}

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
    query: unleash_types::client_features::Query,
}

impl std::fmt::Debug for QueryStuff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "QueryStuff")
    }
}

#[derive(Debug, Default)]
struct BroadcasterInner {
    clients: Vec<StreamClient>,
    filters: HashMap<String, QueryStuff>,
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
    pub async fn new_client(
        &self,
        token: EdgeToken,
        filter_set: Query<FeatureFilters>,
        query: unleash_types::client_features::Query,
        features: Json<ClientFeatures>,
    ) -> Sse<InfallibleStream<ReceiverStream<sse::Event>>> {
        let (tx, rx) = mpsc::channel(10);

        let token_string = token.token.clone();
        let query_stuff = QueryStuff {
            token,
            filter_set,
            query,
        };

        self.inner
            .lock()
            .filters
            .insert(token_string.clone(), query_stuff);

        tx.send(
            sse::Data::new_json(features)
                .unwrap()
                .event("unleash-connected")
                .into(),
        )
        .await
        .unwrap();

        self.inner.lock().clients.push(StreamClient {
            stream: tx,
            id: token_string,
        });

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

    /// Broadcasts `msg` to all clients.
    ///
    /// This is the example implementation of the broadcast function. It's not used anywhere today.
    pub async fn broadcast(&self) {
        let clients = self.inner.lock().clients.clone();

        let send_futures = clients.iter().map(|client| {
            let binding = self.inner.lock();
            let query_stuff = binding.filters.get(&client.id).unwrap();
            let filter_set = Broadcaster::get_query_filters(
                query_stuff.filter_set.clone(),
                query_stuff.token.clone(),
            );
            let features = self
                .features_cache
                .get(&cache_key(&query_stuff.token))
                .map(|client_features| filter_client_features(&client_features, &filter_set));
            // let features = get_features_for_filter(query_stuff.token.clone(), &filter_set).unwrap();
            let event = sse::Data::new_json(&features).unwrap().into();
            client.stream.send(event)
        });

        // try to send to all clients, ignoring failures
        // disconnected clients will get swept up by `remove_stale_clients`
        let _ = future::join_all(send_futures).await;
    }
}
