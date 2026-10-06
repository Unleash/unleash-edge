# Frontend Streaming With Context-Aware Evaluations

## Status

Proposed discussion note. This document is intended to start the design conversation, not to lock in an implementation.

## Problem Statement

Edge already supports streaming for `/api/client/streaming` when running with streaming and delta enabled. That model propagates feature configuration changes to connected backend clients.

Frontend clients are different. `/api/frontend` returns evaluated flags, and those evaluations depend on request context. If we want to support a streaming model for frontend clients, Edge needs a way to keep the client view up to date when:

- feature configuration changes upstream
- segments or constraints change upstream
- the connected client's context changes locally

This changes the cost model. Backend SDKs usually run on servers, so the number of connected instances tends to stay relatively small. Frontend SDKs run on end-user devices, where the number of active clients can easily be several orders of magnitude larger.

Backend streaming can keep very little state per connection. Frontend streaming may require per-client state if we want to react to context changes without forcing a reconnect, and the much larger connection count makes that state materially more expensive.

## Constraints

- This capability should be Edge-only.
- Frontend evaluations must be based on the full flag set, not only enabled flags, because a flag can change from enabled to disabled and that change still needs to be propagated.
- The design must support very high frontend fanout, potentially on the order of hundreds of thousands of concurrent clients per node.
- Context can change faster than feature configuration.
- High-rate context changes can create load spikes even if feature configuration is stable.
- At a planning scale of roughly 600,000 concurrent clients, memory, CPU, and network all become first-order concerns rather than secondary implementation details.
- If a connection or context change results in sending a full evaluated state, payload size becomes a first-order scaling concern.

## Practical Sizing Notes

Some rough initial estimates make per-connection context storage look feasible, at least for a simple first pass, but only as one part of the overall resource picture.

- If the stream handler closes over the normalized context per SDK connection, a representative context with all common fields set plus about 10 properties is roughly 170 bytes.
- At 600,000 concurrent connections, that implies about 100 MB of raw context data.
- Real memory usage will be higher once allocator, struct, and connection overhead are included, so a safer rough planning number is closer to 2 GB.

That is still within a plausible operating range for Edge, which suggests that holding context in memory is not immediately disqualifying. Very large contexts remain a risk, but they are already a risk on the existing HTTP request path as well.

That said, per-connection context is only one part of the memory picture. For high-fanout SSE, the larger cost may be kernel-managed TCP socket buffers rather than Rust heap allocations. Early baseline measurements suggest idle SSE connections at 600,000 clients could consume roughly 132 GB of kernel memory on default Linux settings, and closer to 24 GB with more aggressive tuning in favorable cases. This means connection-level memory may become the main scaling constraint before per-session context storage does.

## Existing Building Blocks

Edge already has a notification path that fans out messages when a new feature event is received. That existing mechanism should be reusable for frontend streaming:

- upstream change arrives
- Edge fanout notifies active frontend streams
- each stream reevaluates the current context against the latest state via Yggdrasil
- Edge emits an updated evaluated snapshot if the result changes

This keeps the first implementation aligned with how Edge already reacts to upstream changes instead of introducing a separate invalidation model.

## Core Design Question

When a frontend client's context changes, should Edge:

1. require the client to close and reopen the stream with the new context
2. accept an in-place context update and return a full rehydration snapshot
3. accept an in-place context update and return only the evaluated diffs

Each option is viable. The main tradeoff is simplicity and bounded memory versus lower network churn and better incremental behavior, with CPU also becoming a concern if context changes happen regularly and Edge must repeatedly compute and send full snapshots on the fly.

## Option 1: Reconnect On Context Change

The client treats context as immutable for the lifetime of a streaming connection. When context changes, it closes the existing stream and opens a new one with the updated context.

### Advantages

- Simplest protocol
- No separate context update endpoint
- No need to keep mutable per-client context state on the server for the purpose of in-place updates
- No race between context updates and in-flight config updates
- Reconnect can always begin with a full hydration event

### Disadvantages

