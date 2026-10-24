//! Opt-in, read-only comparison using the existing token-scoped snapshot endpoint.
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, LazyLock},
    time::{Duration, Instant},
};

use prometheus::{
    Histogram, IntCounterVec, IntGauge, register_histogram, register_int_counter_vec,
    register_int_gauge,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::watch::Receiver;
use unleash_edge_feature_cache::FeatureCache;
use unleash_edge_http_client::UnleashClient;
use unleash_edge_types::{
    ClientFeaturesRequest, ClientFeaturesResponse, RefreshState,
    tokens::{EdgeToken, cache_key},
};
use unleash_types::client_features::ClientFeatures;

use crate::TokenRefreshSet;

static CHECKS: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        "edge_state_consistency_checks_total",
        "Snapshot comparisons by result",
        &["result"]
    )
    .unwrap()
});
static MISMATCHES: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!(
        "edge_state_consistency_mismatching_scopes",
        "Active scopes whose last successful comparison differed"
    )
    .unwrap()
});
static OBSERVED: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!(
        "edge_state_consistency_observed_scopes",
        "Active refresh tokens with a successful comparison"
    )
    .unwrap()
});
static AGE: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!(
        "edge_state_consistency_oldest_mismatch_seconds",
        "Age of oldest observed unresolved mismatch"
    )
    .unwrap()
});
static OVER_THRESHOLD: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!(
        "edge_state_consistency_scopes_over_threshold",
        "Unresolved mismatches exceeding the configured threshold"
    )
    .unwrap()
});
static EPISODES: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        "edge_state_consistency_episodes_total",
        "Mismatch episodes started or exceeding threshold once",
        &["event"]
    )
    .unwrap()
});
static CONVERGENCE: LazyLock<Histogram> = LazyLock::new(|| {
    register_histogram!(
        "edge_state_consistency_convergence_seconds",
        "Observed mismatch duration until a matching snapshot",
        vec![1.0, 5.0, 10.0, 20.0, 30.0, 60.0, 120.0, 300.0]
    )
    .unwrap()
});

/// Hash the SDK configuration, excluding transport metadata and unrelated projects/segments.
/// Keep nested array order: constraint, strategy and variant order can be meaningful.
fn fingerprint(state: &ClientFeatures, token: &EdgeToken) -> Result<Vec<u8>, serde_json::Error> {
    let mut state = state.clone();
    state.features.retain(|feature| {
        token
            .projects
            .iter()
            .any(|p| p == "*" || feature.project.as_ref() == Some(p))
    });
    state.features.sort_by(|a, b| a.name.cmp(&b.name));
    let required: HashSet<_> = state
        .features
        .iter()
        .flat_map(|f| f.strategies.iter().flatten())
        .flat_map(|s| s.segments.iter().flatten())
        .copied()
        .collect();
    let mut segments = state.segments.take().unwrap_or_default();
    segments.retain(|s| required.contains(&s.id));
    segments.sort_by_key(|s| s.id);
    let mut value = serde_json::json!({ "features": state.features, "segments": segments });
    sort_keys(&mut value);
    Ok(Sha256::digest(serde_json::to_vec(&value)?).to_vec())
}

fn sort_keys(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.sort_keys();
            for value in map.values_mut() {
                sort_keys(value);
            }
        }
        Value::Array(values) => values.iter_mut().for_each(sort_keys),
        _ => {}
    }
}

#[derive(Default)]
struct Observation {
    since: Option<Instant>,
    exceeded: bool,
}

impl Observation {
    // Returns (new mismatch, newly exceeded, completed duration).
    fn observe(
        &mut self,
        matches: bool,
        now: Instant,
        threshold: Duration,
    ) -> (bool, bool, Option<f64>) {
        if matches {
            let elapsed = self
                .since
                .take()
                .map(|start| now.duration_since(start).as_secs_f64());
            let exceeded = !self.exceeded && elapsed.is_some_and(|s| s >= threshold.as_secs_f64());
            self.exceeded = false;
            return (false, exceeded, elapsed);
        }
        let started = self.since.is_none();
        let since = *self.since.get_or_insert(now);
        let exceeded = !self.exceeded && now.duration_since(since) >= threshold;
        self.exceeded |= exceeded;
        (started, exceeded, None)
    }
}

