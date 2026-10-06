# Streaming Test Scenarios

This document lists high-level test scenarios for validating current Unleash Edge behavior on `GET /api/client/streaming`.

It is based on:
- [streaming-configuration-and-behavior.md](/home/gaston/projects/unleash-edge/docs/streaming-configuration-and-behavior.md)
- current streaming, delta, reconnect, and filtering implementation in this repository

The intent here is to validate what Edge does today, not an idealized model.

For cluster-aware testing, there is one additional desired contract to keep in mind:
- reconnect across nodes does not need identical SSE history
- reconnect across healthy nodes should preserve the same effective configuration for the client
- hydration fallback on reconnect is acceptable when the new node cannot replay the requested revision locally

## Suggested validation method

For manual checks, keep one or more long-lived SSE connections open with `curl -N`.

Example:

```bash
curl -N \
  -H "Accept: text/event-stream" \
  -H "Authorization: $TOKEN" \
  http://edge-streaming-2:3063/api/client/streaming
```

Replay from a known revision:

```bash
curl -N \
  -H "Accept: text/event-stream" \
  -H "Last-Event-ID: 32033" \
  -H "Authorization: $TOKEN" \
  http://edge-streaming-2:3063/api/client/streaming
```

For most scenarios it helps to keep these clients open at the same time:
- token scoped to one project, for example `project-a:development...`
- token scoped to multiple projects, for example `project-a,project-b:development...`
- wildcard token, for example `*:development...`

For multi-node scenarios, keep equivalent streams open against each Edge node separately.

Record:
- whether Edge emits an SSE event at all
- SSE event type: `unleash-connected` or `unleash-updated`
- SSE envelope `id`
- delta payload event types: `Hydration`, `FeatureUpdated`, `FeatureRemoved`, `SegmentUpdated`, `SegmentRemoved`
- whether the payload contains only expected features and projects
- which Edge node served the response

When comparing responses from different nodes, record both:
- wire shape: hydration versus incremental replay
- effective state: the final feature and segment state the client would hold after applying the payload

For healthy-node cluster scenarios, effective state is the primary correctness signal.

## Important current-code notes

- Feature delta events are project-filtered.
- Segment delta events are not project-filtered in the streaming filter path.
- Initial hydration on stream connect also includes the environment's segments without project filtering.
- Streaming is environment-scoped. Updates from another environment should not reach the client.
- Reconnect with `Last-Event-ID` replays delta events after that revision when the revision is still present in the serving node's cache. Otherwise Edge falls back to a hydration payload.
- Upstream SSE disconnects are retried automatically. Upstream `401` and `403` stop the stream task.
- In enterprise mode, an invalid license pauses refresh activity.
- Startup persistence restores feature snapshots and seeds a delta hydration cache with the persisted last event id when available; it does not restore the full incremental event history.
- Invalid tokens may exist in the token cache, but only validated backend tokens are registered for background refresh and upstream streaming ingestion.
- An unrelated invalid token on the node should normally be inert for the result of these scenarios. An invalid token used by the actual downstream client is not inert and should fail authorization.

## Scenarios

## Token mix controls

### 1. Node has both valid and invalid tokens, but the test stream uses a valid token

Setup:
- the Edge node has at least one validated backend token for the target environment
- the same node also has one or more invalid tokens in its token cache or configured token set
- the downstream client connects with the validated token

Action:
- run any normal streaming scenario

Expected:
- the unrelated invalid token should not change the end result of the scenario
- Edge should still ingest upstream streaming updates for the validated token
- downstream behavior should match the same scenario executed on a node without the unrelated invalid token

Recommended usage:
- treat this as a standing control condition that can be present during the other scenarios if you want to prove invalid sibling tokens are inert

### 2. Downstream client uses an invalid token

Setup:
- Edge has a token known to be invalid

Action:
- attempt to open `/api/client/streaming` with that invalid token

