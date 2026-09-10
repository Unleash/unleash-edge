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