async fn compare(client: &UnleashClient, cache: &FeatureCache, token: &EdgeToken) -> Option<bool> {
    // An ambiguous legacy scope must not produce a reassuring empty-state match.
    if token.projects.is_empty() || token.environment.is_none() {
        return None;
    }
    let response = client
        .get_client_features(ClientFeaturesRequest {
            api_key: token.token.clone(),
            etag: None,
            interval: None,
        })
        .await;
    // Never apply this snapshot: synchronization remains entirely independent.
    match response {
        Ok(ClientFeaturesResponse::Updated(expected, _)) => {
            let local = cache.get(&cache_key(token)).map(|s| s.clone());
            match (
                fingerprint(&expected, token),
                local.as_ref().map(|s| fingerprint(s, token)).transpose(),
            ) {
                (Ok(expected), Ok(local)) => Some(local.is_some_and(|local| local == expected)),
                _ => None,
            }
        }
        _ => None,
    }
}

pub async fn run(
    client: Arc<UnleashClient>,
    tokens: TokenRefreshSet,
    cache: Arc<FeatureCache>,
    mut refresh_state: Receiver<RefreshState>,
) {
    let threshold = std::env::var("EDGE_STATE_CONSISTENCY_THRESHOLD_SECONDS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(30);
    let threshold = Duration::from_secs(threshold);
    for result in ["match", "mismatch", "error"] {
        CHECKS.with_label_values(&[result]);
    }
    for event in ["started", "threshold_exceeded"] {
        EPISODES.with_label_values(&[event]);
    }
    let mut observations: HashMap<String, Observation> = HashMap::new();
    loop {
        if *refresh_state.borrow_and_update() == RefreshState::Paused {
            tokio::time::sleep(Duration::from_secs(5)).await;
            continue;
        }
        let active: Vec<_> = tokens.iter().map(|t| t.token.clone()).collect();
        observations.retain(|key, _| active.iter().any(|t| &t.token == key));
        for token in active {
            let matches = compare(&client, &cache, &token).await;
            let Some(matches) = matches else {
                CHECKS.with_label_values(&["error"]).inc();
                continue;
            };
            CHECKS
                .with_label_values(&[if matches { "match" } else { "mismatch" }])
                .inc();
            let (started, exceeded, completed) = observations
                .entry(token.token.clone())
                .or_default()
                .observe(matches, Instant::now(), threshold);
            if started {
                EPISODES.with_label_values(&["started"]).inc();
            }
            if exceeded {
                EPISODES.with_label_values(&["threshold_exceeded"]).inc();
            }
            if let Some(duration) = completed {
                CONVERGENCE.observe(duration);
            }
        }
        let now = Instant::now();
        let ages: Vec<_> = observations
            .values()
            .filter_map(|o| o.since.map(|s| now.duration_since(s)))
            .collect();
        MISMATCHES.set(ages.len() as i64);
        OBSERVED.set(observations.len() as i64);
        AGE.set(ages.iter().map(Duration::as_secs).max().unwrap_or(0) as i64);
        OVER_THRESHOLD.set(ages.iter().filter(|age| **age >= threshold).count() as i64);
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(value: Value) -> ClientFeatures {
        serde_json::from_value(value).unwrap()
    }
    fn token() -> EdgeToken {
        EdgeToken {
            projects: vec!["a".into()],
            ..EdgeToken::default()
        }
    }

    #[test]
    fn deterministic_and_scoped() {
        let first = state(serde_json::json!({"version": 2, "features": [
            {"name":"z", "project":"a", "enabled":true, "strategies":[{"name":"default", "parameters":{"b":"2", "a":"1"}, "segments":[1]}]},
            {"name":"a", "project":"a", "enabled":false},
            {"name":"other", "project":"b", "enabled":true}
        ], "segments":[{"id":2,"constraints":[]},{"id":1,"constraints":[]}]}));
        let mut second = first.clone();
        second.features.reverse();
        second
            .features
            .retain(|f| f.project.as_deref() == Some("a"));
        second.segments.as_mut().unwrap().reverse();
        second.version = 99;
        assert_eq!(
            fingerprint(&first, &token()).unwrap(),
            fingerprint(&second, &token()).unwrap()
        );
        second.features[0].enabled = !second.features[0].enabled;
        assert_ne!(
            fingerprint(&first, &token()).unwrap(),
            fingerprint(&second, &token()).unwrap()
        );
    }

    #[test]
    fn mismatch_episode_and_threshold_count_once() {
        let now = Instant::now();
        let mut observation = Observation::default();
        let threshold = Duration::from_secs(30);
        assert_eq!(
            observation.observe(false, now, threshold),
            (true, false, None)
        );
        assert_eq!(
            observation.observe(false, now + threshold, threshold),
            (false, true, None)
        );
        assert_eq!(
            observation.observe(false, now + threshold, threshold),
            (false, false, None)
        );
        assert_eq!(
            observation.observe(true, now + threshold, threshold),
            (false, false, Some(30.0))
        );
        assert!(observation.since.is_none());
        observation.observe(false, now, threshold);
        assert_eq!(
            observation.observe(true, now + threshold, threshold),
            (false, true, Some(30.0))
        );
    }

    #[test]
    fn referenced_segment_changes_and_wildcard_scope_are_checked() {
        let original = state(serde_json::json!({"version":2,"features":[
            {"name":"flag","project":"a","enabled":true,"strategies":[{"name":"default","segments":[1]}]},
            {"name":"other","project":"b","enabled":true}
        ],"segments":[{"id":1,"constraints":[]}]}));
        let mut changed = original.clone();
        changed.segments = Some(
            serde_json::from_value(serde_json::json!([
                {"id":1,"constraints":[{"contextName":"userId","operator":"IN","values":["123"]}]}
            ]))
            .unwrap(),
        );
        assert_ne!(
            fingerprint(&original, &token()).unwrap(),
            fingerprint(&changed, &token()).unwrap()
        );
        let wildcard = EdgeToken {
            projects: vec!["*".into()],
            ..token()
        };
        changed = original.clone();
        changed.features[1].enabled = false;
        assert_eq!(
            fingerprint(&original, &token()).unwrap(),
            fingerprint(&changed, &token()).unwrap()
        );
        assert_ne!(
            fingerprint(&original, &wildcard).unwrap(),
            fingerprint(&changed, &wildcard).unwrap()
        );
    }

    #[tokio::test]
    async fn snapshot_propagates_expected_state_without_repairing_local_state() {
        use axum::{Json, Router, http::HeaderMap, routing::get};
        use axum_test::TestServer;
        use unleash_edge_http_client::ClientMetaInformation;
        let server = TestServer::builder().http_transport().build(Router::new().route(
            "/api/client/features", get(|headers: HeaderMap| async move {
                assert_eq!(headers["authorization"], "a:production.test");
                assert!(!headers.contains_key("if-none-match"));
                Json(serde_json::json!({"version":2,"features":[{"name":"flag","project":"a","enabled":true}]}))
            })
        ));
        let client = UnleashClient::from_url_with_backing_client(
            server.server_url("/").unwrap(),
            "Authorization".into(),
            reqwest::Client::new(),
            ClientMetaInformation {
                app_name: "consistency-test".into(),
                instance_id: ulid::Ulid::generate(),
                connection_id: ulid::Ulid::generate(),
            },
        );
        let token = EdgeToken {
            token: "a:production.test".into(),
            environment: Some("production".into()),
            ..token()
        };
        let cache = FeatureCache::default();
        let ambiguous = EdgeToken {
            projects: vec![],
            ..token.clone()
        };
        assert_eq!(compare(&client, &cache, &ambiguous).await, None);
        assert_eq!(compare(&client, &cache, &token).await, Some(false));
        assert!(cache.is_empty());
        cache.insert("production".into(), state(serde_json::json!({"version":2,"features":[{"name":"flag","project":"a","enabled":false}]})));
        assert_eq!(compare(&client, &cache, &token).await, Some(false));
        assert!(!cache.get("production").unwrap().features[0].enabled);
        cache.insert("production".into(), state(serde_json::json!({"version":2,"features":[{"name":"flag","project":"a","enabled":true}]})));
        assert_eq!(compare(&client, &cache, &token).await, Some(true));
    }
}
