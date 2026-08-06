## Contract (POC)

Edge POSTs the (camelCase) `Context` JSON to the endpoint and
expects back a flat `{ "key": "value" }` object of properties to inject. The
enricher is trusted, so its values override anything the client sent.

### POC decisions

- Config is read from ENV (self-hosted only) — no Unleash-served config yet.
- **Fail-open**: any error/timeout evaluates on the un-enriched context.
- Enrichment latency is recorded in a **separate** metric
(`context_enrichment_duration_seconds`) so hot-enricher time does not
contaminate Edge's core evaluation SLA / dashboards. This is the whole
reason we can offer hot-path without lying about Edge's latency story.

### Handled (runtime-configurable, all off by default so the naive path is measurable)

- Caching: identity-keyed TTL cache (`EDGE_CONTEXT_ENRICHER_CACHE_TTL_MS`).
- Bulkhead: reject-and-fail-open past `EDGE_CONTEXT_ENRICHER_MAX_CONCURRENCY`.
- Response size cap: `EDGE_CONTEXT_ENRICHER_MAX_RESPONSE_BYTES` (default 64 KiB).
- Skip: requests without a userId never call the enricher.
- Outcome counter `context_enrichment_total{outcome}` = the fail-open signal.

### Edge cases to revisit

- No circuit breaker / auto-disable when the dependency keeps missing its SLA.
- No response *schema* validation or type coercion beyond the byte cap.
- Cache is keyed on userId only; enrichers keying on another field are out of scope.
- Determinism: a non-deterministic enricher can flip gradual-rollout stickiness
across the cache TTL boundary.
- Leak boundary: enriched (possibly secret) properties feed evaluation only —
confirm they never escape to the client response, metrics, or impression data.
- SSRF is limited (operator-configured URL, not client-controlled), but the
request body echoes the client context to that endpoint.
- Only `properties` are merged; overriding top-level fields (userId, etc.),
per-request auth to the enricher, and multi-key joins are out of scope.
