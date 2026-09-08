use std::sync::LazyLock;
use std::time::Duration;

use prometheus::{Histogram, IntCounter, register_histogram, register_int_counter};

pub(crate) static CONTEXT_ENRICHER_REQUESTS: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(
        "context_enricher_requests_total",
        "Total number of context enrichment requests"
    )
    .unwrap()
});

pub(crate) static CONTEXT_ENRICHER_ERRORS: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(
        "context_enricher_errors_total",
        "Total number of failed context enrichment requests"
    )
    .unwrap()
});

pub(crate) static CONTEXT_ENRICHER_TIMEOUTS: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter!(
        "context_enricher_timeouts_total",
        "Total number of context enrichment requests that timed out"
    )
    .unwrap()
});

pub(crate) static CONTEXT_ENRICHER_DURATION_MILLISECONDS: LazyLock<Histogram> =
    LazyLock::new(|| {
        register_histogram!(
            "context_enricher_duration_milliseconds",
            "Context enrichment request duration in milliseconds",
            vec![0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 25.0, 50.0, 75.0, 100.0,]
        )
        .unwrap()
    });

pub(crate) fn record_enrichment(duration: Duration) {
    CONTEXT_ENRICHER_REQUESTS.inc();
    CONTEXT_ENRICHER_DURATION_MILLISECONDS.observe(duration.as_secs_f64() * 1000.0);
}

pub(crate) fn record_timeout(duration: Duration) {
    CONTEXT_ENRICHER_REQUESTS.inc();
    CONTEXT_ENRICHER_TIMEOUTS.inc();
    CONTEXT_ENRICHER_DURATION_MILLISECONDS.observe(duration.as_secs_f64() * 1000.0);
}

pub(crate) fn record_error(duration: Duration) {
    CONTEXT_ENRICHER_REQUESTS.inc();
    CONTEXT_ENRICHER_ERRORS.inc();
    CONTEXT_ENRICHER_DURATION_MILLISECONDS.observe(duration.as_secs_f64() * 1000.0);
}
