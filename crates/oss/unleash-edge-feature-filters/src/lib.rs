use std::collections::HashSet;

use dashmap::mapref::one::Ref;
use tracing::info;
use unleash_edge_types::{
    EdgeResult, FeatureFilters, TokenCache, errors::EdgeError, tokens::EdgeToken,
};
use unleash_types::client_features::{ClientFeature, ClientFeatures, Segment};

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

fn filter_features(
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
    let features = filter_features(feature_cache, filters);

    ClientFeatures {
        segments: filter_segments(&features, feature_cache.segments.as_deref()),
        features,
        query: feature_cache.query.clone(),
        version: feature_cache.version,
        meta: feature_cache.meta.clone(),
    }
}

fn filter_segments(
    features: &[ClientFeature],
    segments: Option<&[Segment]>,
) -> Option<Vec<Segment>> {
    let segments = segments?;

    let mut required = std::collections::HashSet::new();

    for feature in features {
        let Some(strategies) = &feature.strategies else {
            continue;
        };
        for strategy in strategies {
            let Some(seg_ids) = &strategy.segments else {
                continue;
            };
            for &id in seg_ids {
                required.insert(id);
            }
        }
    }

    if required.is_empty() {
        return None;
    }

    let out: Vec<Segment> = segments
        .iter()
        .filter(|s| required.contains(&s.id))
        .cloned()
        .collect();

    Some(out)
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

pub fn get_feature_filter(
    edge_token: &EdgeToken,
    token_cache: &TokenCache,
    filter_query: FeatureFilters,
) -> EdgeResult<(
    EdgeToken,
    FeatureFilterSet,
    unleash_types::client_features::Query,
)> {
    info!("Checking {edge_token:?}");
    let validated_token = token_cache
        .get(&edge_token.token)
        .map(|e| e.value().clone())
        .ok_or(EdgeError::AuthorizationDenied)?;

    let query_filters = filter_query;
    let query = unleash_types::client_features::Query {
        tags: None,
        projects: Some(validated_token.projects.clone()),
        name_prefix: query_filters.name_prefix.clone(),
        environment: validated_token.environment.clone(),
        inline_segment_constraints: Some(false),
    };

    let filter_set = if let Some(name_prefix) = query_filters.name_prefix {
        FeatureFilterSet::from(Box::new(name_prefix_filter(name_prefix)))
    } else {
        FeatureFilterSet::default()
    }
    .with_filter(project_filter(&validated_token));

    Ok((validated_token, filter_set, query))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dashmap::DashMap;
    use unleash_types::client_features::{ClientFeature, ClientFeatures, Strategy};

    fn strategy_test_default() -> Strategy {
        Strategy {
            name: "default".to_string(),
            parameters: None,
            segments: None,
            sort_order: None,
            constraints: None,
            variants: None,
        }
    }

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

    #[test]
    fn filtering_prunes_out_segments_not_required_by_strategies() {
        let client_features = ClientFeatures {
            version: 0,
            features: vec![
                ClientFeature {
                    name: "feature-one".to_string(),
                    strategies: Some(vec![Strategy {
                        segments: Some(vec![2, 3]),
                        ..strategy_test_default()
                    }]),
                    ..ClientFeature::default()
                },
                ClientFeature {
                    name: "feature-two".to_string(),
                    strategies: Some(vec![Strategy {
                        segments: Some(vec![4, 5]),
                        ..strategy_test_default()
                    }]),
                    ..ClientFeature::default()
                },
            ],
            query: None,
            segments: Some(
                (1..=6)
                    .map(|id| Segment {
                        id,
                        constraints: vec![],
                    })
                    .collect(),
            ),

            meta: None,
        };

        let feature_cache: DashMap<String, ClientFeatures> = DashMap::default();
        let map_key = "some-key".to_string();

        feature_cache.insert(map_key.clone(), client_features);

        let features = feature_cache.get(&map_key).unwrap();
        let filter = FeatureFilterSet::from(Box::new(|f| f.name == "feature-one".to_string()));
        let filtered_client_features = filter_client_features(&features, &filter);

        let sent_segments = filtered_client_features.segments.as_ref().unwrap();

        //and we should only expect segments 2 and 3 to remain
        assert_eq!(sent_segments.len(), 2);

        assert_eq!(sent_segments[0].id, 2);
        assert_eq!(sent_segments[1].id, 3);
    }

    #[test]
    fn no_segments_are_sent_if_not_required() {
        let client_features = ClientFeatures {
            version: 0,
            features: vec![
                ClientFeature {
                    name: "feature-one".to_string(),
                    strategies: Some(vec![Strategy {
                        segments: None,
                        ..strategy_test_default()
                    }]),
                    ..ClientFeature::default()
                },
                ClientFeature {
                    name: "feature-two".to_string(),
                    strategies: Some(vec![Strategy {
                        segments: Some(vec![4, 5]),
                        ..strategy_test_default()
                    }]),
                    ..ClientFeature::default()
                },
            ],
            query: None,
            segments: Some(
                (1..=6)
                    .map(|id| Segment {
                        id,
                        constraints: vec![],
                    })
                    .collect(),
            ),

            meta: None,
        };

        let feature_cache: DashMap<String, ClientFeatures> = DashMap::default();
        let map_key = "some-key".to_string();

        feature_cache.insert(map_key.clone(), client_features);

        let features = feature_cache.get(&map_key).unwrap();
        let filter = FeatureFilterSet::from(Box::new(|f| f.name == "feature-one".to_string()));
        let filtered_client_features = filter_client_features(&features, &filter);

        assert!(filtered_client_features.segments.is_none());
    }
}
