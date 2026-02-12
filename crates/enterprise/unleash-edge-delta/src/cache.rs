use unleash_types::client_features::{ClientFeature, DeltaEvent, Segment};

#[derive(Debug, Clone)]
pub struct DeltaHydrationEvent {
    pub event_id: u32,
    pub features: Vec<ClientFeature>,
    pub segments: Vec<Segment>,
}

#[derive(Debug, Clone)]
pub struct DeltaCache {
    max_length: usize,
    events: Vec<DeltaEvent>,
    hydration_event: DeltaHydrationEvent,
}

impl DeltaCache {
    pub fn new(hydration_event: DeltaHydrationEvent, max_length: usize) -> Self {
        let mut cache = DeltaCache {
            max_length,
            events: Vec::new(),
            hydration_event: hydration_event.clone(),
        };
        cache.add_base_event_from_hydration(&hydration_event);
        cache
    }

    fn add_base_event_from_hydration(&mut self, hydration_event: &DeltaHydrationEvent) {
        if let Some(last_feature) = hydration_event.features.last().cloned() {
            self.add_events(&[DeltaEvent::FeatureUpdated {
                event_id: hydration_event.event_id,
                feature: last_feature,
            }]);
        } else {
            // Keep one marker event so has_revision() can still resolve the hydration revision.
            self.add_events(&[DeltaEvent::Hydration {
                event_id: hydration_event.event_id,
                features: vec![],
                segments: vec![],
            }]);
        }
    }

    pub fn has_revision(&self, revision: u32) -> bool {
        self.get_events()
            .iter()
            .any(|e| e.get_event_id() == revision)
    }

    pub fn add_events(&mut self, events: &[DeltaEvent]) {
        for event in events.iter() {
            self.events.push(event.clone());
            self.update_hydration_event(event);
        }
        self.events.sort_by_key(|event| event.get_event_id());
        if self.events.len() > self.max_length {
            let to_remove = self.events.len() - self.max_length;
            self.events.drain(0..to_remove);
        }
        self.hydration_event.features.sort();
        self.hydration_event.segments.sort();
    }

    pub fn get_events(&self) -> &Vec<DeltaEvent> {
        &self.events
    }

    pub fn is_missing_revision(&self, revision_id: u32) -> bool {
        !self
            .events
            .iter()
            .any(|event| event.get_event_id() == revision_id)
    }

    pub fn get_hydration_event(&self) -> &DeltaHydrationEvent {
        &self.hydration_event
    }

    pub fn merge_hydration_for_projects(
        &mut self,
        projects: &[String],
        hydration_event: DeltaHydrationEvent,
    ) {
        let merged_event_id = self.hydration_event.event_id.max(hydration_event.event_id);

        if projects.iter().any(|project| project == "*") {
            self.hydration_event.features = hydration_event.features;
            self.hydration_event.segments = hydration_event.segments;
        } else {
            self.hydration_event.features = replace_projects_from_hydration(
                projects,
                &self.hydration_event.features,
                &hydration_event.features,
            );
            self.hydration_event.segments =
                merge_segment_updates(&self.hydration_event.segments, &hydration_event.segments);
        }

        self.hydration_event.event_id = merged_event_id;
        self.events.clear();
        self.add_base_event_from_hydration(&self.hydration_event.clone());
    }

    fn update_hydration_event(&mut self, event: &DeltaEvent) {
        let event_id = event.get_event_id();
        self.hydration_event.event_id = self.hydration_event.event_id.max(event_id);
        match event {
            DeltaEvent::FeatureUpdated { feature, .. } => {
                if let Some(existing) = self
                    .hydration_event
                    .features
                    .iter_mut()
                    .find(|f| f.name == feature.name)
                {
                    *existing = feature.clone();
                } else {
                    self.hydration_event.features.push(feature.clone());
                }
            }
            DeltaEvent::FeatureRemoved { feature_name, .. } => {
                self.hydration_event
                    .features
                    .retain(|f| f.name != feature_name.clone());
            }
            DeltaEvent::SegmentUpdated { segment, .. } => {
                if let Some(existing) = self
                    .hydration_event
                    .segments
                    .iter_mut()
                    .find(|s| s.id == segment.id)
                {
                    *existing = segment.clone();
                } else {
                    self.hydration_event.segments.push(segment.clone());
                }
            }
            DeltaEvent::SegmentRemoved { segment_id, .. } => {
                self.hydration_event
                    .segments
                    .retain(|s| s.id != *segment_id);
            }
            DeltaEvent::Hydration { .. } => {
                // do nothing, as hydration will never end up in update events
            }
        }
    }
}

fn replace_projects_from_hydration(
    projects_to_update: &[String],
    existing: &[ClientFeature],
    hydrated: &[ClientFeature],
) -> Vec<ClientFeature> {
    let mut to_keep: Vec<ClientFeature> = existing
        .iter()
        .filter(|toggle| {
            let project = toggle.project.clone().unwrap_or_else(|| "default".into());
            !projects_to_update.contains(&project)
        })
        .cloned()
        .collect();
    to_keep.extend(hydrated.iter().cloned());
    to_keep
}

