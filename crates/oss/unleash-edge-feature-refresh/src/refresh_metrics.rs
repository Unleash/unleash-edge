use prometheus::{IntCounterVec, IntGaugeVec, register_int_counter_vec, register_int_gauge_vec};
use std::sync::LazyLock;

const ENVIRONMENT_LABEL: &str = "environment";
const SOURCE_LABEL: &str = "source";
const KIND_LABEL: &str = "kind";

/// Feature state restored from persistent storage during startup.
pub const HYDRATION_SOURCE: &str = "hydration";
/// Feature state applied from delta polling or upstream SSE events, including hydration events.
pub const DELTA_SOURCE: &str = "delta";
/// Feature state applied from a full polling response.
pub const FULL_SOURCE: &str = "full";
/// Feature state loaded or reloaded from an offline bootstrap file.
pub const OFFLINE_SOURCE: &str = "offline";

/// Expose zero-valued counters before refresh work starts. Repeated calls preserve counts.
pub fn initialize_feature_refresh_metrics(environment: &str) {
    for source in [HYDRATION_SOURCE, DELTA_SOURCE, FULL_SOURCE, OFFLINE_SOURCE] {
        FEATURE_STATE_WARNINGS.with_label_values(&[environment, source]);
    }
    for kind in ["parse", "stream", "fetch"] {
        FEATURE_REFRESH_ERRORS.with_label_values(&[environment, kind]);
    }
}

static FEATURE_STATE_WARNINGS: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        "edge_feature_state_warnings_total",
        "Total number of feature state compile warnings that caused toggles to be discarded",
        &[ENVIRONMENT_LABEL, SOURCE_LABEL]
    )
    .expect("failed to register edge_feature_state_warnings_total")
});

static FEATURE_REFRESH_ERRORS: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        "edge_feature_refresh_errors_total",
        "Total number of background feature refresh errors",
        &[ENVIRONMENT_LABEL, KIND_LABEL]
    )
    .expect("failed to register edge_feature_refresh_errors_total")
});

static LAST_APPLIED_REVISION_ID: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register_int_gauge_vec!(
        "edge_last_applied_revision_id",
        "Last feature revision ID successfully applied by Edge",
        &[ENVIRONMENT_LABEL]
    )
    .expect("failed to register edge_last_applied_revision_id")
});

pub fn observe_feature_state_warnings(environment: &str, source: &str, count: usize) {
    if count > 0 {
        FEATURE_STATE_WARNINGS
            .with_label_values(&[environment, source])
            .inc_by(count as u64);
    }
}

pub fn observe_feature_refresh_error(environment: &str, kind: &str) {
    FEATURE_REFRESH_ERRORS
        .with_label_values(&[environment, kind])
        .inc();
}

pub fn observe_last_applied_revision_id(environment: &str, revision_id: usize) {
    LAST_APPLIED_REVISION_ID
        .with_label_values(&[environment])
        .set(i64::try_from(revision_id).unwrap_or(i64::MAX));
}

pub fn feature_state_warnings_total(environment: &str, source: &str) -> u64 {
    FEATURE_STATE_WARNINGS
        .with_label_values(&[environment, source])
        .get()
}

#[cfg(test)]
pub(crate) fn last_applied_revision_id(environment: &str) -> i64 {
    LAST_APPLIED_REVISION_ID
        .with_label_values(&[environment])
        .get()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialization_exposes_zero_counters_without_resetting_them() {
        let environment = "metrics-initialization-test";
        initialize_feature_refresh_metrics(environment);
        // Gather like /metrics: reading a counter through with_label_values would
        // create the missing series and mask a broken initialization path.
        let gathered = prometheus::gather();
        for (name, expected_series) in [
            ("edge_feature_state_warnings_total", 4),
            ("edge_feature_refresh_errors_total", 3),
        ] {
            let family = gathered.iter().find(|m| m.name() == name).unwrap();
            let series: Vec<_> = family
                .get_metric()
                .iter()
                .filter(|m| {
                    m.get_label()
                        .iter()
                        .any(|l| l.name() == ENVIRONMENT_LABEL && l.value() == environment)
                })
                .collect();
            assert_eq!(series.len(), expected_series);
            assert!(series.iter().all(|m| m.get_counter().value() == 0.0));
        }
        observe_feature_state_warnings(environment, DELTA_SOURCE, 2);
        observe_feature_refresh_error(environment, "stream");
        initialize_feature_refresh_metrics(environment);
        assert_eq!(feature_state_warnings_total(environment, DELTA_SOURCE), 2);
        assert_eq!(
            FEATURE_REFRESH_ERRORS
                .with_label_values(&[environment, "stream"])
                .get(),
            1
        );
    }
}
