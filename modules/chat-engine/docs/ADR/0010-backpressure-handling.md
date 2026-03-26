Created:  2026-02-04 by Constructor Tech
Updated:  2026-03-06 by Constructor Tech
# ADR-0010: Streaming Backpressure with Buffer Limits


<!-- toc -->

- [Context and Problem Statement](#context-and-problem-statement)
- [Decision Drivers](#decision-drivers)
- [Considered Options](#considered-options)
- [Decision Outcome](#decision-outcome)
  - [Consequences](#consequences)
  - [Confirmation](#confirmation)
- [Pros and Cons of the Options](#pros-and-cons-of-the-options)
  - [Option 1: Per-stream buffer with limit and pause](#option-1-per-stream-buffer-with-limit-and-pause)
  - [Option 2: Unbounded buffering](#option-2-unbounded-buffering)
  - [Option 3: Drop chunks](#option-3-drop-chunks)
- [Related Design Elements](#related-design-elements)

<!-- /toc -->

**Date**: 2026-02-04

**Status**: accepted

**Review**: Revisit if backpressure causes client-visible latency

**ID**: `cpt-cf-chat-engine-adr-backpressure-handling`

## Context and Problem Statement

Webhook backends may stream responses faster than clients can consume (slow network, slow device rendering). How should Chat Engine handle backpressure to prevent memory exhaustion while maintaining streaming responsiveness?

## Decision Drivers

* Prevent memory exhaustion from unbounded buffering
* Support slow clients without blocking fast backends entirely
* Graceful handling when client cannot keep up
* HTTP/2 flow control for backend requests
* Per-stream buffer limits (not global)
* Client disconnect cancels backend request
* Minimal latency when client is fast
* Observable buffer metrics for monitoring

## Considered Options

* **Option 1: Per-stream buffer with limit and pause** - Buffer up to 10MB, pause backend via HTTP/2 flow control
* **Option 2: Unbounded buffering** - Buffer all chunks until client catches up
* **Option 3: Drop chunks** - Discard chunks when buffer full

## Decision Outcome

Chosen option: "Per-stream buffer with limit and pause", because it prevents memory exhaustion via 10MB buffer limit, uses HTTP/2 flow control to pause backend when buffer fills, supports slow clients within buffer limit, enables client disconnect to immediately cancel backend request, and maintains low latency for fast clients.

### Consequences

* Good, because memory usage bounded (10MB max per stream)
* Good, because backend paused via HTTP/2 flow control (not cancelled)
* Good, because slow clients supported within buffer limit
* Good, because client disconnect immediately cancels backend (saves resources)
* Good, because fast clients see minimal latency (no buffering)
* Good, because per-stream limits prevent one slow client affecting others
* Bad, because extremely slow clients may exhaust buffer (stream cancellation)
* Bad, because HTTP/2 flow control complexity (not all backends support)
* Bad, because buffer management adds overhead (~5% CPU)
* Bad, because no prioritization (all streams treated equally)

### Confirmation

Confirmed when per-stream buffers are capped at 10MB, HTTP/2 flow control pauses the backend on buffer full, and client disconnect immediately cancels the upstream request.

## Pros and Cons of the Options

### Option 1: Per-stream buffer with limit and pause

* Good, because memory usage is bounded per stream (10MB limit prevents exhaustion)
* Good, because HTTP/2 flow control pauses the backend gracefully instead of cancelling
* Good, because slow clients are supported within the buffer limit without data loss
* Good, because fast clients experience minimal latency (chunks forwarded immediately)
* Bad, because HTTP/2 flow control adds implementation complexity (not all backends support it)
* Bad, because extremely slow clients still exhaust the buffer, triggering stream cancellation
* Bad, because buffer management adds per-stream CPU overhead (~5%)

### Option 2: Unbounded buffering

* Good, because implementation is simple (no flow control, no buffer limits to manage)
* Good, because no data loss regardless of client speed (all chunks retained)
* Good, because backend never paused or cancelled, maintaining maximum throughput
* Bad, because memory usage grows without bound when clients are slower than backends
* Bad, because a single slow client can exhaust server memory, affecting all other streams
* Bad, because no backpressure signal to the backend wastes resources on data nobody consumes
* Bad, because makes capacity planning unpredictable (memory depends on client behavior)

### Option 3: Drop chunks

* Good, because memory usage stays bounded (discard when full, no accumulation)
* Good, because simple implementation (no flow control negotiation needed)
* Good, because backend is never paused, keeping backend interaction straightforward
* Bad, because dropped chunks cause data loss, corrupting the streamed response
* Bad, because client receives incomplete or garbled output with no way to recover mid-stream
* Bad, because requires complex reassembly logic or retransmission protocol on the client side
* Bad, because user experience degrades silently when chunks are dropped

## Related Design Elements

**Actors**:
* `cpt-cf-chat-engine-component-response-streaming` - Implements buffer and backpressure logic
* `cpt-cf-chat-engine-actor-backend-plugin` - Paused via HTTP/2 flow control
* `cpt-cf-chat-engine-actor-client` - Slow consumption triggers backpressure

**Requirements**:
* `cpt-cf-chat-engine-nfr-streaming` - Backpressure handling requirement
* `cpt-cf-chat-engine-fr-stop-streaming` - Client disconnect cancels backend

**Design Elements**:
* `cpt-cf-chat-engine-design-context-backpressure` - Implementation details (10MB limit, HTTP/2 flow control)
* `cpt-cf-chat-engine-component-response-streaming` - Buffer management per stream

**Related ADRs**:
* ADR-0003 (Streaming Architecture) - Streaming design depends on backpressure handling
* ADR-0008 (Client-Initiated Streaming Cancellation) - Client cancellation releases buffer