Expected:
- request should be rejected rather than establishing a usable stream
- do not mix this case into the normal streaming scenarios, because it changes the test from propagation behavior to auth behavior

## Baseline streaming contract

### 3. Initial connect returns a hydration payload

Setup:
- Edge running with `STREAMING=true`
- token is valid for the target environment

Action:
- open `/api/client/streaming`

Expected:
- first SSE event type is `unleash-connected`
- payload contains a `Hydration` event
- SSE envelope contains an `id` matching the latest revision in the payload

### 4. Reconnect with `Last-Event-ID` resumes from that revision

Setup:
- establish a stream and note an SSE `id`
- apply one or more upstream changes afterwards

Action:
- reconnect with `Last-Event-ID` set to the earlier id

Expected:
- Edge should send only events with revision greater than `Last-Event-ID`
- response should not fall back to hydration if the revision is still present in the in-memory delta cache on that node

### 5. Updates from another environment are ignored

Setup:
- client connected with a `development` token
- another environment such as `production` is also active upstream

Action:
- make upstream changes only in `production`

Expected:
- no SSE event is emitted to the `development` stream

### 6. Stream closes after token invalidation

Setup:
- client is already connected

Action:
- invalidate the token in Edge's token cache or upstream token validation path
- trigger another upstream update

Expected:
- stream should terminate rather than continue emitting updates

## Disconnect and reconnect behavior

### 7. Upstream SSE disconnect triggers reconnect and resume

Setup:
- Edge is connected upstream in streaming mode
- at least one downstream client is connected to Edge
- note the latest downstream SSE `id`

Action:
- force the upstream `/api/client/streaming` connection used by Edge to drop
- make another upstream change after Edge reconnects

Expected:
- Edge should reconnect upstream automatically
- Edge should resume from the last seen upstream event id when possible
- downstream clients should continue receiving updates after reconnect

What to watch:
- whether the first post-reconnect downstream event is incremental or a hydration fallback
- whether any update is missed or duplicated

### 8. Upstream returns `401` or `403` on streaming reconnect

Setup:
- Edge was previously streaming successfully

Action:
- make upstream start rejecting Edge's streaming connection with `401` or `403`

Expected by current code:
- the upstream stream task stops instead of retrying forever
- downstream clients connected to Edge remain open but stop receiving fresh updates

Why this matters:
- this can look like a downstream filtering bug when the real issue is that Edge is no longer ingesting upstream deltas

### 9. Temporary upstream streaming failure without auth rejection

Setup:
- Edge was previously streaming successfully

Action:
- cause transient network failure, connection reset, or unexpected upstream stream termination

Expected:
- Edge should reconnect upstream automatically with backoff
- once reconnected, new updates should continue to propagate

### 10. Downstream client reconnect with old `Last-Event-ID`

Setup:
- client connects to Edge, receives some revisions, disconnects
- enough additional updates happen that replay behavior is worth checking

Action:
- reconnect the downstream client with the earlier `Last-Event-ID`

Expected:
- if Edge still has that revision in delta cache, it should replay incrementally
- if Edge no longer has that revision, it should return hydration instead

Why:
- reconnect semantics depend on the serving Edge node's local delta cache, not just on upstream state

## Project filtering

### 11. Single-project token does not receive feature updates from another project

Setup:
- client connected with token scoped only to `project-a`
- another feature exists in `project-b`

Action:
- update a feature in `project-b`

Expected:
- no SSE event related to that feature should reach the `project-a` client

Note:
- this expectation is safe for feature updates and removals
- it is not safe for segment-only updates because segment events are not project-filtered today

### 12. Multi-project token receives updates only for projects in scope

Setup:
- client connected with token scoped to `project-a,project-b`
- a third project `project-c` exists

Action:
- update one feature in `project-a`
- update one feature in `project-b`
- update one feature in `project-c`

Expected:
- stream emits updates for `project-a` and `project-b`
- stream does not emit a feature delta for `project-c`

### 13. Wildcard token receives updates from any project in the environment

