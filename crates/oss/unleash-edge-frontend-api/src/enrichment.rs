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
//! Handled (runtime-configurable, all off by default so the naive path is measurable):
//! - Caching: identity-keyed TTL cache (`EDGE_CONTEXT_ENRICHER_CACHE_TTL_MS`).
//! - Bulkhead: reject-and-fail-open past `EDGE_CONTEXT_ENRICHER_MAX_CONCURRENCY`.
//! - Response size cap: `EDGE_CONTEXT_ENRICHER_MAX_RESPONSE_BYTES` (default 64 KiB).
//! - Skip: requests without a userId never call the enricher.
//! - Outcome counter `context_enrichment_total{outcome}` = the fail-open signal.
//!
//! Edge cases to revisit before this is production (intentionally NOT solved here):
//! - No circuit breaker / auto-disable when the dependency keeps missing its SLA.
//! - No response *schema* validation or type coercion beyond the byte cap.
//! - Cache is keyed on userId only; enrichers keying on another field are out of scope.
//! - Determinism: a non-deterministic enricher can flip gradual-rollout stickiness
//!   across the cache TTL boundary.
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
    use std::sync::Arc;
    use std::sync::LazyLock;
    use std::time::{Duration, Instant};

    use dashmap::DashMap;
    use prometheus::{
        HistogramVec, IntCounterVec, register_histogram_vec, register_int_counter_vec,
    };
    use reqwest::Client;
    use tokio::sync::Semaphore;
    use unleash_types::client_features::Context;

    const ENRICHER_URL_ENV: &str = "EDGE_CONTEXT_ENRICHER_URL";
    const ENRICHER_TIMEOUT_MS_ENV: &str = "EDGE_CONTEXT_ENRICHER_TIMEOUT_MS";
    const ENRICHER_CACHE_TTL_MS_ENV: &str = "EDGE_CONTEXT_ENRICHER_CACHE_TTL_MS";
    const ENRICHER_MAX_CONCURRENCY_ENV: &str = "EDGE_CONTEXT_ENRICHER_MAX_CONCURRENCY";
    const ENRICHER_MAX_RESPONSE_BYTES_ENV: &str = "EDGE_CONTEXT_ENRICHER_MAX_RESPONSE_BYTES";
    const DEFAULT_TIMEOUT_MS: u64 = 100;
    const DEFAULT_MAX_RESPONSE_BYTES: usize = 64 * 1024;

    /// Why a fetch failed. All variants fail open; the distinction is for logs.
    #[derive(Debug)]
    enum EnrichError {
        Http(reqwest::Error),
        /// Response exceeded `EDGE_CONTEXT_ENRICHER_MAX_RESPONSE_BYTES`.
        TooLarge(usize),
        Decode(serde_json::Error),
    }

    static ENRICHMENT_DURATION: LazyLock<HistogramVec> = LazyLock::new(|| {
        register_histogram_vec!(
            "context_enrichment_duration_seconds",
            "Time spent calling the configured HTTP enricher (fetches only). \
            Kept separate from flag-evaluation latency so it does not skew Edge's SLA metrics.",
            &["outcome"]
        )
        .unwrap()
    });

    /// Every enrichment attempt, by outcome: `ok`, `error` (fail-open after a
    /// failed fetch), `cache_hit`, or `rejected` (bulkhead full → fail-open).
    /// This is the signal you page on — it says how often a flag decision was
    /// made on degraded (un-enriched) data.
    static ENRICHMENT_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
        register_int_counter_vec!(
            "context_enrichment_total",
            "Count of context enrichment attempts by outcome.",
            &["outcome"]
        )
        .unwrap()
    });

    static ENRICHER: LazyLock<Option<ContextEnricher>> = LazyLock::new(ContextEnricher::from_env);

    fn read_u64(name: &str) -> Option<u64> {
        std::env::var(name).ok().and_then(|value| value.parse().ok())
    }

    fn record(outcome: &str) {
        ENRICHMENT_TOTAL.with_label_values(&[outcome]).inc();
    }

    /// Identity-keyed TTL cache of enricher responses. Trades a bounded amount of
    /// freshness for load relief: repeated lookups of the same subject within the
    /// TTL skip the network hop and also stabilise gradual-rollout stickiness.
    struct PropertyCache {
        ttl: Duration,
        entries: DashMap<String, (Instant, HashMap<String, String>)>,
    }

    impl PropertyCache {
        fn get(&self, key: &str) -> Option<HashMap<String, String>> {
            let entry = self.entries.get(key)?;
            let (stored_at, properties) = entry.value();
            (stored_at.elapsed() <= self.ttl).then(|| properties.clone())
        }

        fn insert(&self, key: String, properties: HashMap<String, String>) {
            self.entries.insert(key, (Instant::now(), properties));
        }
    }

    struct ContextEnricher {
        url: String,
        timeout: Duration,
        client: Client,
        /// `Some` when `EDGE_CONTEXT_ENRICHER_CACHE_TTL_MS` > 0.
        cache: Option<PropertyCache>,
        /// `Some` when `EDGE_CONTEXT_ENRICHER_MAX_CONCURRENCY` > 0.
        semaphore: Option<Arc<Semaphore>>,
        /// Hard cap on the enricher response body; a hostile/buggy enricher
        /// cannot make Edge buffer or merge an unbounded map.
        max_response_bytes: usize,
    }

    impl ContextEnricher {
        fn from_env() -> Option<Self> {
            let url = std::env::var(ENRICHER_URL_ENV)
                .ok()
                .filter(|url| !url.is_empty())?;
            let timeout_ms = read_u64(ENRICHER_TIMEOUT_MS_ENV).unwrap_or(DEFAULT_TIMEOUT_MS);
            let cache = read_u64(ENRICHER_CACHE_TTL_MS_ENV)
                .filter(|ttl| *ttl > 0)
                .map(|ttl| PropertyCache {
                    ttl: Duration::from_millis(ttl),
                    entries: DashMap::new(),
                });
            let semaphore = read_u64(ENRICHER_MAX_CONCURRENCY_ENV)
                .filter(|limit| *limit > 0)
                .map(|limit| Arc::new(Semaphore::new(limit as usize)));
            let max_response_bytes = read_u64(ENRICHER_MAX_RESPONSE_BYTES_ENV)
                .map(|bytes| bytes as usize)
                .unwrap_or(DEFAULT_MAX_RESPONSE_BYTES);
            tracing::info!(
                url = %url,
                timeout_ms,
                cache_ttl_ms = cache.as_ref().map(|cache| cache.ttl.as_millis()),
                max_concurrency = semaphore.as_ref().map(|semaphore| semaphore.available_permits()),
                max_response_bytes,
                "Context enrichment enabled (hot-path POC)"
            );
            Some(Self {
                url,
                timeout: Duration::from_millis(timeout_ms),
                client: Client::new(),
                cache,
                semaphore,
                max_response_bytes,
            })
        }

        async fn fetch(&self, context: &Context) -> Result<HashMap<String, String>, EnrichError> {
            let bytes = self
                .client
                .post(&self.url)
                .timeout(self.timeout)
                .json(context)
                .send()
                .await
                .map_err(EnrichError::Http)?
                .error_for_status()
                .map_err(EnrichError::Http)?
                .bytes()
                .await
                .map_err(EnrichError::Http)?;
            if bytes.len() > self.max_response_bytes {
                return Err(EnrichError::TooLarge(bytes.len()));
            }
            serde_json::from_slice(&bytes).map_err(EnrichError::Decode)
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
    /// unchanged. Always fail-open. Order: cache → bulkhead → fetch. Records the
    /// outcome (always) and fetch latency (fetches only).
    pub async fn enrich_context(context: Context) -> Context {
        let Some(enricher) = ENRICHER.as_ref() else {
            return context;
        };

        // Enrichment is keyed on the user identity (like the cold-path POC). With
        // no userId there is nothing to look up, so skip the network hop entirely
        // rather than pay a round-trip for a guaranteed-empty answer. Enrichers
        // that key on a different field are out of scope for this POC.
        let Some(cache_key) = context.user_id.clone() else {
            record("skipped");
            return context;
        };

        // 1. Cache: repeated identity within the TTL skips the network hop entirely.
        if let Some(cache) = enricher.cache.as_ref() {
            if let Some(properties) = cache.get(&cache_key) {
                record("cache_hit");
                return merge_properties(context, properties);
            }
        }

        // 2. Bulkhead: if the enricher is saturated, fail open immediately instead
        //    of piling up in-flight requests on Edge's hottest path. The permit is
        //    held for the duration of the fetch below.
        let _permit = match enricher.semaphore.as_ref() {
            Some(semaphore) => match semaphore.clone().try_acquire_owned() {
                Ok(permit) => Some(permit),
                Err(_) => {
                    record("rejected");
                    tracing::warn!("Context enrichment bulkhead full; using un-enriched context");
                    return context;
                }
            },
            None => None,
        };

        // 3. Fetch (timed).
        let start = Instant::now();
        let result = enricher.fetch(&context).await;
        let outcome = if result.is_ok() { "ok" } else { "error" };
        ENRICHMENT_DURATION
            .with_label_values(&[outcome])
            .observe(start.elapsed().as_secs_f64());
        record(outcome);

        match result {
            Ok(extra) => {
                if let Some(cache) = enricher.cache.as_ref() {
                    cache.insert(cache_key, extra.clone());
                }
                merge_properties(context, extra)
            }
            Err(error) => {
                tracing::warn!(?error, "Context enrichment failed; using un-enriched context");
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

        #[test]
        fn cache_serves_fresh_entries_and_drops_stale_ones() {
            let fresh = PropertyCache {
                ttl: Duration::from_secs(60),
                entries: DashMap::new(),
            };
            fresh.insert("u1".into(), HashMap::from([("tier".into(), "gold".into())]));
            assert_eq!(fresh.get("u1").unwrap().get("tier").unwrap(), "gold");
            assert!(fresh.get("absent").is_none());

            // A zero TTL means any elapsed time is already stale -> treated as a miss.
            let stale = PropertyCache {
                ttl: Duration::from_millis(0),
                entries: DashMap::new(),
            };
            stale.insert("u1".into(), HashMap::from([("tier".into(), "gold".into())]));
            assert!(stale.get("u1").is_none());
        }
    }
}
