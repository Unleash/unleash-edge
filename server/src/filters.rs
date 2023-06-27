use crate::{http::feature_refresher::FeatureFilter, types::EdgeToken};

pub fn project_filter(token: &EdgeToken) -> FeatureFilter {
    let token = token.clone();
    Box::new(move |feature| {
        if let Some(feature_project) = &feature.project {
            token.projects.is_empty()
                || token.projects.contains(&"*".to_string())
                || token.projects.contains(feature_project)
        } else {
            false
        }
    })
}