fn merge_segment_updates(existing: &[Segment], hydrated: &[Segment]) -> Vec<Segment> {
    let mut merged = hydrated.to_vec();
    for segment in existing {
        if !merged.iter().any(|s| s.id == segment.id) {
            merged.push(segment.clone());
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use crate::cache::{DeltaCache, DeltaHydrationEvent};
    use unleash_types::client_features::{ClientFeature, DeltaEvent, Segment};

    #[test]
    fn test_update_hydration_event_and_remove_event_when_over_limit() {
        let base_event = DeltaHydrationEvent {
            event_id: 1,
            features: vec![
                ClientFeature {
                    name: "test-flag".to_string(),
                    ..ClientFeature::default()
                },
                ClientFeature {
                    name: "my-feature-flag".to_string(),
                    ..ClientFeature::default()
                },
            ],
            segments: vec![
                Segment {
                    id: 1,
                    constraints: vec![],
                },
                Segment {
                    id: 2,
                    constraints: vec![],
                },
            ],
        };
        let max_length = 2;
        let mut delta_cache = DeltaCache::new(base_event.clone(), max_length);

        let initial_events = &[DeltaEvent::FeatureUpdated {
            event_id: 2,
            feature: ClientFeature {
                name: "my-feature-flag".to_string(),
                ..ClientFeature::default()
            },
        }];
        delta_cache.add_events(initial_events);

        let added_events = vec![
            DeltaEvent::FeatureUpdated {
                event_id: 3,
                feature: ClientFeature {
                    name: "another-feature-flag".to_string(),
                    ..ClientFeature::default()
                },
            },
            DeltaEvent::FeatureRemoved {
                event_id: 4,
                feature_name: "test-flag".to_string(),
                project: "default".to_string(),
            },
            DeltaEvent::SegmentUpdated {
                event_id: 5,
                segment: Segment {
                    id: 1,
                    constraints: vec![],
                },
            },
            DeltaEvent::SegmentRemoved {
                event_id: 6,
                segment_id: 2,
            },
            DeltaEvent::SegmentUpdated {
                event_id: 7,
                segment: Segment {
                    id: 3,
                    constraints: vec![],
                },
            },
        ];
        delta_cache.add_events(&added_events);

        let events: Vec<_> = delta_cache.get_events().to_vec();
        assert_eq!(events.len(), max_length);
        assert_eq!(events, added_events[added_events.len() - max_length..]);

        let hydration_event = delta_cache.get_hydration_event();
        assert_eq!(hydration_event.features.len(), 2);
        assert_eq!(hydration_event.event_id, 7);
        assert!(
            hydration_event
                .features
                .iter()
                .any(|f| f.name == "my-feature-flag")
        );
        assert!(
            hydration_event
                .features
                .iter()
                .any(|f| f.name == "another-feature-flag")
        );
        assert!(hydration_event.segments.iter().any(|s| s.id == 1));
    }

    #[test]
    fn test_prevent_mutation_of_previous_feature_updated_events() {
        let base_event = DeltaHydrationEvent {
            event_id: 1,
            features: vec![ClientFeature {
                name: "base-flag".to_string(),
                ..ClientFeature::default()
            }],
            segments: vec![],
        };
        let mut delta_cache = DeltaCache::new(base_event, 10);

        let initial_feature_event = DeltaEvent::FeatureUpdated {
            event_id: 129,
            feature: ClientFeature {
                name: "streaming-test".to_string(),
                enabled: false,
                ..ClientFeature::default()
            },
        };
        delta_cache.add_events(std::slice::from_ref(&initial_feature_event));

        let updated_feature_event = DeltaEvent::FeatureUpdated {
            event_id: 130,
            feature: ClientFeature {
                name: "streaming-test".to_string(),
                enabled: true,
                strategies: Some(vec![unleash_types::client_features::Strategy {
                    name: "new-strategy".into(),
                    sort_order: None,
                    segments: None,
                    variants: None,
                    constraints: None,
                    parameters: None,
                }]),
                ..ClientFeature::default()
            },
        };
        delta_cache.add_events(std::slice::from_ref(&updated_feature_event));

        assert_eq!(delta_cache.get_events()[1], initial_feature_event);
        assert_eq!(delta_cache.get_events()[2], updated_feature_event);
    }

    #[test]
    fn test_add_events_keeps_deterministic_event_order() {
        let mut delta_cache = DeltaCache::new(
            DeltaHydrationEvent {
                event_id: 10,
                features: vec![ClientFeature {
                    name: "bootstrap".to_string(),
                    ..ClientFeature::default()
                }],
                segments: vec![],
            },
            10,
        );

        delta_cache.add_events(&[
            DeltaEvent::FeatureUpdated {
                event_id: 12,
                feature: ClientFeature {
                    name: "event-12".to_string(),
                    ..ClientFeature::default()
                },
            },
            DeltaEvent::FeatureUpdated {
                event_id: 11,
                feature: ClientFeature {
                    name: "event-11".to_string(),
                    ..ClientFeature::default()
                },
            },
        ]);

        let ids = delta_cache
            .get_events()
            .iter()
            .map(|event| event.get_event_id())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec![10, 11, 12]);
        assert_eq!(delta_cache.get_hydration_event().event_id, 12);
    }

    #[test]
    fn test_empty_hydration_keeps_revision_marker_without_panicking() {
        let delta_cache = DeltaCache::new(
            DeltaHydrationEvent {
                event_id: 33,
                features: vec![],
                segments: vec![],
            },
            10,
        );

        assert!(delta_cache.has_revision(33));
        assert_eq!(delta_cache.get_events().len(), 1);
        assert!(matches!(
            delta_cache.get_events().first(),
            Some(DeltaEvent::Hydration { event_id: 33, .. })
        ));
    }

    #[test]
    fn test_merge_hydration_for_specific_projects_preserves_other_projects_and_merges_segments() {
        let mut delta_cache = DeltaCache::new(
            DeltaHydrationEvent {
                event_id: 10,
                features: vec![
                    ClientFeature {
                        name: "flag-a-old".to_string(),
                        project: Some("project-a".to_string()),
                        ..ClientFeature::default()
                    },
                    ClientFeature {
                        name: "flag-b".to_string(),
                        project: Some("project-b".to_string()),
                        ..ClientFeature::default()
                    },
                ],
                segments: vec![Segment {
                    id: 1,
                    constraints: vec![],
                }],
            },
            10,
        );

        delta_cache.merge_hydration_for_projects(
            &["project-a".to_string()],
            DeltaHydrationEvent {
                event_id: 12,
                features: vec![ClientFeature {
                    name: "flag-a-new".to_string(),
                    project: Some("project-a".to_string()),
                    ..ClientFeature::default()
                }],
                segments: vec![Segment {
                    id: 2,
                    constraints: vec![],
                }],
            },
        );

        let hydration = delta_cache.get_hydration_event();
        let feature_names = hydration
            .features
            .iter()
            .map(|feature| feature.name.clone())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(feature_names.len(), 2);
        assert!(feature_names.contains("flag-a-new"));
        assert!(feature_names.contains("flag-b"));

        let segment_ids = hydration
            .segments
            .iter()
            .map(|segment| segment.id)
            .collect::<Vec<_>>();
        assert_eq!(segment_ids, vec![1, 2]);

        assert_eq!(delta_cache.get_events().len(), 1);
        assert!(delta_cache.has_revision(12));
    }

    #[test]
    fn test_merge_hydration_for_wildcard_replaces_all_projects() {
        let mut delta_cache = DeltaCache::new(
            DeltaHydrationEvent {
                event_id: 1,
                features: vec![
                    ClientFeature {
                        name: "flag-a".to_string(),
                        project: Some("project-a".to_string()),
                        ..ClientFeature::default()
                    },
                    ClientFeature {
                        name: "flag-b".to_string(),
                        project: Some("project-b".to_string()),
                        ..ClientFeature::default()
                    },
                ],
                segments: vec![Segment {
                    id: 1,
                    constraints: vec![],
                }],
            },
            10,
        );

        delta_cache.merge_hydration_for_projects(
            &["*".to_string()],
            DeltaHydrationEvent {
                event_id: 2,
                features: vec![ClientFeature {
                    name: "flag-wildcard".to_string(),
                    project: Some("project-c".to_string()),
                    ..ClientFeature::default()
                }],
                segments: vec![Segment {
                    id: 9,
                    constraints: vec![],
                }],
            },
        );

        let hydration = delta_cache.get_hydration_event();
        assert_eq!(hydration.event_id, 2);
        assert_eq!(hydration.features.len(), 1);
        assert_eq!(hydration.features[0].name, "flag-wildcard");
        assert_eq!(hydration.segments.len(), 1);
        assert_eq!(hydration.segments[0].id, 9);
    }

    #[test]
    fn test_merge_hydration_for_projects_keeps_monotonic_event_id() {
        let mut delta_cache = DeltaCache::new(
            DeltaHydrationEvent {
                event_id: 20,
                features: vec![ClientFeature {
                    name: "flag-a".to_string(),
                    project: Some("project-a".to_string()),
                    ..ClientFeature::default()
                }],
                segments: vec![],
            },
            10,
        );

        delta_cache.merge_hydration_for_projects(
            &["project-a".to_string()],
            DeltaHydrationEvent {
                event_id: 10,
                features: vec![ClientFeature {
                    name: "flag-a-stale".to_string(),
                    project: Some("project-a".to_string()),
                    ..ClientFeature::default()
                }],
                segments: vec![],
            },
        );

        assert_eq!(delta_cache.get_hydration_event().event_id, 20);
        assert_eq!(delta_cache.get_events().len(), 1);
        assert_eq!(delta_cache.get_events()[0].get_event_id(), 20);
    }
}
