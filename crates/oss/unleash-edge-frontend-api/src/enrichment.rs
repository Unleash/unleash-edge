//! Hot-path context enrichment (self-hosted POC ala FAFO).
//!
//! Enriches the incoming evaluation `Context` with trusted, server-side
//! attributes by calling a user-configured HTTP endpoint **per request**, right
//! before flag evaluation.
//!
//! behind the `context-enrichment` feature glag (off by default). When
//! the feature is disabled, none of the enrichment machinery — HTTP client,
//! Prometheus metric, per-request call — is compiled in; `enrich_context` is a
//! zero-cost identity. When the feature is enabled, enrichment still only
//! activates at runtime if `EDGE_CONTEXT_ENRICHER_URL` is set. Two switches:
//! compile-time (ship it or not) and runtime (turn it on or not).
//!
//! Contract (POC): Edge POSTs the (camelCase) `Context` JSON to the endpoint and
//! expects back a flat `{ "key": "value" }` object of properties to inject. The
//! enricher is trusted, so its values override anything the client sent.
//!
//! POC decisions:
//! - Config is read from ENV (self-hosted only) — no Unleash-served config yet.
//! - **Fail-open**: any error/timeout evaluates on the un-enriched context.
//! - Enrichment latency is recorded in a **separate** metric
//!   (`context_enrichment_duration_seconds`) so hot-enricher time does not
//!   contaminate Edge's core evaluation SLA / dashboards. This is the whole
//!   reason we can offer hot-path without lying about Edge's latency story.
//!
//! Edge cases to revisit before this is production (intentionally NOT solved here):
//! - No caching: every request hits the enricher — this *is* the hot-path cost.
//! - No circuit breaker / auto-disable when the dependency keeps missing its SLA.
//! - No response schema validation, size cap, or type coercion — a huge/hostile
//!   response is merged as-is.
//! - Determinism: a non-deterministic enricher can flip gradual-rollout stickiness.
//! - Leak boundary: enriched (possibly secret) properties feed evaluation only —
//!   confirm they never escape to the client response, metrics, or impression data.
//! - SSRF is limited (operator-configured URL, not client-controlled), but the
//!   request body echoes the client context to that endpoint.
//! - Only `properties` are merged; overriding top-level fields (userId, etc.),
//!   per-request auth to the enricher, and multi-key joins are out of scope.

#[cfg(feature = "context-enrichment")]
pub use enabled::enrich_context;

/// Identity fallback compiled when the `context-enrichment` feature is off.
/// No HTTP client, no metric, no dependency pulled in — the call sites keep the
/// same `enrich_context(context).await` shape and this optimizes away.
#[cfg(not(feature = "context-enrichment"))]
pub async fn enrich_context(
    context: unleash_types::client_features::Context,
) -> unleash_types::client_features::Context {
    context
}

#[cfg(feature = "context-enrichment")]
mod enabled {
    use std::collections::HashMap;
    use std::sync::LazyLock;
    use std::time::{Duration, Instant};

    use prometheus::{HistogramVec, register_histogram_vec};
    use reqwest::Client;
    use unleash_types::client_features::Context;

    const ENRICHER_URL_ENV: &str = "EDGE_CONTEXT_ENRICHER_URL";
    const ENRICHER_TIMEOUT_MS_ENV: &str = "EDGE_CONTEXT_ENRICHER_TIMEOUT_MS";
    const DEFAULT_TIMEOUT_MS: u64 = 100;

    static ENRICHMENT_DURATION: LazyLock<HistogramVec> = LazyLock::new(|| {
        register_histogram_vec!(
            "context_enrichment_duration_seconds",
            "Time spent enriching the evaluation context via the configured HTTP enricher. \
            Kept separate from flag-evaluation latency so it does not skew Edge's SLA metrics.",
            &["outcome"]
        )
        .unwrap()
    });

    static ENRICHER: LazyLock<Option<ContextEnricher>> = LazyLock::new(ContextEnricher::from_env);

    struct ContextEnricher {
        url: String,
        timeout: Duration,
        client: Client,
    }

    impl ContextEnricher {
        fn from_env() -> Option<Self> {
            let url = std::env::var(ENRICHER_URL_ENV)
                .ok()
                .filter(|url| !url.is_empty())?;
            let timeout_ms = std::env::var(ENRICHER_TIMEOUT_MS_ENV)
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(DEFAULT_TIMEOUT_MS);
            tracing::info!(
                url = %url,
                timeout_ms,
                "Context enrichment enabled (hot-path POC)"
            );
            Some(Self {
                url,
                timeout: Duration::from_millis(timeout_ms),
                client: Client::new(),
            })
        }

        async fn fetch(&self, context: &Context) -> reqwest::Result<HashMap<String, String>> {
            self.client
                .post(&self.url)
                .timeout(self.timeout)
                .json(context)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await
        }
    }

    /// Overlays enricher-provided properties onto the context. The enricher is
    /// trusted, so its values win over anything the (untrusted) client sent.
    fn merge_properties(mut context: Context, extra: HashMap<String, String>) -> Context {
        if extra.is_empty() {
            return context;
        }
        let mut properties = context.properties.take().unwrap_or_default();
        properties.extend(extra);
        context.properties = Some(properties);
        context
    }

    /// Enriches the context when an enricher is configured, otherwise returns it
    /// unchanged. Always fail-open: any error/timeout evaluates on the original
    /// context. Records enrichment latency in a dedicated metric.
    pub async fn enrich_context(context: Context) -> Context {
        let Some(enricher) = ENRICHER.as_ref() else {
            return context;
        };

        let start = Instant::now();
        let result = enricher.fetch(&context).await;
        let outcome = if result.is_ok() { "ok" } else { "error" };
        ENRICHMENT_DURATION
            .with_label_values(&[outcome])
            .observe(start.elapsed().as_secs_f64());

        match result {
            Ok(extra) => merge_properties(context, extra),
            Err(error) => {
                tracing::warn!(%error, "Context enrichment failed; using un-enriched context");
                context
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn context() -> Context {
            Context {
                user_id: Some("7".into()),
                session_id: None,
                environment: None,
                app_name: None,
                current_time: None,
                remote_address: None,
                properties: Some(HashMap::from([
                    ("group".into(), "placeholder".into()),
                    ("keep".into(), "me".into()),
                ])),
            }
        }

        #[test]
        fn enricher_values_win_and_new_keys_are_added() {
            let enriched = merge_properties(
                context(),
                HashMap::from([
                    ("group".into(), "trusted-group-77".into()),
                    ("tier".into(), "gold".into()),
                ]),
            );

            let properties = enriched.properties.unwrap();
            assert_eq!(properties.get("group").unwrap(), "trusted-group-77"); // overridden
            assert_eq!(properties.get("tier").unwrap(), "gold"); // added
            assert_eq!(properties.get("keep").unwrap(), "me"); // preserved
        }

        #[test]
        fn empty_enrichment_leaves_context_untouched() {
            let before = context();
            let after = merge_properties(before.clone(), HashMap::new());
            assert_eq!(after.properties, before.properties);
        }

        #[test]
        fn merges_into_context_without_existing_properties() {
            let mut context = context();
            context.properties = None;

            let enriched =
                merge_properties(context, HashMap::from([("tier".into(), "gold".into())]));

            assert_eq!(enriched.properties.unwrap().get("tier").unwrap(), "gold");
        }
    }
}
