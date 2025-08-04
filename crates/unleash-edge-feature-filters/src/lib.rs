use dashmap::mapref::one::Ref;
use unleash_edge_types::tokens::EdgeToken;
use unleash_types::client_features::{ClientFeature, ClientFeatures};

pub mod delta_filters;

pub type FeatureFilter = Box<dyn Fn(&ClientFeature) -> bool>;

#[derive(Default)]
pub struct FeatureFilterSet {
    filters: Vec<FeatureFilter>,
}

impl FeatureFilterSet {
    pub fn from(filter: FeatureFilter) -> Self {
        Self {
            filters: vec![filter],
        }
    }

    pub fn with_filter(mut self, filter: FeatureFilter) -> Self {
        self.filters.push(filter);
        self
    }

    pub fn apply(&self, feature: &ClientFeature) -> bool {
        self.filters.iter().all(|filter| filter(feature))
    }
}

pub fn filter_features(
    feature_cache: &Ref<'_, String, ClientFeatures>,
    filters: &FeatureFilterSet,
) -> Vec<ClientFeature> {
    feature_cache
        .features
        .iter()
        .filter(|feature| filters.apply(feature))
        .cloned()
        .collect::<Vec<ClientFeature>>()
}

pub fn filter_client_features(
    feature_cache: &Ref<'_, String, ClientFeatures>,
    filters: &FeatureFilterSet,
) -> ClientFeatures {
    ClientFeatures {
        features: filter_features(feature_cache, filters),
        segments: feature_cache.segments.clone(),
        query: feature_cache.query.clone(),
        version: feature_cache.version,
        meta: feature_cache.meta.clone(),
    }
}

pub fn name_prefix_filter(name_prefix: String) -> FeatureFilter {
    Box::new(move |f| f.name.starts_with(&name_prefix))
}

pub fn name_match_filter(name_prefix: String) -> FeatureFilter {
    Box::new(move |f| f.name.starts_with(&name_prefix))
}

pub fn project_filter_from_projects(projects: Vec<String>) -> FeatureFilter {
    Box::new(move |feature| {
        if let Some(feature_project) = &feature.project {
            projects.is_empty()
                || projects.contains(&"*".to_string())
                || projects.contains(feature_project)
        } else {
            false
        }
    })
}

pub fn project_filter(token: &EdgeToken) -> FeatureFilter {
    project_filter_from_projects(token.projects.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dashmap::DashMap;
    use unleash_types::client_features::{ClientFeature, ClientFeatures};

    #[test]
    pub fn filter_features_applies_filters() {
        let feature_name = "some-feature".to_string();

        let client_features = ClientFeatures {
            version: 0,
            features: vec![ClientFeature {
                enabled: true,
                ..ClientFeature::default()
            }],
            query: None,
            segments: None,
            meta: None,
        };

        let map: DashMap<String, ClientFeatures> = DashMap::default();
        map.insert(feature_name.clone(), client_features.clone());

        let features = map.get(&feature_name).unwrap();
        let filter_for_enabled = FeatureFilterSet::from(Box::new(|f| f.enabled));
        let enabled_features = filter_features(&features, &filter_for_enabled);

        let features = map.get(&feature_name).unwrap();
        let filter_for_disabled = FeatureFilterSet::from(Box::new(|f| !f.enabled));
        let disabled_features = filter_features(&features, &filter_for_disabled);

        assert_eq!(enabled_features[0].name, client_features.features[0].name);

        assert!(disabled_features.is_empty());
    }

    #[test]
    pub fn chaining_filters_applies_all_filters() {
        let client_features = ClientFeatures {
            version: 0,
            features: vec![
                ClientFeature {
                    name: "feature-one".to_string(),
                    enabled: true,
                    impression_data: Some(false),
                    ..ClientFeature::default()
                },
                ClientFeature {
                    name: "feature-two".to_string(),
                    enabled: false,
                    impression_data: Some(true),
                    ..ClientFeature::default()
                },
                ClientFeature {
                    name: "feature-three".to_string(),
                    enabled: true,
                    impression_data: Some(true),
                    ..ClientFeature::default()
                },
            ],
            query: None,
            segments: None,
            meta: None,
        };

        let map: DashMap<String, ClientFeatures> = DashMap::default();
        let map_key = "some-key".to_string();

        map.insert(map_key.clone(), client_features);
        let features = map.get(&map_key).unwrap();

        let chained_filter = FeatureFilterSet::from(Box::new(|f| f.enabled))
            .with_filter(Box::new(|f| f.impression_data.unwrap_or(false)));
        let enabled_features = filter_features(&features, &chained_filter);

        assert_eq!(enabled_features[0].name, "feature-three".to_string());
    }

    #[test]
    fn name_prefix_filter_filters_by_prefix() {
        let client_features = ClientFeatures {
            version: 0,
            features: vec![
                ClientFeature {
                    name: "feature-one".to_string(),
                    ..ClientFeature::default()
                },
                ClientFeature {
                    name: "feature-two".to_string(),
                    ..ClientFeature::default()
                },
                ClientFeature {
                    name: "feature-three".to_string(),
                    ..ClientFeature::default()
                },
            ],
            query: None,
            segments: None,
            meta: None,
        };

        let map: DashMap<String, ClientFeatures> = DashMap::default();
        let map_key = "some-feature".to_string();

        map.insert(map_key.clone(), client_features);
        let features = map.get(&map_key).unwrap();

        let filter = FeatureFilterSet::from(name_prefix_filter("feature-".to_string()));
        let filtered_features = filter_features(&features, &filter);

        assert_eq!(filtered_features.len(), 3);

        let filter = FeatureFilterSet::from(name_prefix_filter("feature-t".to_string()));
        let filtered_features = filter_features(&features, &filter);

        assert_eq!(filtered_features.len(), 2);

        let filter = FeatureFilterSet::from(name_prefix_filter("feature-o".to_string()));
        let filtered_features = filter_features(&features, &filter);

        assert_eq!(filtered_features.len(), 1);

        let filter = FeatureFilterSet::from(name_prefix_filter("feature-four".to_string()));
        let filtered_features = filter_features(&features, &filter);

        assert_eq!(filtered_features.len(), 0);
    }

    #[test]
    fn project_filter_filters_on_project_tokens() {
        let client_features = ClientFeatures {
            version: 0,
            features: vec![
                ClientFeature {
                    name: "feature-one".to_string(),
                    project: Some("default".to_string()),
                    ..ClientFeature::default()
                },
                ClientFeature {
                    name: "feature-two".to_string(),
                    project: Some("default".to_string()),
                    ..ClientFeature::default()
                },
                ClientFeature {
                    name: "feature-three".to_string(),
                    project: Some("not-default".to_string()),
                    ..ClientFeature::default()
                },
            ],
            query: None,
            segments: None,
            meta: None,
        };

        let map: DashMap<String, ClientFeatures> = DashMap::default();
        let map_key = "some-key".to_string();

        map.insert(map_key.clone(), client_features);
        let features = map.get(&map_key).unwrap();

        let token = EdgeToken {
            projects: vec!["default".to_string()],
            ..Default::default()
        };

        let filter = FeatureFilterSet::from(project_filter(&token));
        let filtered_features = filter_features(&features, &filter);

        assert_eq!(filtered_features.len(), 2);
        assert_eq!(filtered_features[0].name, "feature-one".to_string());
        assert_eq!(filtered_features[1].name, "feature-two".to_string());
    }
}