- Higher connection churn
- More repeated authentication and stream setup work
- More full snapshots sent over the network
- Potential reconnect storms if many clients update context at the same time

### Best Fit

This is the safest first version if context is usually stable for the life of a page, tab, or user session.

## Option 2: In-Place Context Update With Full Rehydration

The client keeps the streaming connection open and sends context updates through a separate endpoint. Edge stores the latest context for the session, re-evaluates all flags, and emits a fresh full evaluated snapshot.

### Advantages

- Less connection churn than reconnecting
- Simpler than maintaining fine-grained evaluated diffs
- A reasonable incremental step if reconnect cost becomes noticeable

### Disadvantages

- Requires server-side session state
- Requires a context update protocol
- Every context update still triggers full reevaluation of all flags
- Still sends a full snapshot after each context update

### Best Fit

This is a good middle step if reconnect churn is too expensive but we want to avoid the complexity of per-session diffing.

## Option 3: In-Place Context Update With Evaluated Diffs

The client keeps the streaming connection open and sends context updates through a separate endpoint. Edge stores enough state to compare old and new evaluations and emits only changed toggles.

### Advantages

- Lowest network overhead on frequent context changes
- Best incremental behavior for highly dynamic clients
- Avoids reconnect churn and repeated full snapshots

### Disadvantages

- Highest implementation complexity
- Highest per-session memory cost
- Higher CPU cost on each context update
- Requires session state, diffing logic, and careful concurrency handling

### Best Fit

This should be treated as an optimization path, not the default starting point.

## Main Tradeoffs

### Memory

Frontend streaming becomes materially more expensive if Edge stores per-session state. Depending on the chosen model, this may include:

- token and environment identity
- last context
- session timestamps and liveness data
- last event id or revision marker
- previous evaluated result set or a compact representation used for diffing

Option 1 has the lowest server memory footprint. Option 3 has the highest.

However, application-managed session state is not the full story. At large connection counts, SSE socket overhead in the kernel may outweigh the Rust-side memory used for context and session bookkeeping, so capacity planning needs to consider both layers.

### CPU

Context changes can be more frequent than configuration changes. Every context change may force a reevaluation across the full flag set for that client.

- Option 1 shifts this work into reconnect and fresh hydration.
- Option 2 reevaluates all flags on each accepted context update.
- Option 3 reevaluates all flags and then computes a per-session diff.

If context changes at high rate, CPU can become the dominant risk even before memory limits are hit.

There is also a separate fanout cost when upstream configuration changes. A rough per-evaluation estimate in Yggdrasil is on the order of sub-microsecond work, but the real cost depends on fanout width and how many flags need to be reevaluated.

- If each affected stream triggers its own reevaluation, total work scales with active client count.
- Even if a single evaluation is cheap, multiplying that by hundreds of thousands of active streams can become expensive during large updates.
- This needs real benchmarks before we rely on napkin math.

The key point is that "cheap per evaluation" does not automatically mean "cheap at fleet scale" once fanout is involved.

### Network

- Option 1 increases connection churn and full hydration traffic.
- Option 2 reduces connection churn but still produces full hydration traffic after context changes. It's similar to polling, but upstream changes are streamed.
- Option 3 minimizes payload size at the cost of more server work.

If the chosen protocol sends the full evaluated state on connect or on context change, event size becomes a critical part of system behavior. Even when feature configuration is stable, large evaluated payloads can materially increase bandwidth, serialization cost, and client-side processing time.

### Complexity

- Option 1 is operationally simplest.
- Option 2 introduces moderate protocol and state-management complexity.
- Option 3 introduces the most moving parts and the largest test surface.

## High-Rate Context Change Risk

Clients that change context very frequently can become a load amplifier:

- repeated reevaluation of the full flag set
- repeated full snapshots or diffs
- repeated connection establishment if reconnect is the chosen model
- bursty fanout if many clients update context during the same period
- repeated transmission of large evaluated payloads when the server responds with full state

This risk is especially important for frontend integrations that build context from rapidly changing UI state instead of a smaller stable identity model.

