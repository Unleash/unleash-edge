use unleash_types::{
    client_features::{ClientFeature, Payload},
    frontend::{EvaluatedToggle, EvaluatedVariant},
};
use unleash_yggdrasil::{Context, EngineState};

use crate::types::EdgeToken;

pub mod builder;
pub mod memory_provider;

trait ProjectFilter<T> {
    fn filter_by_projects(&self, token: &EdgeToken) -> Vec<T>;
}


impl ProjectFilter<ClientFeature> for Vec<ClientFeature> {
    fn filter_by_projects(&self, token: &EdgeToken) -> Vec<ClientFeature> {
        self.iter()
            .filter(|feature| {
                if let Some(feature_project) = &feature.project {
                    token.projects.contains(&"*".to_string())
                        || token.projects.contains(feature_project)
                } else {
                    false
                }
            })
            .cloned()
            .collect::<Vec<ClientFeature>>()
    }
}

pub(crate) fn evaluate_toggles(
    engine: &EngineState,
    include_disabled: bool,
    context: &Context,
) -> Vec<EvaluatedToggle> {
    let resolved = engine.resolve_all(context).unwrap();
    resolved
        .iter()
        .map(|(name, resolved_toggle)| EvaluatedToggle {
            name: name.clone(),
            enabled: resolved_toggle.enabled,
            variant: EvaluatedVariant {
                name: resolved_toggle.variant.name.clone(),
                enabled: resolved_toggle.variant.enabled,
                payload: resolved_toggle
                    .variant
                    .payload
                    .as_ref()
                    .map(|success| Payload {
                        payload_type: success.payload_type.clone(),
                        value: success.value.clone(),
                    }),
            },
            project: resolved_toggle.project.clone(),
            impression_data: resolved_toggle.impression_data,
        })
        .filter(|t| include_disabled || t.enabled)
        .collect()
}
