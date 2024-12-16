use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::broadcast;
use unleash_types::{
    client_features::{ClientFeature, ClientFeatures, Segment},
    Deduplicate,
};

use crate::types::EdgeToken;

#[derive(Debug, Clone, Default)]
pub struct FeatureCache {
    pub features: DashMap<String, ClientFeatures>,
    pub update_sender: Option<broadcast::Sender<String>>,
}

impl FeatureCache {
    pub fn new(
        features: DashMap<String, ClientFeatures>,
        update_sender: broadcast::Sender<String>,
    ) -> Self {
        Self {
            features,
            update_sender: Some(update_sender),
        }
    }

    pub fn get(&self, key: &str) -> Option<dashmap::mapref::one::Ref<'_, String, ClientFeatures>> {
        self.features.get(key)
    }

    pub fn insert(&self, key: String, features: ClientFeatures) -> Option<ClientFeatures> {
        let v = self.features.insert(key.clone(), features);
        self.send(key);
        v
    }

    pub fn send(&self, cache_key: String) {
        if let Some(sender) = self.update_sender.clone() {
            let _ = sender.send(cache_key);
        }
    }

    pub fn remove(&self, key: &str) -> Option<(String, ClientFeatures)> {
        let v = self.features.remove(key);
        self.send(key.to_string());
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
        self.send(key);
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
