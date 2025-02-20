use crate::delta_cache::DeltaCache;
use crate::filters::FeatureFilterSet;
use tracing::info;
use unleash_types::client_features::{ClientFeature, ClientFeaturesDelta, DeltaEvent};

pub type DeltaFilter = Box<dyn Fn(&DeltaEvent) -> bool>;

#[derive(Default)]
pub(crate) struct DeltaFilterSet {
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

pub(crate) fn revision_id_filter(required_revision_id: u32) -> DeltaFilter {
    Box::new(move |event| match event {
        DeltaEvent::FeatureUpdated { event_id, .. }
        | DeltaEvent::FeatureRemoved { event_id, .. }
        | DeltaEvent::SegmentUpdated { event_id, .. }
        | DeltaEvent::SegmentRemoved { event_id, .. }
        | DeltaEvent::Hydration { event_id, .. } => *event_id > required_revision_id,
    })
}

pub(crate) fn projects_filter(projects: Vec<String>) -> DeltaFilter {
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
        _ => false,
    })
}

pub(crate) fn name_prefix_filter(name_prefix: Option<String>) -> DeltaFilter {
    Box::new(move |event| match (event, &name_prefix) {
        (DeltaEvent::FeatureUpdated { feature, .. }, Some(prefix)) => {
            feature.name.starts_with(prefix)
        }
        (DeltaEvent::FeatureRemoved { feature_name, .. }, Some(prefix)) => {
            feature_name.starts_with(prefix)
        }
        (_, None) => true,
        _ => false,
    })
}

pub(crate) fn is_segment_event_filter() -> DeltaFilter {
    Box::new(|event| {
        matches!(
            event,
            DeltaEvent::SegmentUpdated { .. } | DeltaEvent::SegmentRemoved { .. }
        )
    })
}

pub(crate) fn combined_filter(
    required_revision_id: u32,
    projects: Vec<String>,
    name_prefix: Option<String>,
) -> DeltaFilter {
    let revision_filter = revision_id_filter(required_revision_id);
    let projects_filter = projects_filter(projects);
    let name_filter = name_prefix_filter(name_prefix);
    let segment_filter = is_segment_event_filter();

    Box::new(move |event| {
        segment_filter(event)
            || (name_filter(event) && projects_filter(event) && revision_filter(event))
    })
}

fn filter_deltas(
    delta_cache: &DeltaCache,
    feature_filters: &FeatureFilterSet,
    delta_filters: &DeltaFilterSet,
    revision: u32,
) -> Vec<DeltaEvent> {
    if revision > 0 {
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

pub(crate) fn filter_delta_events(
    delta_cache: &DeltaCache,
    feature_filters: &FeatureFilterSet,
    delta_filters: &DeltaFilterSet,
    revision: u32,
) -> ClientFeaturesDelta {
    info!("filtering delta events for api");

    ClientFeaturesDelta {
        events: filter_deltas(delta_cache, feature_filters, delta_filters, revision),
    }
}
