use std::collections::VecDeque;
use unleash_types::client_features::{DeltaEvent, ClientFeature, Segment};



#[derive(Debug, Clone)]
pub struct DeltaHydrationEvent {
    event_id: i32,
    features: Vec<ClientFeature>,
    segments: Vec<Segment>,
}


#[derive(Debug, Clone)]
pub struct DeltaCache {
    max_length: usize,
    events: VecDeque<DeltaEvent>,
    hydration_event: DeltaHydrationEvent,
}

trait GetEventId {
    fn get_event_id(&self) -> i32;
}

impl GetEventId for DeltaEvent {
    fn get_event_id(&self) -> i32 {
        match self {
            DeltaEvent::FeatureUpdated { event_id, .. }
            | DeltaEvent::FeatureRemoved { event_id, .. }
            | DeltaEvent::SegmentUpdated { event_id, .. }
            | DeltaEvent::SegmentRemoved { event_id, .. }
            | DeltaEvent::Hydration { event_id, .. } => *event_id,
        }
    }
}

impl DeltaCache {
    pub fn new(hydration_event: DeltaHydrationEvent, max_length: usize) -> Self {
        let mut cache = DeltaCache {
            max_length,
            events: VecDeque::new(),
            hydration_event: hydration_event.clone(),
        };
        cache.add_base_event_from_hydration(hydration_event);
        cache
    }

    fn add_base_event_from_hydration(&mut self, hydration_event: DeltaHydrationEvent) {
        if let Some(last_feature) = hydration_event.features.last().cloned() {
            self.add_events(vec![DeltaEvent::FeatureUpdated {
                event_id: hydration_event.event_id,
                feature: last_feature,
            }]);
        }
    }

    pub fn add_events(&mut self, events: Vec<DeltaEvent>) {
        for event in events.into_iter() {
            self.events.push_back(event.clone());
            self.update_hydration_event(event);
            if self.events.len() > self.max_length {
                self.events.pop_front();
            }
        }
    }

    pub fn get_events(&self) -> &VecDeque<DeltaEvent> {
        &self.events
    }

    pub fn is_missing_revision(&self, revision_id: u32) -> bool {
        !self.events.iter().any(|event| event.get_event_id() == revision_id as i32)
    }

    pub fn get_hydration_event(&self) -> &DeltaHydrationEvent {
        &self.hydration_event
    }

    fn update_hydration_event(&mut self, event: DeltaEvent) {
        match event {
            DeltaEvent::FeatureUpdated { event_id, feature } => {
                if let Some(existing) = self.hydration_event.features.iter_mut().find(|f| f.name == feature.name) {
                    *existing = feature;
                } else {
                    self.hydration_event.features.push(feature);
                }
                self.hydration_event.event_id = event_id;
            }
            DeltaEvent::FeatureRemoved { event_id, feature_name } => {
                self.hydration_event.features.retain(|f| f.name != feature_name);
                self.hydration_event.event_id = event_id;
            }
            DeltaEvent::SegmentUpdated { event_id, segment } => {
                if let Some(existing) = self.hydration_event.segments.iter_mut().find(|s| s.id == segment.id) {
                    *existing = segment;
                } else {
                    self.hydration_event.segments.push(segment);
                }
                self.hydration_event.event_id = event_id;
            }
            DeltaEvent::SegmentRemoved { event_id, segment_id } => {
                self.hydration_event.segments.retain(|s| s.id != segment_id);
                self.hydration_event.event_id = event_id;
            }
            DeltaEvent::Hydration { .. } => {
               // do nothing, as hydration will never end up in update events
            }
        }
    }
}