Setup:
- client connected with wildcard token `*`

Action:
- update features in several projects

Expected:
- all feature updates in that environment are eligible to appear on the stream

## Segment propagation

### 14. Global segment change propagates to connected clients in the environment

Setup:
- at least one streamed client in the target environment

Action:
- change a global segment upstream

Expected:
- client should receive an update containing a segment delta or hydration fallback

Reason:
- current streaming filter path allows segment events regardless of project token scope

### 15. Project-scoped segment change for a connected project propagates

Setup:
- client connected to the same project that uses the segment

Action:
- update or remove that project-scoped segment upstream

Expected:
- stream should emit an update

### 16. Project-scoped segment change for a different project

Setup:
- client connected only to `project-a`
- segment belongs to or is only used by `project-b`

Action:
- update or remove the segment upstream

Expected by current code:
- Edge may still emit a segment update to the `project-a` client

Why this is worth validating:
- segment events are not project-filtered in `combined_filter`
- if product expectation is "no propagation outside the project", this test may expose a gap

### 17. Unreferenced segment change

Setup:
- change a segment that is not referenced by any streamed feature for that client

Action:
- update or remove the segment upstream

Expected by current code:
- if upstream emits a segment delta event, Edge may still forward it

Why:
- delta streaming does not prune segment events based on feature references
- segment pruning exists in feature snapshot filtering, not in live delta event filtering

## Moving a feature between projects

### 18. Move feature from origin project to target project

Setup:
- same feature name exists in `project-a` before the move
- `project-a` token
- `project-b` token
- `project-a,project-b` token

Action:
- move the feature from `project-a` to `project-b` upstream

Validate these outcomes:
- `project-b` client should receive an update that makes the feature appear in `project-b`
- combined `project-a,project-b` client should converge to the feature being in `project-b`
- `project-a` client should either receive a `FeatureRemoved` or receive no event at all, depending on the exact upstream delta sequence

Important note:
- current Edge behavior here depends on what upstream sends
- if upstream sends `FeatureRemoved(project-a)` followed by `FeatureUpdated(project-b)`, the origin stream should lose the feature and the target stream should gain it
- if upstream sends only `FeatureUpdated(project-b)`, Edge will filter that update out for the origin-project client, so the origin client may not observe the removal until a later hydration or reconnect

## Archive and restore

### 19. Archiving a flag removes it from the stream view

Setup:
- client connected to the feature's project

Action:
- archive the flag upstream

Expected:
- stream should emit a change that removes the feature from the client's effective state
- most likely payload is `FeatureRemoved`

### 20. Restoring an archived flag adds it back

Setup:
- same flag was archived and is absent from the client's effective state

Action:
- restore the flag upstream

Expected:
- stream should emit a change that adds the feature back
- most likely payload is `FeatureUpdated`

## License-related behavior

### 21. License becomes invalid while Edge is already running

Setup:
- enterprise Edge is running and streaming successfully

Action:
- make upstream heartbeat start reporting an invalid license

Expected by current code:
- Edge sets refresh state to paused
- feature refresh and upstream streaming ingestion stop advancing while the license is invalid
- downstream clients already connected to Edge may remain connected but should stop seeing new updates

Why this matters:
- this can affect every other streaming scenario and can be mistaken for stale-cache or filter issues

### 22. License becomes valid again after being invalid

Setup:
- continue from the previous scenario

Action:
- restore a valid license state upstream

Expected:
- Edge resumes refresh activity
- new upstream changes should start propagating again

Validate:
- whether downstream clients continue seamlessly
- whether the first observed update after recovery is incremental or hydration-based

### 23. Upstream license policy change reduces allowed Edge connections

Setup:
- two or more Edge instances connect upstream via streaming
- all are healthy before the policy change

Action:
- change upstream licensing or connection policy so fewer Edge instances are allowed

Expected:
- at least one Edge instance may lose the ability to continue ingesting upstream streaming updates
- affected nodes may become stale while still serving existing downstream connections

