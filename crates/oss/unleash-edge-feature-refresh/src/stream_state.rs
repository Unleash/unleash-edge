//! Version 1 scoped SSE checkpoints. No network or cache mutation in this module.
use prometheus::{
    Histogram, IntCounterVec, IntGaugeVec, register_histogram, register_int_counter_vec,
    register_int_gauge_vec,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashSet, VecDeque},
    sync::LazyLock,
    time::{Duration, Instant},
};
use unleash_edge_feature_cache::FeatureCache;
use unleash_edge_types::tokens::{EdgeToken, cache_key};

static CHECKS: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        "edge_stream_state_checks_total",
        "Scoped checkpoint results",
        &["result"]
    )
    .unwrap()
});
static RECOVERIES: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        "edge_stream_state_recoveries_total",
        "Fresh hydration reconnects",
        &["reason"]
    )
    .unwrap()
});
static SCOPES: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register_int_gauge_vec!(
        "edge_stream_state_scopes",
        "Active monitored streams by last verification status",
        &["status"]
    )
    .unwrap()
});
static LAG: LazyLock<Histogram> = LazyLock::new(|| {
    register_histogram!(
        "edge_stream_state_lag_seconds",
        "Oldest unresolved checkpoint age sampled each second",
        vec![1.0, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0]
    )
    .unwrap()
});
static CONVERGENCE: LazyLock<Histogram> = LazyLock::new(|| {
    register_histogram!(
        "edge_stream_state_convergence_seconds",
        "Time from checkpoint receipt until its revision is applied",
        vec![0.01, 1.0, 5.0, 10.0, 30.0, 60.0, 120.0]
    )
    .unwrap()
});
static MISMATCH_DURATION: LazyLock<Histogram> = LazyLock::new(|| {
    register_histogram!(
        "edge_stream_state_mismatch_duration_seconds",
        "Observed content mismatch until a verified match, including recovery",
        vec![1.0, 10.0, 30.0, 60.0, 120.0, 300.0, 600.0]
    )
    .unwrap()
});

pub(super) fn fingerprint(cache: &FeatureCache, token: &EdgeToken) -> Option<String> {
    if token.environment.is_none() || token.projects.is_empty() {
        return None;
    }
    let state = cache.get(&cache_key(token))?;
    let mut features = Vec::new();
    let mut referenced = HashSet::new();
    for feature in &state.features {
        if !token
            .projects
            .iter()
            .any(|p| p == "*" || feature.project.as_ref() == Some(p))
        {
            continue;
        }
        for strategy in feature.strategies.iter().flatten() {
            referenced.extend(strategy.segments.iter().flatten().copied());
        }
        features.push(json!({
            "name": feature.name, "enabled": feature.enabled, "project": feature.project,
            "impressionData": feature.impression_data.unwrap_or(false),
            "strategies": feature.strategies, "variants": feature.variants,
            "dependencies": feature.dependencies,
        }));
    }
    features.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    let mut segments: Vec<_> = state
        .segments
        .iter()
        .flatten()
        .filter(|s| referenced.contains(&s.id))
        .collect();
    segments.sort_by_key(|s| s.id);
    let mut value = json!({"features": features, "segments": segments});
    normalize(&mut value);
    Some(
        Sha256::digest(serde_json::to_vec(&value).ok()?)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    )
}

// Mirrors Server stream-state.ts. Null/absent and empty arrays normalize alike;
// constraint flags default to false. Preserve nested array order and all values.
fn normalize(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.retain(|key, value| {
                !value.is_null()
                    && !value.as_array().is_some_and(Vec::is_empty)
                    && !((key == "inverted" || key == "caseInsensitive") && value == false)
            });
            map.sort_keys();
            for value in map.values_mut() {
                normalize(value);
            }
        }
        Value::Array(values) => values.iter_mut().for_each(normalize),
        _ => {}
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Checkpoint {
    version: u32,
    revision: u64,
    state_hash: String,
}

struct Pending {
    checkpoint: Checkpoint,
    received: Instant,
}

pub(super) struct Verification {
    position: Option<u64>,
    pending: VecDeque<Pending>,
    mismatch: bool,
    mismatch_since: Option<Instant>,
    last_checkpoint: Option<Instant>,
    last_recovery: Option<Instant>,
    threshold: Duration,
    status: &'static str,
}

