# Frontend Streaming Protocol Sketch

## Status

Proposed protocol sketch. This is not a final specification. It exists to make the design discussion concrete enough to evaluate tradeoffs and implementation cost.

## Purpose

This document sketches two candidate delivery models for frontend streaming on Edge:

- v1: immutable context per stream, reconnect on context change
- v1.5: in-place context update, full rehydration after update

It does not define a final payload schema, but it does identify the expected endpoints, event types, and reconnect rules.

## Shared Assumptions

- This capability is Edge-only.
- Frontend streaming returns evaluated flags, not raw feature configuration.
- The initial stream response must contain the full evaluated state for the current token and context.
- The server evaluates against the full flag set, not only enabled flags.
- Event payload size matters and should be treated as a protocol concern, even in a lightweight first version.
- SSE is the transport for server-to-client delivery.

## Common Event Model

The exact payload shape can change, but the event categories should remain small and predictable.

### `unleash-connected`

Sent once after the stream is established.

Expected payload:

- full evaluated hydration snapshot
- metadata needed by the client to treat this snapshot as authoritative

This is the client's baseline state.

### `unleash-updated`

Sent when the evaluated result changes because upstream configuration changed, segments changed, or the session context changed.

Expected payload:

- either a full evaluated hydration snapshot
- or an incremental set of changed toggles

For v1 and v1.5 as described here, this should remain a full evaluated hydration snapshot for simplicity. The same principle applies when session context changes: the client can treat the follow-up event as a fresh authoritative snapshot instead of trying to merge a partial context or evaluation delta.

### `unleash-disconnected`

Optional terminal event before the server closes the stream.

Useful when:

- the token becomes invalid
- the environment is removed
- the session is evicted
- the client exceeds policy limits

Expected payload:

- reason code
- whether the client should retry
- optional retry hint

### `keep-alive`

Periodic keep-alive frame to keep intermediaries and clients from treating the connection as idle.

## Version 1: Immutable Context Per Stream

## Summary

The client opens a stream with a fixed context. If the context changes, the client must close the stream and reconnect with the new context.

This keeps the server-side model simple and avoids in-place session mutation.

## Endpoints

### `GET /api/frontend/streaming`

Opens an SSE stream.

Inputs:

- `Authorization` header with frontend token
- context supplied in query parameters for the initial version
- optional `Last-Event-ID` header

Expected response:

- `200 OK`
- `Content-Type: text/event-stream`
- first event is `unleash-connected`

Notes:

- A `POST` variant could be added later if query-string context becomes too limiting, but the simplest sketch starts with `GET`.
- If the initial context is too large for a query string, that is a signal that the protocol may need to move to a session bootstrap endpoint.

## Connection Lifecycle

1. Client opens `GET /api/frontend/streaming` with token and context.
2. Edge validates the token and normalizes the context.
3. Edge evaluates the full flag set for that token and context.
4. Edge sends `unleash-connected` with a full evaluated hydration snapshot.
5. Edge keeps the connection open and sends `unleash-updated` whenever upstream changes alter the evaluated result for that same context.
6. If the client's context changes, the client closes the stream and opens a new one.

## Reconnect Semantics

### Reconnect with unchanged context

If the client reconnects with the same effective context:

- the client may send `Last-Event-ID`
- Edge may use it to decide whether replay is possible
- if replay is not supported or not safe, Edge should send a fresh full hydration snapshot

For a conservative first version, always sending a fresh hydration snapshot is acceptable.

### Reconnect with changed context

If the context has changed:

- the client must treat the connection as a new session
- Edge should ignore `Last-Event-ID` for recovery purposes
- Edge should send a fresh `unleash-connected` hydration snapshot for the new context

This avoids mixing event recovery semantics across different evaluation inputs.

## Failure Semantics

If Edge rejects or closes the stream:

- invalid token: close immediately, optionally send `unleash-disconnected`
- context too large or invalid: reject the request before opening the stream
- server overload or policy limit: reject or terminate the stream with a retry hint

Clients should reconnect using exponential backoff with jitter.

## Operational Notes

- This model pushes context change cost into connection churn.
- Every reconnect may trigger a full evaluated hydration snapshot.
- Payload size is part of the cost model, especially if the token has access to many flags.