The specification should explicitly discourage high-frequency context mutation and document expected usage patterns. It should also call out that event design matters: if full evaluated state is sent often, field shape, redundancy, and serialization overhead can become part of the scaling problem even if this document does not define the final event schema.

## Recommended Safety Controls

Regardless of which option we choose, the first implementation should include protective controls.

### Rate Limits

Rate limit context changes per client session, per token, and potentially per IP.

This should apply to:

- reconnect frequency for the reconnect-based model
- accepted context update frequency for in-place update models

### Backoff Guidance

Document that clients should use exponential backoff with jitter when:

- reconnecting after a rejected or failed context change
- retrying after rate limiting
- re-establishing a dropped stream

This reduces synchronized retry spikes and gives Edge time to recover under pressure.

### Bounds

Set explicit bounds in the design:

- maximum number of concurrent frontend streaming sessions
- maximum context payload size
- idle session timeout
- maximum queued updates per session or per environment

When limits are exceeded, Edge should prefer rejecting updates or terminating sessions over allowing unbounded growth.

Hard limits are especially important if we keep context in memory. A maximum context size per connection is simpler and safer than trying to spill active streaming state to disk. Disk spill is theoretically possible, but it creates a new bottleneck if many live streams need those contexts again at the same time.

### Normalization

Normalize context before storing or evaluating it:

- fill in derived values consistently
- remove redundant fields where possible
- reject malformed or oversized payloads

This keeps comparisons predictable and reduces avoidable memory usage.

### Observability

Add metrics before scaling usage:

- connected frontend streaming sessions
- context update rate
- reconnect rate
- rate-limited requests
- reevaluation latency
- per-session termination count
- dropped or rejected updates

Without these signals, it will be difficult to judge whether reconnect churn or in-place updates are the larger problem in production.

## Suggested Direction For Discussion

The most conservative starting point is:

- v1: context is immutable for the lifetime of a stream
- clients reconnect when context changes
- Edge responds with a full evaluated hydration snapshot on connect
- rate limits and backoff guidance are part of the first version

If we move beyond reconnect-on-change, the next simplest step is still to treat context mutation as "reevaluate and send a new full snapshot", not "send only context deltas" or "send only evaluated diffs". Partial update schemes may become worthwhile later, but they add protocol and state complexity before we have baseline memory and CPU numbers.

## Deferred Optimizations

If the simple model is too expensive, there are more advanced optimizations we could explore later:

- deduplicate identical normalized contexts behind a shared lookup instead of storing each one inline
- cache repeated evaluation results where multiple clients converge on the same effective context
- use rule-aware hints from Yggdrasil to identify which context fields actually matter for a given flag evaluation, so irrelevant fields do not prevent cache hits

These ideas may reduce work, but they should be treated as follow-on optimizations, not requirements for the first design.

If reconnect churn proves too costly, a likely next step is:

- v1.5: keep the stream open, accept context updates, and emit a fresh full snapshot

Only after measurement should we consider:

- v2: in-place context updates with evaluated diffs

This progression keeps the first implementation operationally simple while preserving a path toward more efficient behavior if usage patterns justify it.

## Open Questions

- How often do real frontend clients change context in practice?
- Should the initial protocol support `Last-Event-ID`, or should reconnect always force a fresh hydration event?
- Should rate limits be enforced per session, per token, per IP, or all three?
- What is the maximum acceptable memory budget for frontend streaming state in a single Edge instance?
- How should Edge behave when a client repeatedly exceeds reconnect or context update limits?
- Do we want the first version to support only one stable context per browser tab/session, and explicitly recommend that SDKs debounce context changes?

## Decision Criteria For The Follow-Up Proposal

The final proposal should be evaluated against:

- implementation complexity
- correctness under concurrent config and context changes
- memory growth under many connected clients
- CPU growth under high-rate context changes
- user-visible latency for receiving updated evaluations
- operational safety under bursty traffic

## Next Step

Use this note to align on the first version we want to build, then write a narrower follow-up decision document that:

- picks one delivery model
- defines the API and event shapes
- sets explicit guardrails and limits
- describes rollout and observability requirements
