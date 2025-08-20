use crate::FeatureFilterSet;
use unleash_edge_delta::cache::DeltaCache;
use unleash_types::client_features::{ClientFeature, ClientFeaturesDelta, DeltaEvent};

pub type DeltaFilter = Box<dyn Fn(&DeltaEvent) -> bool>;

#[derive(Default)]
pub struct DeltaFilterSet {
    filters: Vec<DeltaFilter>,
}

impl DeltaFilterSet {
    pub fn with_filter(mut self, filter: DeltaFilter) -> Self {
        self.filters.push(filter);
        self
    }

    pub fn apply(&self, feature: &DeltaEvent) -> bool {
        self.filters.iter().all(|filter| filter(feature))
    }
}

pub fn revision_id_filter(required_revision_id: u32) -> DeltaFilter {
    Box::new(move |event| event.get_event_id() > required_revision_id)
}

pub fn projects_filter(projects: Vec<String>) -> DeltaFilter {
    let all_projects = projects.contains(&"*".to_string());
    Box::new(move |event| match event {
        DeltaEvent::FeatureUpdated { feature, .. } => {
            if let Some(feature_project) = &feature.project {
                all_projects || projects.contains(feature_project)
            } else {
                false
            }
        }
        DeltaEvent::FeatureRemoved { project, .. } => all_projects || projects.contains(project),
        _ => true,
    })
}

pub fn name_prefix_filter(name_prefix: Option<String>) -> DeltaFilter {
    Box::new(move |event| match (event, &name_prefix) {
        (DeltaEvent::FeatureUpdated { feature, .. }, Some(prefix)) => {
            feature.name.starts_with(prefix)
        }
        (DeltaEvent::FeatureRemoved { feature_name, .. }, Some(prefix)) => {
            feature_name.starts_with(prefix)
        }
        (_, None) => true,
        _ => true,
    })
}

pub fn is_segment_event_filter() -> DeltaFilter {
    Box::new(|event| {
        matches!(
            event,
            DeltaEvent::SegmentUpdated { .. } | DeltaEvent::SegmentRemoved { .. }
        )
    })
}

pub fn combined_filter(
    required_revision_id: u32,
    projects: Vec<String>,
    name_prefix: Option<String>,
) -> DeltaFilter {
    let revision_filter = revision_id_filter(required_revision_id);
    let projects_filter = projects_filter(projects);
    let name_filter = name_prefix_filter(name_prefix);
    let segment_filter = is_segment_event_filter();

    Box::new(move |event| {
        (segment_filter(event) && revision_filter(event))
            || (name_filter(event) && projects_filter(event) && revision_filter(event))
    })
}

pub fn filter_deltas(
    delta_cache: &DeltaCache,
    feature_filters: &FeatureFilterSet,
    delta_filters: &DeltaFilterSet,
    revision: u32,
) -> Vec<DeltaEvent> {
    let hydration_event = delta_cache.get_hydration_event();
    if revision > hydration_event.event_id {
        return vec![];
    }
    if revision > 0 && delta_cache.has_revision(revision) {
        let events = delta_cache.get_events().clone();
        events
            .iter()
            .filter(|delta| delta_filters.apply(delta))
            .cloned()
            .collect::<Vec<DeltaEvent>>()
    } else {
        let hydration_event = delta_cache.get_hydration_event().clone();
        vec![DeltaEvent::Hydration {
            event_id: hydration_event.event_id,
            segments: hydration_event.segments,
            features: hydration_event
                .features
                .iter()
                .filter(|feature| feature_filters.apply(feature))
                .cloned()
                .collect::<Vec<ClientFeature>>(),
        }]
    }
}

