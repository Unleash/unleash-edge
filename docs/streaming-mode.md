# Streaming Mode

This document covers the operational model for running Edge in streaming mode, with a focus on how to verify that Edge
is healthy and how self-hosted customers can alert on stale Edge instances.

## Scope

Streaming mode changes how Edge receives updates from upstream Unleash:

- by default, Edge refreshes feature data by polling `GET /api/client/features`
- with `STREAMING=true`, Edge bootstraps through `GET /api/client/delta` and then receives updates from upstream
  `GET /api/client/streaming`

This is primarily about streaming from upstream Unleash to Edge. Edge can also expose `GET /api/client/streaming` to
SDKs, and reconnecting SDKs may send `Last-Event-ID`; Edge replays from that revision when it still has the local delta
history, otherwise it falls back to hydration. SDK-to-Edge streaming is beta functionality, is not available in all
SDKs, and is not recommended for production environments yet.

## Enable streaming

Streaming mode requires:

- the enterprise Edge build
- an enterprise Unleash instance
- for self-hosted Unleash, a license that enables Edge streaming
- only one configured Edge token per environment

Start Edge with streaming enabled:

```shell
unleash-edge edge --streaming
```

or set:

```shell
STREAMING=true
```

You do not need to set `STREAMING=false`; polling mode is the default.

When streaming is enabled, make sure upstream Unleash exposes both endpoints to Edge:

- `GET /api/client/delta` for startup hydration
- `GET /api/client/streaming` for steady-state updates

## Runtime behavior

Streaming mode does not automatically fall back to polling. If Edge is started with `STREAMING=true`, it uses the delta
and streaming endpoints described above until streaming is disabled and Edge is restarted or reconfigured.

When the upstream SSE connection ends, errors, or becomes idle, Edge reconnects and resumes from the last upstream event
id when possible. Temporary network failures should recover without operator action.

If upstream rejects the streaming connection with `401` or `403`, Edge stops that stream task instead of retrying
forever. Edge continues serving the last state it has already applied, but it will not receive new upstream changes for
that token scope until the rejection is fixed and Edge is restarted. The usual causes are missing enterprise or
license support, an invalid token, a revoked token, or a token that no longer has access to the requested environment or
projects.

## Health model

The main operational question is whether every Edge instance has observed the same latest feature revision for each
token scope it serves. A healthy deployment should converge so that all active Edge instances report the same revision
for the same `environment` and `projects` combination.

There are two practical ways to inspect this.

### Enterprise Edge UI

In Unleash, open `/admin/enterprise-edge`.

The page shows each registered Edge instance, the tokens it serves, and the revision id each token has observed. Use this
view when debugging a customer incident because it answers these questions quickly:

- which Edge instances are connected
- which token scopes each instance serves
- whether one instance is behind the others
- whether a specific environment or project scope is stale

If one instance reports an older revision for the same token scope than the rest of the fleet, that instance is probably
not receiving updates from upstream Unleash.

### Prometheus metric

Edge exports the latest delta revision it has applied:

```text
delta_revision_id{environment="<environment>",projects="<project-list>"} <revision>
```

Hosted and self-hosted observability stacks may add labels such as `app_name`, `instance_id`, `pod`, `region`, or
`cluster`. Use those labels to compare revisions across Edge instances.

For a self-hosted Prometheus setup, use explicit deployment labels to compare the revision reported by each Edge
instance:

```promql
max by (cluster, namespace, pod, environment, projects) (
  delta_revision_id{job="unleash-edge"}
)
```

## Alerting on drift

The most useful alert is not "revision changed"; it is "more than one revision is currently reported for the same token
scope." That catches the case where at least one Edge instance is stale while other instances have moved forward. In most
deployments, every region should serve the same upstream Unleash state, so the alert should compare all Edge instances
for the same `environment` and `projects` labels.

Example alert expression:

```promql
count_values by (environment, projects) (
  "revision",
  max by (pod, environment, projects) (
    delta_revision_id{job="unleash-edge"}
  )
) > 1
```

Recommended alert settings:

- evaluate every minute
- require the condition for at least five minutes
- include the affected `environment` and `projects` labels in the notification
- link the notification to the Edge dashboard or `/admin/enterprise-edge`

## Validate streaming

Use `curl -N` to keep a streaming connection open while you make a controlled feature change in Unleash:

```shell
curl -N \
  -H "Accept: text/event-stream" \
  -H "Authorization: $TOKEN" \
  "$EDGE_URL/api/client/streaming"
```

The initial response should contain an `unleash-connected` event with a hydration payload. The SSE envelope should
include an `id` matching the latest revision in the payload.

After noting an event id, reconnect from that revision:

```shell
curl -N \
  -H "Accept: text/event-stream" \
  -H "Last-Event-ID: $REVISION_ID" \
  -H "Authorization: $TOKEN" \
  "$EDGE_URL/api/client/streaming"
```

If the serving Edge node still has the requested revision in memory, it should replay later events. If it does not, it
should send a hydration payload instead. In both cases, the important result is that the client reaches the same
effective feature state.

For multi-node validation, run the same `curl` command against each Edge node or load-balanced endpoint and compare:

- whether each node emits an SSE event
- the SSE event type, usually `unleash-connected` or `unleash-updated`
- the SSE envelope `id`
- whether the final feature state is equivalent across nodes

You can also compare regular client API responses and ETags across Edge nodes. Use the same token and query parameters
for every request:

```shell
curl -i \
  -H "Authorization: $TOKEN" \
  "$EDGE_URL/api/client/features"
```

For the same token scope and request shape, matching `delta_revision_id` values should normally produce matching
responses and matching `ETag` headers. If two Edge nodes report the same revision but return different ETags, treat that
as possible state drift. The revision id says both nodes observed the same latest revision; the ETag compares the actual
response content. Different ETags can indicate a missed message, inconsistent local state, or another bug that left one
node with different effective feature data.

## Failure modes

Streaming introduces a few distinct failure modes. The observed symptom is usually a revision that stops advancing on
one or more Edge instances. In rarer cases, the revision can look healthy while the effective response state differs
between nodes.

### Startup cannot hydrate

Streaming mode still depends on `GET /api/client/delta` for initial hydration. If the delta endpoint is unavailable,
misconfigured, or blocked by network policy, Edge may fail readiness or start without usable feature data for the token.

Check:

- Edge logs for delta fetch failures
- connectivity from Edge to upstream Unleash
- token validity and environment/project scope
- upstream support for the delta endpoint

### Upstream stream disconnects

Edge reconnects when the upstream SSE connection ends, errors, or becomes idle. Temporary network failures should
recover automatically. During the gap, Edge keeps serving its last known state, but `delta_revision_id` will not advance.

Alerting on revision drift catches this when at least one other Edge instance continues receiving updates.

### Upstream rejects streaming

Upstream can reject streaming in multiple ways. The first thing to verify is that Edge is connected to an enterprise
Unleash instance, and that a self-hosted Unleash instance has a license that enables Edge streaming.

If upstream returns `401` or `403` for the streaming connection, Edge stops that stream task. This can also mean the token
was revoked, changed, or no longer has access to the requested scope. Edge does not switch that token scope back to
polling automatically.

Check:

- whether upstream Unleash is enterprise and licensed for Edge streaming
- whether the token still exists in Unleash
- whether the token has the expected environment and project access
- whether all Edge instances were restarted or reconfigured with the same token set

### All Edge instances are stale

Revision drift alerts compare Edge instances against each other. If every Edge instance is disconnected from upstream,
they can all report the same old revision and the drift alert will not fire. This cannot be detected reliably from
`delta_revision_id` alone because a quiet Unleash instance may legitimately have no feature changes for a day or more.

In this case, use direct debugging instead: check Edge logs, upstream Unleash availability, token validity, and whether a
controlled feature update advances `delta_revision_id`.

### Same revision but different response state

Revision drift is the easiest stale-data signal to alert on, but it is not a complete proof that every node has identical
state. A node can theoretically report the latest revision while its cached feature state differs from another node with
the same revision. This usually points to a missed or incorrectly applied message, or to another state-management bug.

The practical symptom is that two Edge nodes return different `ETag` headers for the same `GET /api/client/features`
request, even though they report the same `delta_revision_id` for that token scope. When debugging this case, collect:

- the `delta_revision_id` labels and value for each node
- the request URL, token scope, and query parameters used for comparison
- the `ETag` header from each node
- the response body from each node, if it is safe to collect
- the Edge logs around the revision where state diverged

### Different nodes have different replay history

Edge keeps delta history in memory. Two healthy nodes can have the same effective feature state while retaining different
local event history, especially after restarts. When an SDK reconnects to a different node with `Last-Event-ID`, that node
may replay incrementally if it has the requested revision or send a hydration event if it does not.

This is expected. Compare effective feature state and reported latest revision, not the exact sequence of replayed
events.

### Invalid or unsupported token shape for revision reporting

Revision reporting depends on Edge knowing the token environment and project scope. If a token shape does not carry a
clear environment, it may not produce the same revision labels as normal environment-scoped tokens.

Prefer normal client tokens with explicit environment and project scope for streaming beta customers.

### More than one token is configured for an environment

Streaming mode enforces one configured token per environment. If an Edge deployment uses multiple tokens for the same
environment, consolidate the token configuration before enabling streaming.

## Incident checklist

When a customer reports stale streaming data:

1. Open `/admin/enterprise-edge` and compare revision ids for the affected environment and project.
2. Check `delta_revision_id` grouped by instance, environment, and projects.
3. Verify the customer is using enterprise Unleash and, for self-hosted Unleash, a license that enables Edge streaming.
4. Confirm there is only one configured Edge token per environment.
5. Use `curl -N` against the affected Edge node to validate whether `/api/client/streaming` emits events.
6. If only one instance is behind, inspect that instance's logs and upstream network path.
7. If all instances are behind, make a controlled feature update and check whether `delta_revision_id` advances.
8. If revisions match but behavior differs, compare `ETag` headers and response bodies for the same
   `/api/client/features` request against each Edge node.
9. Restart or roll the affected Edge instance only after collecting the stale revision, ETag, response, and logs, because
   restart may rehydrate and hide the original failure mode.
10. If needed, disable streaming and return to polling while the streaming issue is investigated.