## Version 1.5: In-Place Context Update With Full Rehydration

## Summary

The client keeps the stream open and sends explicit context updates through a separate endpoint. Edge stores the latest context for the session and responds by sending a fresh full evaluated hydration snapshot over the stream.

This adds protocol and state-management complexity, but avoids forcing a reconnect for every context change.

## Endpoints

### `GET /api/frontend/streaming`

Opens an SSE stream.

Inputs:

- `Authorization` header with frontend token
- initial context supplied in query parameters
- optional `Last-Event-ID` header

Expected response:

- `200 OK`
- `Content-Type: text/event-stream`
- first event is `unleash-connected`

### `POST /api/frontend/streaming/context`

Updates the context for an existing streaming session.

Inputs:

- `Authorization` header with the same frontend token
- session identifier
- context in request body

Expected response:

- `202 Accepted` when the update is accepted
- `400` for invalid context
- `404` if the session does not exist
- `409` if the session is no longer valid for update
- `429` if the client exceeds context update limits

Notes:

- The session identifier can be generated by Edge and returned in the initial `unleash-connected` payload or in a response header during stream setup.
- This endpoint only changes session context. It does not return evaluated flags directly.

## Connection Lifecycle

1. Client opens `GET /api/frontend/streaming` with token and initial context.
2. Edge validates the token, creates a session, normalizes the context, and evaluates the full flag set.
3. Edge sends `unleash-connected` with a full evaluated hydration snapshot and a session identifier.
4. When the client's context changes, the client sends `POST /api/frontend/streaming/context` with the session identifier and the new context.
5. Edge validates and stores the new context.
6. Edge reevaluates the full flag set for the updated context.
7. Edge sends `unleash-updated` with a fresh full evaluated hydration snapshot.
8. The stream remains open for future config changes and future context updates.

## Reconnect Semantics

### Reconnect after transport failure

If the SSE connection drops unexpectedly:

- the client should attempt to reconnect with backoff
- the client may reuse the current context as the initial context
- Edge may treat this as a new session and return a new session identifier

For the first version of this model, creating a new session on reconnect is simpler than trying to resume a prior session.

### Reconnect after context update race

If a client sends a context update and the stream drops before the follow-up event is received:

- the client should reconnect using the latest context it believes to be authoritative
- Edge should return a fresh full hydration snapshot

This avoids having to make session recovery dependent on partially applied context mutations.

## Failure Semantics

If Edge rejects a context update:

- the stream may remain open with the previous context still active
- the server should not silently switch to a partially accepted state
- the client should either retry with backoff or reconnect explicitly with a known-good context

If Edge terminates the stream:

- the client should assume the in-memory session is gone unless the protocol later guarantees otherwise
- reconnect should be treated as a fresh hydration flow

## Operational Notes

- This model reduces reconnect churn.
- It requires session storage on Edge.
- It still sends a full evaluated hydration snapshot after each accepted context update.
- Rate limiting the context update endpoint is critical.
- Payload size is still a scaling risk because the server may send a full evaluated state repeatedly without any connection teardown.
- This is intentionally simpler than sending only context changes or only evaluated diffs. Those can be revisited later if benchmarks show that full rehydration is too expensive.

## Deferred Topics

These items are intentionally not part of this sketch:

- incremental evaluated diffs after context updates
- context-delta payloads from client to server instead of sending a fresh full context
- final event payload field names
- batching or coalescing multiple context updates
- compression strategy
- cross-node session recovery in a multi-instance deployment
- rule-aware caching or deduplication based on which context fields a flag actually reads

## Open Questions

- Should `GET /api/frontend/streaming` accept context in query parameters only, or should we add a bootstrap `POST` flow early?
- Should session identifiers be opaque and server-generated, or should the client present its own idempotency key?
- Should `Last-Event-ID` be supported in v1, or should reconnect always force a full hydration event?
- Should rate limiting be expressed through headers, event payloads, or both?
- At what payload size should Edge reject a context or terminate a session for safety reasons?

## Suggested Next Step

Use this sketch to choose the first protocol to implement. Once that is decided, the next document should narrow this into:

- concrete request and response examples
- exact event payload schema
- explicit retry and rate-limit behavior
- testing and rollout requirements