pub fn filter_delta_events(
    delta_cache: &DeltaCache,
    feature_filters: &FeatureFilterSet,
    delta_filters: &DeltaFilterSet,
    revision: u32,
) -> ClientFeaturesDelta {
    ClientFeaturesDelta {
        events: filter_deltas(delta_cache, feature_filters, delta_filters, revision),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use unleash_types::client_features::{ClientFeature, DeltaEvent, Segment};

    fn mock_events() -> Vec<DeltaEvent> {
        vec![
            DeltaEvent::FeatureUpdated {
                event_id: 1,
                feature: ClientFeature {
                    name: "test-feature".to_string(),
                    project: Some("project1".to_string()),
                    enabled: true,
                    ..Default::default()
                },
            },
            DeltaEvent::FeatureUpdated {
                event_id: 2,
                feature: ClientFeature {
                    name: "alpha-feature".to_string(),
                    project: Some("project2".to_string()),
                    enabled: true,
                    ..Default::default()
                },
            },
            DeltaEvent::FeatureRemoved {
                event_id: 3,
                feature_name: "beta-feature".to_string(),
                project: "project3".to_string(),
            },
            DeltaEvent::SegmentUpdated {
                event_id: 4,
                segment: Segment {
                    id: 0,
                    constraints: vec![],
                },
            },
            DeltaEvent::SegmentRemoved {
                event_id: 5,
                segment_id: 2,
            },
        ]
    }

    #[test]
    fn filters_events_based_on_event_id() {
        let events = mock_events();
        let delta_filters = DeltaFilterSet::default().with_filter(revision_id_filter(2));
        let filtered: Vec<_> = events
            .iter()
            .filter(|e| delta_filters.apply(e))
            .cloned()
            .collect();

        assert_eq!(filtered.len(), 3);
        assert!(matches!(
            filtered[0],
            DeltaEvent::FeatureRemoved { event_id: 3, .. }
        ));
        assert!(matches!(
            filtered[1],
            DeltaEvent::SegmentUpdated { event_id: 4, .. }
        ));
        assert!(matches!(
            filtered[2],
            DeltaEvent::SegmentRemoved { event_id: 5, .. }
        ));
    }

    #[test]
    fn allows_all_projects_when_wildcard_is_provided() {
        let events = mock_events();
        let delta_filters =
            DeltaFilterSet::default().with_filter(projects_filter(vec!["*".to_string()]));
        let filtered: Vec<_> = events
            .iter()
            .filter(|e| delta_filters.apply(e))
            .cloned()
            .collect();

        assert_eq!(filtered, events);
    }

    #[test]
    fn filters_by_name_prefix() {
        let events = mock_events();
        let delta_filters =
            DeltaFilterSet::default().with_filter(name_prefix_filter(Some("alpha".to_string())));
        let filtered: Vec<_> = events
            .iter()
            .filter(|e| delta_filters.apply(e))
            .cloned()
            .collect();

        assert_eq!(filtered.len(), 3);
        assert!(matches!(
            filtered[0],
            DeltaEvent::FeatureUpdated { event_id: 2, .. }
        ));
        assert!(matches!(
            filtered[1],
            DeltaEvent::SegmentUpdated { event_id: 4, .. }
        ));
        assert!(matches!(
            filtered[2],
            DeltaEvent::SegmentRemoved { event_id: 5, .. }
        ));
    }

    #[test]
    fn filters_by_project_list() {
        let events = mock_events();
        let delta_filters = DeltaFilterSet::default()
            .with_filter(projects_filter(vec!["project3".to_string()]))
            .with_filter(name_prefix_filter(Some("beta".to_string())));
        let filtered: Vec<_> = events
            .iter()
            .filter(|e| delta_filters.apply(e))
            .cloned()
            .collect();

        assert_eq!(filtered.len(), 3);
        assert!(matches!(
            filtered[0],
            DeltaEvent::FeatureRemoved { event_id: 3, .. }
        ));
        assert!(matches!(
            filtered[1],
            DeltaEvent::SegmentUpdated { event_id: 4, .. }
        ));
        assert!(matches!(
            filtered[2],
            DeltaEvent::SegmentRemoved { event_id: 5, .. }
        ));
    }
}