impl Verification {
    pub(super) fn new(threshold: Duration) -> Self {
        for result in [
            "match",
            "mismatch",
            "behind",
            "stale",
            "invalid",
            "unsupported",
        ] {
            CHECKS.with_label_values(&[result]);
        }
        for reason in ["mismatch", "lag"] {
            RECOVERIES.with_label_values(&[reason]);
        }
        for status in ["unknown", "matched", "lagging", "mismatching"] {
            SCOPES.with_label_values(&[status]);
        }
        SCOPES.with_label_values(&["unknown"]).inc();
        Self {
            position: None,
            pending: VecDeque::new(),
            mismatch: false,
            mismatch_since: None,
            last_checkpoint: None,
            last_recovery: None,
            threshold,
            status: "unknown",
        }
    }

    fn status(&mut self, status: &'static str) {
        if self.status != status {
            SCOPES.with_label_values(&[self.status]).dec();
            SCOPES.with_label_values(&[status]).inc();
            self.status = status;
        }
    }

    pub(super) fn checkpoint(
        &mut self,
        data: &str,
        now: Instant,
        hash: impl FnMut() -> Option<String>,
    ) {
        let Ok(checkpoint) = serde_json::from_str::<Checkpoint>(data) else {
            CHECKS.with_label_values(&["invalid"]).inc();
            return;
        };
        if checkpoint.version != 1 {
            CHECKS.with_label_values(&["unsupported"]).inc();
            return;
        }
        if checkpoint.state_hash.len() != 64
            || !checkpoint.state_hash.bytes().all(|b| b.is_ascii_hexdigit())
        {
            CHECKS.with_label_values(&["invalid"]).inc();
            return;
        }
        self.last_checkpoint = Some(now);
        if self.position.is_some_and(|p| p > checkpoint.revision) {
            CHECKS.with_label_values(&["stale"]).inc();
            return;
        }
        // Repeated heartbeats for the same target must not restart its timer.
        if !self
            .pending
            .iter()
            .any(|p| p.checkpoint.revision == checkpoint.revision)
        {
            // Bound memory while retaining the oldest unresolved target.
            if self.pending.len() == 64 {
                self.pending.remove(1);
            }
            self.pending.push_back(Pending {
                checkpoint,
                received: now,
            });
        }
        self.evaluate(now, hash);
        if !self.pending.is_empty() {
            CHECKS.with_label_values(&["behind"]).inc();
            self.status("lagging");
        }
    }

    pub(super) fn applied(&mut self, id: &str, now: Instant, hash: impl FnMut() -> Option<String>) {
        if let Ok(position) = id.parse() {
            self.position = Some(position);
            self.evaluate(now, hash);
        }
    }

    fn evaluate(&mut self, now: Instant, mut hash: impl FnMut() -> Option<String>) {
        let Some(position) = self.position else {
            return;
        };
        while self
            .pending
            .front()
            .is_some_and(|p| p.checkpoint.revision <= position)
        {
            let pending = self.pending.pop_front().unwrap();
            CONVERGENCE.observe(now.duration_since(pending.received).as_secs_f64());
            if pending.checkpoint.revision == position {
                if let Some(local) = hash() {
                    self.mismatch = local != pending.checkpoint.state_hash;
                    if self.mismatch {
                        self.mismatch_since.get_or_insert(now);
                    } else if let Some(since) = self.mismatch_since.take() {
                        MISMATCH_DURATION.observe(now.duration_since(since).as_secs_f64());
                    }
                    let result = if self.mismatch { "mismatch" } else { "match" };
                    CHECKS.with_label_values(&[result]).inc();
                    self.status(if self.mismatch {
                        "mismatching"
                    } else {
                        "matched"
                    });
                } else {
                    CHECKS.with_label_values(&["invalid"]).inc();
                    self.status("unknown");
                }
            } else {
                // An older fingerprint must never be compared to newer contents.
                self.status("unknown");
            }
        }
    }

    pub(super) fn recovery(&mut self, now: Instant) -> Option<&'static str> {
        if !self.pending.is_empty() && !self.mismatch {
            self.status("lagging");
        }
        let age = self.pending.front().map(|p| now.duration_since(p.received));
        if let Some(age) = age {
            LAG.observe(age.as_secs_f64());
        }
        if self
            .last_checkpoint
            .is_some_and(|t| now.duration_since(t) > Duration::from_secs(300))
        {
            self.status("unknown");
        }
        let reason = if self.mismatch {
            "mismatch"
        } else if age.is_some_and(|age| age >= self.threshold) {
            "lag"
        } else {
            return None;
        };
        // Persistent faults or incompatible data must not create a reconnect storm.
        if self
            .last_recovery
            .is_some_and(|t| now.duration_since(t) < Duration::from_secs(60))
        {
            return None;
        }
        RECOVERIES.with_label_values(&[reason]).inc();
        self.last_recovery = Some(now);
        self.disconnected();
        Some(reason)
    }

    pub(super) fn disconnected(&mut self) {
        self.pending.clear();
        self.position = None;
        self.mismatch = false;
        self.last_checkpoint = None;
        self.status("unknown");
    }
}