Validate per node:
- whether upstream streaming is rejected immediately or only on reconnect or heartbeat
- whether downstream streams stay open but become stale
- whether reconnecting a downstream client to a stale node changes the observed behavior

## Multi-node drift and startup ordering

### 24. Two Edge nodes start at different times and one has already consumed stream updates

Setup:
- `edge-a` starts first and receives one or more upstream streaming updates
- `edge-b` starts later against the same upstream and environment
- a client can connect to both nodes independently

Action:
- compare the first downstream stream response from each node

Expected:
- both nodes should converge to the same effective feature state after bootstrap
- SSE revision history may differ between nodes
- comparing only event ids or event counts is insufficient; compare effective state

Why:
- startup bootstrap uses `/api/client/delta`
- persisted startup state restores feature snapshots and seeds a delta hydration cache, but does not restore the full incremental event history

### 25. Reconnect behavior differs across nodes because delta cache history differs

Setup:
- `edge-a` has a richer local delta history than `edge-b`
- downstream client disconnects after observing an `id` from `edge-a`

Action:
- reconnect the same client through `edge-b` with that `Last-Event-ID`

Expected:
- `edge-b` may not be able to replay incrementally from that id
- `edge-b` may return hydration instead, even if `edge-a` could have replayed
- if both nodes are healthy, the resulting effective state should still match

This is expected today:
- replay is based on the serving node's local in-memory delta cache

### 26. Stale node can affect results of other streaming tests

Setup:
- one Edge node is stale because of disconnect, paused refresh, or rejected upstream streaming
- another Edge node remains healthy

Action:
- run the same downstream test case against both nodes

Expected:
- results may differ even with the same token and same upstream project changes

Recommendation:
- before executing project, segment, archive, or variant scenarios, confirm the target node is actively ingesting upstream updates
- if results differ across nodes, treat node freshness as the first variable to eliminate

## Manual cluster playbook

For reproducible manual checks against a real cluster:

1. Pin direct SSE connections to two individual Edge pods or instances rather than a load-balanced address.
2. Record the first event from each node and normalize what the client would consider its current state after applying it.
3. Create startup skew by starting `edge-a`, applying one or more upstream changes, and only then starting `edge-b`.
4. Capture an SSE `id` from `edge-a`, disconnect, and reconnect to `edge-b` with `Last-Event-ID`.
5. Treat hydration fallback on `edge-b` as acceptable if the normalized effective state matches what a healthy `edge-a` would produce.
6. If the normalized state differs, first verify whether one node is stale before treating it as a streaming contract failure.

## Variant changes

### 27. Add variants to a flag that previously had none

Action:
- add an initial variants array upstream

Expected:
- stream emits `FeatureUpdated`
- updated feature payload includes the new variants

### 28. Change variant weights only

Action:
- keep the same variant names and payloads but change weights

Expected:
- stream emits `FeatureUpdated`
- updated feature payload reflects new weights

### 29. Change variant definitions

Action:
- rename a variant, change stickiness, or change variant payload

Expected:
- stream emits `FeatureUpdated`
- updated feature payload reflects the changed definitions

### 30. Remove all variants from a flag

Action:
- delete all variants upstream

Expected:
- stream emits `FeatureUpdated`
- updated feature payload shows the variants removed

## What I would prioritize first

If you want the minimum set with the highest value, validate these first:
- token-mix control: valid stream token plus unrelated invalid token present on the node
- initial hydration and replay with `Last-Event-ID`
- upstream disconnect and reconnect, plus downstream reconnect behavior
- license invalidation and revalidation
- cross-node drift and replay fallback differences
- single-project token does not receive unrelated feature updates
- multi-project token receives only in-scope feature updates
- wildcard token receives all in-environment feature updates
- global segment change propagates
- project-scoped segment change to an unrelated project
- feature move across projects
- archive and restore
- variant add, modify, remove
