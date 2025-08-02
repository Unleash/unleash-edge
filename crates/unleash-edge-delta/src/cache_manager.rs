use dashmap::DashMap;
use tokio::sync::broadcast;
use tracing::error;
use unleash_types::client_features::DeltaEvent;
use crate::cache::DeltaCache;

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
        self.update_sender.subscribe()
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
            let result = self
                .update_sender
                .send(DeltaCacheUpdate::Update(env.to_string()));
            if let Err(e) = result {
                error!("Unexpected broadcast error: {:#?}", e);
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

#[cfg(test)]
mod tests {
    use super::*;
    use unleash_types::client_features::{ClientFeature, DeltaEvent, Segment};
    use crate::cache::{DeltaCache, DeltaHydrationEvent};

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

        delta_cache_manager.update_cache(env, &[update_event.clone()]);

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
