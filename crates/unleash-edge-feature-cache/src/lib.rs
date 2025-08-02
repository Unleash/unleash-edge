use dashmap::DashMap;
use tokio::sync::broadcast;
use unleash_types::client_features::ClientFeaturesDelta;
use unleash_types::{
    Deduplicate,
    client_features::{ClientFeature, ClientFeatures, Segment},
};
use unleash_edge_types::tokens::EdgeToken;

#[derive(Debug, Clone)]
pub enum UpdateType {
    Full(String),
    Update(String),
    Deletion,
}

#[derive(Debug, Clone)]
pub struct FeatureCache {
    features: DashMap<String, ClientFeatures>,
    update_sender: broadcast::Sender<UpdateType>,
}

impl FeatureCache {
    pub fn new(features: DashMap<String, ClientFeatures>) -> Self {
        let (tx, _rx) = tokio::sync::broadcast::channel::<UpdateType>(16);
        Self {
            features,
            update_sender: tx,
        }
    }

    pub fn len(&self) -> usize {
        self.features.len()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<UpdateType> {
        self.update_sender.subscribe()
    }
    pub fn get(&self, key: &str) -> Option<dashmap::mapref::one::Ref<'_, String, ClientFeatures>> {
        self.features.get(key)
    }

    pub fn insert(&self, key: String, features: ClientFeatures) -> Option<ClientFeatures> {
        let v = self.features.insert(key.clone(), features);
        self.send_full_update(key);
        v
    }

    pub fn send_full_update(&self, cache_key: String) {
        let _ = self.update_sender.send(UpdateType::Full(cache_key));
    }

    pub fn remove(&self, key: &str) -> Option<(String, ClientFeatures)> {
        let v = self.features.remove(key);
        self.send_full_update(key.to_string());
        v
    }

    pub fn modify(&self, key: String, token: &EdgeToken, features: ClientFeatures) {
        self.features
            .entry(key.clone())
            .and_modify(|existing_features| {
                let updated = update_client_features(token, existing_features, &features);
                *existing_features = updated;
            })
            .or_insert(features);
        self.send_full_update(key);
    }

    pub fn apply_delta(&self, key: String, delta: &ClientFeaturesDelta) {
        self.features
            .entry(key.clone())
            .and_modify(|existing_features| {
                existing_features.apply_delta(delta);
            })
            .or_insert(ClientFeatures::create_from_delta(delta));
        self.send_full_update(key);
    }

    pub fn is_empty(&self) -> bool {
        self.features.is_empty()
    }

    pub fn iter(&self) -> dashmap::iter::Iter<'_, String, ClientFeatures> {
        self.features.iter()
    }
}

impl Default for FeatureCache {
    fn default() -> Self {
        FeatureCache::new(DashMap::default())
    }
}

fn update_client_features(
    token: &EdgeToken,
    old: &ClientFeatures,
    update: &ClientFeatures,
) -> ClientFeatures {
    let mut updated_features =
        update_projects_from_feature_update(token, &old.features, &update.features);
    updated_features.sort();
    let segments = merge_segments_update(old.segments.clone(), update.segments.clone());
    ClientFeatures {
        version: old.version.max(update.version),
        features: updated_features,
        segments: segments.map(|mut s| {
            s.sort();
            s
        }),
        query: old.query.clone().or(update.query.clone()),
        meta: old.meta.clone().or(update.meta.clone()),
    }
}

pub(crate) fn update_projects_from_feature_update(
    token: &EdgeToken,
    original: &[ClientFeature],
    updated: &[ClientFeature],
) -> Vec<ClientFeature> {
    let projects_to_update = &token.projects;
    if projects_to_update.contains(&"*".into()) {
        updated.into()
    } else {
        let mut to_keep: Vec<ClientFeature> = original
            .iter()
            .filter(|toggle| {
                let p = toggle.project.clone().unwrap_or_else(|| "default".into());
                !projects_to_update.contains(&p)
            })
            .cloned()
            .collect();
        to_keep.extend(updated.iter().cloned());
        to_keep
    }
}

fn merge_segments_update(
    segments: Option<Vec<Segment>>,
    updated_segments: Option<Vec<Segment>>,
) -> Option<Vec<Segment>> {
    match (segments, updated_segments) {
        (Some(s), Some(mut o)) => {
            o.extend(s);
            Some(o.deduplicate())
        }
        (Some(s), None) => Some(s),
        (None, Some(o)) => Some(o),
        (None, None) => None,
    }
}
