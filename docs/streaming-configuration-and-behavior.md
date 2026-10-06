# Streaming Configuration and Behavior

This document explains how `STREAMING` and `DELTA` currently affect Edge behavior.

It focuses on:
- which hydrator is selected
- which upstream endpoints are used for bootstrap and updates
- readiness implications
- known coupling/risk points

## Scope

Behavior described here reflects current implementation in this repository (as of 2026-03-10).

## Key point

`STREAMING` controls hydrator selection.

`DELTA` does **not** currently select a delta-polling hydrator when `STREAMING=false`.

Hydrator selection is effectively binary:
- `STREAMING=true` => always `DeltaRefresher` (no further hydrator choice)
- `STREAMING=false` => always `FeatureRefresher` (currently unaffected by `DELTA`)

## Configuration matrix

### `STREAMING=false`, `DELTA=false`

- Hydrator: polling (`FeatureRefresher`)
- Bootstrap hydration: upstream `GET /api/client/features`
- Ongoing refresh: periodic polling to upstream `GET /api/client/features`
- Upstream SSE: not used

### `STREAMING=true`, `DELTA=false`

- Hydrator: streaming (`DeltaRefresher`)
- Bootstrap hydration: upstream `GET /api/client/delta`
- Ongoing refresh: upstream SSE on `GET /api/client/streaming`
- Note: there is still an initial delta fetch before stream task startup

### `STREAMING=true`, `DELTA=true`

- Hydrator: streaming (`DeltaRefresher`)
- Bootstrap hydration: upstream `GET /api/client/delta`
- Ongoing refresh: upstream SSE on `GET /api/client/streaming`
- In practice this behaves the same as `STREAMING=true`, `DELTA=false` for hydrator/refresh path

### `STREAMING=false`, `DELTA=true`

- Hydrator: polling (`FeatureRefresher`)
- Bootstrap hydration: upstream `GET /api/client/features`
- Ongoing refresh: periodic polling to upstream `GET /api/client/features`
- Important: this does **not** currently switch to delta polling (`/api/client/delta`)

## Why streaming still calls delta first

In streaming mode, Edge does a synchronous bootstrap hydration before background SSE tasks are started.

That bootstrap uses delta (`/api/client/delta`) and populates feature cache/engine state via delta events.

Likely reasons this exists:
- deterministic startup hydration
- simpler readiness semantics
- unified update path for delta payload handling

## Readiness impact

Current ready check returns ready when:
- token cache is not empty, and
- feature cache is not empty

In streaming mode, the initial delta bootstrap helps satisfy readiness without waiting for asynchronous SSE event timing.

Without bootstrap, readiness could remain not ready until the first stream event arrives.

## Hidden coupling / risk

With `STREAMING=true`, Edge currently depends on upstream delta endpoint for bootstrap (`/api/client/delta`) even though updates are then received from SSE (`/api/client/streaming`).

This creates a coupling risk:
- if delta endpoint is unavailable/misconfigured but SSE is otherwise healthy, startup readiness can be delayed or fail
- operators may expect "streaming only" but still require delta endpoint compatibility at startup

## What `DELTA` currently affects

`DELTA` is still used in configuration constraints, notably single backend token per environment checks in DELTA/STREAMING mode.

But today it does not independently choose a delta-polling hydrator when streaming is off.

## Operator guidance

- If you enable `STREAMING`, ensure both upstream endpoints are usable:
  - `GET /api/client/delta` (bootstrap)
  - `GET /api/client/streaming` (steady-state updates)
- Do not assume `DELTA=true` alone enables delta polling.
- If your intent is "streaming only, no bootstrap delta dependency", that requires a code change.

## Cross-node reconnect contract

For clients connecting to Edge in a cluster, the important correctness boundary is the effective state the client can reconstruct, not whether every node exposes the same local revision history.

The intended contract for healthy nodes is:
- a downstream client may disconnect from one node and reconnect to another
- the new node may either replay incrementally from `Last-Event-ID` or fall back to a fresh hydration payload
- both responses are acceptable if they describe the same effective feature configuration for that token and environment

This follows from current implementation details:
- replay is based on the serving node's in-memory delta cache
- startup bootstrap and persistence can produce nodes with the same current snapshot but different local delta history

Practical implication:
- treat SSE `id` values as node-local replay hints, not as cluster-global correctness markers
- for cluster testing, compare semantic equivalence of the resulting state before comparing event shape
