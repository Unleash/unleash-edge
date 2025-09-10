use crate::cache::DeltaCache;
use dashmap::DashMap;
use prometheus::{IntCounter, IntGauge, register_int_counter, register_int_gauge};
use std::sync::{Arc, LazyLock};
use tokio::sync::broadcast;
use tracing::info;
use unleash_edge_types::BackgroundTask;
use unleash_edge_types::metrics::instance_data::CONNECTED_STREAMING_CLIENTS;
use unleash_types::client_features::DeltaEvent;

static CONNECTED_STREAMING_CLIENTS_GAUGE: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!(
        CONNECTED_STREAMING_CLIENTS,
        "Number of connected streaming clients",
    )
    .unwrap()
});

static STREAMING_CONNECTIONS_ESTABLISHED: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(
        "streaming_connections_made",
        "Number of connections made (since startup)",
    )
    .unwrap()
});

#[derive(Debug, Clone)]
pub enum DeltaCacheUpdate {
    Full(String),     // environment with a newly inserted cache
    Update(String),   // environment with an updated delta cache
    Deletion(String), // environment removed
}

pub struct DeltaCacheManager {
    caches: DashMap<String, DeltaCache>,
    update_sender: broadcast::Sender<DeltaCacheUpdate>,
}

impl Default for DeltaCacheManager {
    fn default() -> Self {
        Self::new()
    }
}

impl DeltaCacheManager {
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel::<DeltaCacheUpdate>(16);
        Self {
            caches: DashMap::new(),
            update_sender: tx,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<DeltaCacheUpdate> {
        STREAMING_CONNECTIONS_ESTABLISHED.inc();
        let receiver = self.update_sender.subscribe();
        CONNECTED_STREAMING_CLIENTS_GAUGE.set(self.update_sender.receiver_count() as i64);
        receiver
    }

    pub fn get(&self, env: &str) -> Option<DeltaCache> {
        self.caches.get(env).map(|entry| entry.value().clone())
    }

    pub fn insert_cache(&self, env: &str, cache: DeltaCache) {
        self.caches.insert(env.to_string(), cache);
        let _ = self
            .update_sender
            .send(DeltaCacheUpdate::Full(env.to_string()));
    }

    pub fn update_cache(&self, env: &str, events: &[DeltaEvent]) {
        if let Some(mut cache) = self.caches.get_mut(env) {
            cache.add_events(events);
            CONNECTED_STREAMING_CLIENTS_GAUGE.set(self.update_sender.receiver_count() as i64);
            let result = self
                .update_sender
                .send(DeltaCacheUpdate::Update(env.to_string()));
            if result.is_err() {
                info!("No active subscribers to delta broadcast for env: {env}");
            }
        }
    }

    pub fn remove_cache(&self, env: &str) {
        self.caches.remove(env);
        let _ = self
            .update_sender
            .send(DeltaCacheUpdate::Deletion(env.to_string()));
    }
}

pub fn create_terminate_sse_connections_task(
    cache_manager: Arc<DeltaCacheManager>,
) -> BackgroundTask {
    Box::pin(async move {
        for v in cache_manager.caches.iter() {
            let _ = cache_manager
                .update_sender
                .send(DeltaCacheUpdate::Deletion(v.key().clone()));
        }
        let _ = cache_manager.update_sender.closed().await;
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::{DeltaCache, DeltaHydrationEvent};
    use unleash_types::client_features::{ClientFeature, DeltaEvent, Segment};

    #[test]
    fn test_insert_and_update_delta_cache() {
        let hydration = DeltaHydrationEvent {
            event_id: 1,
            features: vec![ClientFeature {
                name: "feature1".to_string(),
                ..Default::default()
            }],
            segments: vec![Segment {
                id: 1,
                constraints: vec![],
            }],
        };
        let max_length = 5;
        let delta_cache = DeltaCache::new(hydration, max_length);
        let delta_cache_manager = DeltaCacheManager::new();
        let env = "test-env";

        let mut rx = delta_cache_manager.subscribe();

        delta_cache_manager.insert_cache(env, delta_cache);

        match rx.try_recv() {
            Ok(DeltaCacheUpdate::Full(e)) => assert_eq!(e, env),
            e => panic!("Expected Full update, got {:?}", e),
        }

        let update_event = DeltaEvent::FeatureUpdated {
            event_id: 2,
            feature: ClientFeature {
                name: "feature1_updated".to_string(),
                ..Default::default()
            },
        };

        delta_cache_manager.update_cache(env, std::slice::from_ref(&update_event));

        match rx.try_recv() {
            Ok(DeltaCacheUpdate::Update(e)) => assert_eq!(e, env),
            e => panic!("Expected Update update, got {:?}", e),
        }

        let cache = delta_cache_manager.get(env).expect("Cache should exist");
        let events = cache.get_events();
        assert_eq!(events.last().unwrap(), &update_event);
    }

    #[test]
    fn test_remove_delta_cache() {
        let hydration = DeltaHydrationEvent {
            event_id: 1,
            features: vec![ClientFeature {
                name: "feature-a".to_string(),
                ..Default::default()
            }],
            segments: vec![],
        };
        let delta_cache = DeltaCache::new(hydration, 3);
        let delta_cache_manager = DeltaCacheManager::new();
        let env = "remove-env";

        delta_cache_manager.insert_cache(env, delta_cache);
        let mut rx = delta_cache_manager.subscribe();
        let _ = rx.try_recv();

        delta_cache_manager.remove_cache(env);
        match rx.try_recv() {
            Ok(DeltaCacheUpdate::Deletion(e)) => assert_eq!(e, env),
            e => panic!("Expected Deletion update, got {:?}", e),
        }
        assert!(delta_cache_manager.get(env).is_none());
    }
}
