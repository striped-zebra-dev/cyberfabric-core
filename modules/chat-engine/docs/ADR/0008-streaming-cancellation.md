Created:  2026-02-04 by Constructor Tech
Updated:  2026-03-06 by Constructor Tech
# ADR-0008: Client-Initiated Streaming Cancellation


<!-- toc -->

- [Context and Problem Statement](#context-and-problem-statement)
- [Decision Drivers](#decision-drivers)
- [Considered Options](#considered-options)
- [Decision Outcome](#decision-outcome)
  - [Consequences](#consequences)
  - [Confirmation](#confirmation)
- [Pros and Cons of the Options](#pros-and-cons-of-the-options)
  - [Option 1: Close HTTP connection](#option-1-close-http-connection)
  - [Option 2: HTTP DELETE request](#option-2-http-delete-request)
  - [Option 3: HTTP timeout](#option-3-http-timeout)
- [Related Design Elements](#related-design-elements)

<!-- /toc -->

**Date**: 2026-02-04

**Status**: accepted

**Review**: Revisit if graceful shutdown semantics are needed for streaming.

**ID**: `cpt-cf-chat-engine-adr-streaming-cancellation`

## Context and Problem Statement

Users may want to stop assistant responses mid-generation (too slow, wrong direction, changing question). How should clients cancel ongoing streaming responses to save compute resources and provide responsive "stop" button UX?

## Decision Drivers

* User control over generation (stop button in UI)
* Compute resource conservation (cancel backend processing)
* Partial response preservation (save incomplete response)
* Responsive cancellation (immediate UI feedback)
* Simple cancellation mechanism
* Backend cleanup (cancel backend request)
* Database persistence of partial responses

## Considered Options

* **Option 1: Close HTTP connection** - Abort HTTP request to cancel stream
* **Option 2: HTTP DELETE request** - Separate HTTP endpoint to cancel by message_id
* **Option 3: HTTP timeout** - Set aggressive timeout to limit long operations

## Decision Outcome

Chosen option: "Close HTTP connection", because it provides immediate cancellation by aborting the HTTP request (using AbortController in browsers or request cancellation in other clients), saves backend resources, preserves partial responses with is_complete=false flag, aligns with standard HTTP patterns, and requires no separate cancellation endpoint.

### Consequences

* Good, because standard HTTP cancellation pattern (AbortController)
* Good, because immediate resource cleanup (no lingering connections)
* Good, because simple client implementation (abort request)
* Good, because backend detects disconnection immediately
* Good, because partial response preserved with is_complete=false flag
* Good, because no separate cancellation endpoint needed
* Bad, because connection close terminates stream (by design)
* Bad, because no explicit acknowledgment (implicit via disconnection)
* Bad, because backend must handle connection close gracefully

### Confirmation

Confirmed by testing client-side connection close behavior across HTTP clients.

## Pros and Cons of the Options

### Option 1: Close HTTP connection

Client aborts the HTTP request to cancel the stream. Uses AbortController in browsers or request cancellation in other clients.

* Good, because cancellation is immediate — no round-trip needed, the connection is simply dropped
* Good, because standard HTTP pattern familiar to all developers (AbortController, request.abort())
* Good, because no separate cancellation endpoint required, reducing API surface
* Good, because server detects disconnection and can clean up resources promptly
* Bad, because no explicit cancellation acknowledgment from the server (cancellation is implicit)
* Bad, because backend must handle abrupt connection close gracefully to avoid partial state corruption
* Bad, because network-level disconnects are indistinguishable from intentional cancellations

### Option 2: HTTP DELETE request

Client sends a separate DELETE request to a cancellation endpoint (e.g., DELETE /api/v1/messages/{id}/stream) to stop generation.

* Good, because cancellation is an explicit API call with a clear response confirming the action
* Good, because the original streaming connection remains open for a final status or summary event
* Good, because cancellation intent is unambiguous and distinguishable from network failures
* Good, because cancellation can include metadata (reason, partial response handling instructions)
* Bad, because requires a separate endpoint, increasing API surface and implementation complexity
* Bad, because race condition between cancellation request and stream completion requires careful handling
* Bad, because two concurrent requests (streaming + cancel) increase client complexity and connection usage
* Bad, because cancellation latency depends on a full HTTP round-trip rather than immediate connection close

### Option 3: HTTP timeout

Aggressive timeouts are configured to automatically terminate long-running streaming operations.

* Good, because simplest implementation — no client-side cancellation logic needed
* Good, because provides a safety net against runaway or stuck backend responses
* Good, because timeout behavior is consistent and predictable across all clients
* Bad, because timeout is indiscriminate — it cannot distinguish between a legitimately long response and one the user wants to cancel
* Bad, because users have no control over when cancellation happens (no "stop" button functionality)
* Bad, because choosing the right timeout value is difficult — too short cuts off valid responses, too long wastes resources
* Bad, because partial response preservation depends on where the timeout is enforced (client, proxy, or server)

## Related Design Elements

**Actors**:
* `cpt-cf-chat-engine-actor-client` - Closes HTTP connection on user action
* `cpt-cf-chat-engine-component-response-streaming` - Detects connection close, saves partial response
* `cpt-cf-chat-engine-component-webhook-integration` - Cancels HTTP request to backend

**Requirements**:
* `cpt-cf-chat-engine-fr-stop-streaming` - Cancel streaming, save partial response with incomplete flag
* `cpt-cf-chat-engine-nfr-streaming` - Minimal latency for cancellation response

**Design Elements**:
* `cpt-cf-chat-engine-design-entity-message` - is_complete field indicates cancelled messages
* HTTP connection close mechanism (Section 3.3.1 of DESIGN.md)
* Sequence diagram S11 (Stop Streaming Response)

**Related ADRs**:
* ADR-0003 (Streaming Architecture) - Depends on this for complete streaming lifecycle
* ADR-0006 (HTTP Client Protocol) - HTTP streaming client protocol with cancellation
