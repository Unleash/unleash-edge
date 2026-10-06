# Upstream Streaming Linked Event Proposal

## Problem

Edge consumes upstream Unleash changes over SSE and uses the upstream event `id` as the resume cursor when reconnecting. Those event ids are monotonically increasing, but they are not consecutive for a single Edge stream. A single stream represents one client token, while the global event id space may include updates that are irrelevant to that token.

This means Edge can tell that time moved forward, but it cannot prove that it received every relevant message on an already-established stream.

For example, Edge may observe:

```text
keepalive latest revision: 11
feature-updated revision: 14
```

If a relevant `feature-updated` event with revision `12` was lost while the connection was partially silent, Edge would not detect it. The later event with revision `14` is valid and newer, but it does not prove that all relevant earlier events were delivered. Once Edge applies revision `14`, a later keepalive that reports latest revision `14` also appears healthy.

The current idle-timeout reconnect fix handles fully silent streams. It does not detect selective loss where some bytes or later events still arrive after one or more relevant messages were missed.

## Proposed Solution

Unleash should include a `previous_event_id` value in every SSE message that is part of a single token stream. The value points to the previous message id sent on that same SSE response, forming a linked list of delivered stream messages.

The existing event `id` remains the durable resume cursor. The new `previous_event_id` is an in-stream loss detector for the active connection.

Conceptually:

```text
id: 51421
event: feature-updated
data: {"previous_event_id":51418,...}
```

Edge tracks the last accepted event id for the current SSE connection. When a new SSE message includes `previous_event_id`, Edge validates that it matches the last accepted event id for that connection.

Expected behavior:

- First message on a stream may omit `previous_event_id` or set it to `null`.
- After Edge accepts a message with `id`, that id becomes the connection's last accepted event id.
- The next message with `previous_event_id` must point to that last accepted event id.
- If `previous_event_id` points to a different value, Edge treats the stream as unreliable.
- On an unreliable stream, Edge closes the stream and reconnects using the last successfully applied durable event id.
- Unleash then replays or hydrates from that durable cursor, as it does for reconnects today.

This separates the two responsibilities:

- Event `id`: durable replay cursor across reconnects.
- `previous_event_id`: in-stream message-loss detector.

This works with the existing non-consecutive global event ids because Edge no longer needs to infer gaps from numeric adjacency. It only needs to verify that each delivered message links to the last message Edge actually processed on that connection.

## Server State

Unleash does not need a durable per-token sequence counter for this mechanism.

For each active SSE response, Unleash only needs to keep the last event id sent on that response. When it sends the next message, it includes that remembered value as `previous_event_id`, then updates the remembered value to the new message id.

If the server uses an in-memory event cache and the previous event is no longer present, the connection-local last sent id is still enough. The protocol is linking messages delivered on the current response, not requiring lookup of historical event contents.

## Compatibility

Edge should tolerate missing `previous_event_id`.

If an SSE message does not include `previous_event_id`, Edge keeps the current behavior and does not perform linked-event validation for that message. This allows Edge to work with older Unleash versions and allows Unleash to roll out the field before making it mandatory.

When `previous_event_id` is present, Edge should validate it. A stream can therefore become safer as soon as the upstream server starts emitting the field, without requiring a lockstep deployment.

During rollout, useful logging would be:

- Debug log when a stream starts linked-event validation.
- Warning log when a link mismatch is detected, including expected and received previous event ids.
- Debug log when validation is skipped because `previous_event_id` is absent.

## Required Changes

### Unleash

Unleash should track the last event id sent for each active SSE response serving a client token.

For each data-bearing stream message, Unleash should:

- Include the current connection-local last sent id as `previous_event_id`.
- Send the new message with its normal durable event `id`.
- Update the connection-local last sent id to the new message id after sending.

The link should apply to hydration, feature update, segment update, and other data-bearing stream messages. If keepalives are represented as parseable SSE events rather than comments, they can also carry the same link to detect gaps earlier.

The connection-local previous id does not need to be durable across reconnects. On reconnect, Edge uses the last successfully applied durable event id as the resume cursor, and the new SSE response starts a new linked chain.

### Edge

Edge should parse optional `previous_event_id` values from upstream SSE messages.

For each active upstream SSE connection, Edge should keep linked-event state:

- No linked-event state before the first accepted message with an event id.
- Last accepted event id after every successfully processed message with an event id.

When a message with `previous_event_id` arrives:

- If no last accepted event id exists for the connection, accept the message and establish the baseline.
- If `previous_event_id` equals the last accepted event id, process the message and advance the baseline to the message id.
- If `previous_event_id` differs from the last accepted event id, close the stream and reconnect using the last successfully applied durable event id.

Edge should not advance its stored durable cursor for a message that fails linked-event validation.

### Tests

Edge should have tests for:

- Existing behavior when upstream messages do not include `previous_event_id`.
- Valid linked messages where each `previous_event_id` points to the prior accepted event id.
- A missing `previous_event_id` after validation has started.
- A link mismatch, causing reconnect with the last successfully applied event id.
- A duplicate or older message whose `previous_event_id` does not match the current connection baseline.

Unleash should have tests proving that a single token stream emits `previous_event_id` values that link each sent message to the previous message sent on that SSE response.

## Follow-Up

If this mechanism works well for Edge consuming Unleash, we should consider applying the same model to SDKs consuming Edge streaming.

That follow-up would require Edge's streaming controller to emit linked event metadata for each SDK-facing SSE response, and SDKs would use it the same way Edge uses upstream `previous_event_id`: tolerate missing values for compatibility, validate present values, and reconnect when a link mismatch is detected.