#[test]
fn test_update_hydration_event_and_remove_event_when_over_limit() {
    let base_event = DeltaHydrationEvent {
        event_id: 1,
        features: vec![
            ClientFeature {
                name: "test-flag".to_string(),
                feature_type: None,
                description: None,
                created_at: None,
                last_seen_at: None,
                enabled: false,
                stale: None,
                impression_data: None,
                project: None,
                strategies: None,
                variants: None,
                dependencies: None,
            },
            ClientFeature {
                name: "my-feature-flag".to_string(),
                feature_type: None,
                description: None,
                created_at: None,
                last_seen_at: None,
                enabled: true,
                stale: None,
                impression_data: None,
                project: None,
                strategies: None,
                variants: None,
                dependencies: None,
            },
        ],
        segments: vec![
            Segment { id: 1, constraints: vec![] },
            Segment { id: 2,constraints: vec![] },
        ],
    };
    let max_length = 2;
    let mut delta_cache = DeltaCache::new(base_event.clone(), max_length);

    let initial_events = vec![
        DeltaEvent::FeatureUpdated {
            event_id: 2,
            feature: ClientFeature {
                name: "my-feature-flag".to_string(),
                feature_type: None,
                description: None,
                created_at: None,
                last_seen_at: None,
                enabled: true,
                stale: None,
                impression_data: None,
                project: None,
                strategies: None,
                variants: None,
                dependencies: None,
            },
        },
    ];
    delta_cache.add_events(initial_events);

    let added_events = vec![
        DeltaEvent::FeatureUpdated {
            event_id: 3,
            feature: ClientFeature {
                name: "another-feature-flag".to_string(),
                feature_type: None,
                description: None,
                created_at: None,
                last_seen_at: None,
                enabled: true,
                stale: None,
                impression_data: None,
                project: None,
                strategies: None,
                variants: None,
                dependencies: None,
            },
        },
        DeltaEvent::FeatureRemoved {
            event_id: 4,
            feature_name: "test-flag".to_string(),
        },
        DeltaEvent::SegmentUpdated {
            event_id: 5,
            segment: Segment { id: 1,  constraints: vec![] },
        },
        DeltaEvent::SegmentRemoved {
            event_id: 6,
            segment_id: 2,
        },
        DeltaEvent::SegmentUpdated {
            event_id: 7,
            segment: Segment { id: 3, constraints: vec![] },
        },
    ];
    delta_cache.add_events(added_events.clone());

    let events: Vec<_> = delta_cache.get_events().iter().cloned().collect();
    assert_eq!(events.len(), max_length);
    assert_eq!(events, added_events[added_events.len() - max_length..]);

    let hydration_event = delta_cache.get_hydration_event();
    assert_eq!(hydration_event.features.len(), 2);
    assert_eq!(hydration_event.event_id, 7);
    assert!(hydration_event.features.iter().any(|f| f.name == "my-feature-flag"));
    assert!(hydration_event.features.iter().any(|f| f.name == "another-feature-flag"));
    assert!(hydration_event.segments.iter().any(|s| s.id == 1));
}

#[test]
fn test_prevent_mutation_of_previous_feature_updated_events() {
    let base_event = DeltaHydrationEvent {
        event_id: 1,
        features: vec![ClientFeature {
            name: "base-flag".to_string(),
            feature_type: None,
            description: None,
            created_at: None,
            last_seen_at: None,
            enabled: true,
            stale: None,
            impression_data: None,
            project: None,
            strategies: None,
            variants: None,
            dependencies: None,
        }],
        segments: vec![],
    };
    let mut delta_cache = DeltaCache::new(base_event, 10);

    let initial_feature_event = DeltaEvent::FeatureUpdated {
        event_id: 129,
        feature: ClientFeature {
            name: "streaming-test".to_string(),
            feature_type: None,
            description: None,
            created_at: None,
            last_seen_at: None,
            enabled: false,
            stale: None,
            impression_data: None,
            project: None,
            strategies: None,
            variants: None,
            dependencies: None,
        },
    };
    delta_cache.add_events(vec![initial_feature_event.clone()]);

    let updated_feature_event = DeltaEvent::FeatureUpdated {
        event_id: 130,
        feature: ClientFeature {
            name: "streaming-test".to_string(),
            feature_type: None,
            description: None,
            created_at: None,
            last_seen_at: None,
            enabled: true,
            stale: None,
            impression_data: None,
            project: None,
            strategies: None,
            variants: None,
            dependencies: None,
        },
    };
    delta_cache.add_events(vec![updated_feature_event]);

    assert_eq!(delta_cache.get_events()[1], initial_feature_event);
}