impl Drop for Verification {
    fn drop(&mut self) {
        SCOPES.with_label_values(&[self.status]).dec();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use unleash_types::client_features::ClientFeatures;

    fn checkpoint(revision: u64) -> String {
        json!({"version":1,"revision":revision,"stateHash":"a".repeat(64)}).to_string()
    }

    #[test]
    fn golden_hash_matches_server_after_typed_roundtrip_and_scoping() {
        let fixture: Value = serde_json::from_str(include_str!("stream-state-v1.json")).unwrap();
        let mut payload = fixture.clone();
        payload["version"] = json!(2);
        let cache = FeatureCache::default();
        cache.insert(
            "production".into(),
            serde_json::from_value::<ClientFeatures>(payload).unwrap(),
        );
        let mut token = EdgeToken {
            environment: Some("production".into()),
            projects: vec!["*".into()],
            ..Default::default()
        };
        assert_eq!(
            fingerprint(&cache, &token).unwrap(),
            "1078c59ef139d4f1549ba15058cd168bf81746168cff81cc2d9b68b031217147"
        );
        token.projects = vec!["a".into()];
        let before = fingerprint(&cache, &token);
        let mut scoped: ClientFeatures = cache.get("production").unwrap().clone();
        scoped.features.reverse();
        scoped
            .features
            .retain(|f| f.project.as_deref() == Some("a"));
        scoped.segments.as_mut().unwrap().reverse();
        cache.insert("production".into(), scoped);
        assert_eq!(fingerprint(&cache, &token), before);
        let mut changed = cache.get("production").unwrap().clone();
        changed
            .segments
            .as_mut()
            .unwrap()
            .iter_mut()
            .find(|s| s.id == 7)
            .unwrap()
            .constraints
            .clear();
        cache.insert("production".into(), changed);
        assert_ne!(fingerprint(&cache, &token), before);
    }

    #[test]
    fn moving_targets_retire_satisfied_checkpoints_without_resetting_remaining_lag() {
        let now = Instant::now();
        let mut state = Verification::new(Duration::from_secs(30));
        state.applied("1", now, || None);
        state.checkpoint(&checkpoint(2), now, || None);
        state.checkpoint(&checkpoint(3), now + Duration::from_secs(10), || None);
        state.applied("2", now + Duration::from_secs(15), || Some("a".repeat(64)));
        state.checkpoint(&checkpoint(4), now + Duration::from_secs(20), || None);
        state.checkpoint(&checkpoint(3), now + Duration::from_secs(25), || None);
        assert_eq!(state.recovery(now + Duration::from_secs(39)), None);
        assert_eq!(state.recovery(now + Duration::from_secs(40)), Some("lag"));
        assert!(state.pending.is_empty());
    }

    #[test]
    fn confirmed_mismatch_recovers_and_is_rate_limited() {
        let now = Instant::now();
        let mut state = Verification::new(Duration::from_secs(30));
        state.applied("2", now, || None);
        state.checkpoint(&checkpoint(2), now, || Some("b".repeat(64)));
        assert_eq!(state.recovery(now), Some("mismatch"));
        assert_eq!(state.mismatch_since, Some(now));
        state.applied("3", now, || None);
        state.checkpoint(&checkpoint(3), now, || Some("b".repeat(64)));
        assert_eq!(state.recovery(now + Duration::from_secs(59)), None);
        assert_eq!(
            state.recovery(now + Duration::from_secs(60)),
            Some("mismatch")
        );
        state.applied("4", now + Duration::from_secs(61), || None);
        state.checkpoint(&checkpoint(4), now + Duration::from_secs(61), || {
            Some("a".repeat(64))
        });
        assert_eq!(state.mismatch_since, None);
    }

    #[test]
    fn older_fingerprints_and_unsupported_or_invalid_checkpoints_do_not_trigger_recovery() {
        let now = Instant::now();
        let mut state = Verification::new(Duration::from_secs(30));
        state.applied("3", now, || None);
        state.checkpoint(&checkpoint(2), now, || {
            panic!("must not compare older contents")
        });
        state.checkpoint("{}", now, || None);
        state.checkpoint(
            &checkpoint(4).replace("\"version\":1", "\"version\":2"),
            now,
            || None,
        );
        assert_eq!(state.recovery(now + Duration::from_secs(120)), None);
        state.checkpoint(&checkpoint(3), now, || Some("a".repeat(64)));
        assert_eq!(state.status, "matched");
        assert_eq!(state.recovery(now + Duration::from_secs(301)), None);
        assert_eq!(state.status, "unknown");
    }
}
