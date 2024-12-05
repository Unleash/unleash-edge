/// copied from https://github.com/actix/examples/blob/master/server-sent-events/src/broadcast.rs
use std::{sync::Arc, time::Duration};

use actix_web::{rt::time::interval, web::Json};
use actix_web_lab::{
    sse::{self, Event, Sse},
    util::InfallibleStream,
};
use futures_util::future;
use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use unleash_types::client_features::{ClientFeatures, Query};

use crate::{filters::FeatureFilterSet, types::EdgeToken};

pub struct Broadcaster {
    inner: Mutex<BroadcasterInner>,
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

// #[derive(Debug, Clone)]

struct StreamClient {
    stream: mpsc::Sender<sse::Event>,
    token: EdgeToken,
    filter_set: FeatureFilterSet,
    query: Query,
}

#[derive(Debug, Default)]
struct BroadcasterInner {
    clients: Vec<mpsc::Sender<sse::Event>>,
}

impl Broadcaster {
    /// Constructs new broadcaster and spawns ping loop.
    pub fn create() -> Arc<Self> {
        let this = Arc::new(Broadcaster {
            inner: Mutex::new(BroadcasterInner::default()),
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
        // token: EdgeToken,
        // filter_set: FeatureFilterSet,
        // query: Query,
        features: Json<ClientFeatures>,
    ) -> Sse<InfallibleStream<ReceiverStream<sse::Event>>> {
        let (tx, rx) = mpsc::channel(10);

        tx.send(
            sse::Data::new_json(features)
                .unwrap()
                .event("unleash-connected")
                .into(),
        )
        .await
        .unwrap();

        self.inner.lock().clients.push(tx);

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

        let send_futures = clients.iter().map(|client| client.send(data.clone()));

        // try to send to all clients, ignoring failures
        // disconnected clients will get swept up by `remove_stale_clients`
        let _ = future::join_all(send_futures).await;
    }

    /// Broadcasts `msg` to all clients.
    ///
    /// This is the example implementation of the broadcast function. It's not used anywhere today.
    pub async fn broadcast(&self, msg: &str) {
        let clients = self.inner.lock().clients.clone();

        let send_futures = clients
            .iter()
            .map(|client| client.send(sse::Data::new(msg).into()));

        // try to send to all clients, ignoring failures
        // disconnected clients will get swept up by `remove_stale_clients`
        let _ = future::join_all(send_futures).await;
    }
}
