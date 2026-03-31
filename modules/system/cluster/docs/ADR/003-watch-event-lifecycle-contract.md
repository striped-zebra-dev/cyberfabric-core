# ADR-003: Watch Event Lifecycle Contract for All Three Watches

**Status**: Accepted (originally 2026-04-02 for cache watch only; generalized 2026-04-27 to leader-election and service-discovery watches; lightweight-notifications principle folded in)

**ID**: `cpt-cf-clst-adr-watch-event-lifecycle-contract`

<!-- toc -->

- [Context and Problem Statement](#context-and-problem-statement)
- [Decision Drivers](#decision-drivers)
- [Considered Options](#considered-options)
- [Decision Outcome](#decision-outcome)
  - [Generalization to all three watches](#generalization-to-all-three-watches)
  - [Lightweight notifications: events carry no value payload](#lightweight-notifications-events-carry-no-value-payload)
  - [Shutdown sequence](#shutdown-sequence)
  - [Consequences](#consequences)
  - [Confirmation](#confirmation)
- [Pros and Cons of the Options](#pros-and-cons-of-the-options)
  - [Option 1: Result-based (Lagged and Reset as ClusterError variants)](#option-1-result-based-lagged-and-reset-as-clustererror-variants)
  - [Option 2: Union type `CacheWatchEvent` (CHOSEN)](#option-2-union-type-cachewatchevent-chosen)
  - [Option 3: Two traits (CacheWatch + ReliableCacheWatch)](#option-3-two-traits-cachewatch--reliablecachewatch)
- [More Information](#more-information)
  - [Good consumer pattern (idiomatic, correct)](#good-consumer-pattern-idiomatic-correct)
  - [Bad consumer pattern (would work with Option 1, broken with Option 2 — compile error)](#bad-consumer-pattern-would-work-with-option-1-broken-with-option-2--compile-error)
  - [Mapping per backend](#mapping-per-backend)
  - [References](#references)

<!-- /toc -->

## Context and Problem Statement

The `ClusterCache` trait exposes `watch(key)` and `watch_prefix(prefix)` returning a `CacheWatch`. The initial design has `CacheWatch::changed() -> CacheEvent` — infallible, with no signal for lag or subscription reset. This is a contract-level lie for any remote backend.

Five concrete scenarios expose the problem:

1. **Slow consumer**: The consumer processes events slowly while the provider receives a burst (e.g., 100 shard movements in 50 ms). The provider must buffer unboundedly (OOM), drop silently (silent correctness bug — consumer's state is stale), or block writers (unacceptable — unrelated writers get punished).

2. **Provider reconnect**: The provider's connection drops for 3 seconds and reconnects. Redis Pub/Sub is fire-and-forget (events lost), NATS KV tracks sequence numbers but needs explicit resumption, K8s watch returns `410 Gone` requiring relist. The consumer sees no signal that a gap occurred.

3. **Prefix watch on busy namespace**: `watch_prefix("session/")` on a cluster with 10,000 sessions. Fan-in easily saturates a slow consumer. The trait has no way to communicate "you're being throttled."

4. **Network partition**: Events queued server-side (etcd, NATS JetStream) flood the consumer on reconnect. Without a `Reset` signal, the consumer cannot decide between "process each event" vs "coalesce via a full re-read."

5. **Cluster restart**: Long-lived watch survives across a backend restart. The consumer either silently misses all events during the restart window or hangs forever. Nothing says "treat this as a cache miss and re-read."

All five scenarios share the same root: **watch is inherently unreliable across network and process boundaries, and the contract must say so explicitly**. Borrowing the "infallible stream of events" idiom from in-process pub/sub is misleading for distributed backends.

## Decision Drivers

- CyberFabric's cluster module is remote-only — every watcher crosses a network boundary.
- SDK-default backends (`CacheBasedServiceDiscovery`, `CasBasedLeaderElection`, `CasBasedDistributedLock` — see ADR-001) depend on watch. Silent lag or reset would silently break these primitives.
- Rust's `Result`-based error propagation via `?` is idiomatic — any signal that looks like an error will be propagated as one, which is wrong for transient lag signals.
- Every target backend has a native notion of "you missed events" (etcd compaction, K8s 410 Gone, NATS KV sequence gap, Postgres NOTIFY overflow marker, Redis Pub/Sub backpressure). The trait should expose this uniformly.
- Analogous Rust libraries (`tokio::sync::broadcast`, `kube-rs::watcher`) already solved this problem by surfacing lag and reset as first-class events.

## Considered Options

1. **`Result<CacheEvent, ClusterError>` with `Lagged` and `Reset` as error variants**: `changed()` returns `Result`; lag and reset are new `ClusterError` variants.
2. **Union type `CacheWatchEvent`**: `changed()` returns an enum with variants `Event`, `Lagged`, `Reset`, `Closed`. Infallible at the type level.
3. **Two traits**: `CacheWatch` (best-effort, infallible, may silently drop) + `ReliableCacheWatch` (at-least-once, with lag/reset signals).

## Decision Outcome

Chosen option: **Option 2** (union type `CacheWatchEvent`) with a terminal `Closed(ClusterError)` variant added for stream-end signaling.

```rust
enum CacheWatchEvent {
    Event(CacheEvent),
    Lagged { dropped: u64 },
    Reset,
    Closed(ClusterError),
}

impl CacheWatch {
    async fn changed(&mut self) -> CacheWatchEvent;
}
```

The consumer contract:

- `Event(CacheEvent)`: a real cache mutation. Consumer calls `cache.get(key)` if the value is needed.
- `Lagged { dropped }`: the watcher fell behind and `dropped` events were discarded. Consumer MUST treat all watched keys as potentially stale and re-read. `dropped: 0` means "unknown count, at least one" (for providers that cannot count).
- `Reset`: the subscription was re-established (reconnect, compaction, provider restart). Consumer MUST re-read all keys in the watch scope.
- `Closed(err)`: the stream ended with a terminal error. `CacheWatch` is no longer usable; consumer must call `watch()` again to continue observing.

### Generalization to all three watches

The same union shape applies to `LeaderWatchEvent` and `ServiceWatchEvent`. All three watches yield events of the form `{value-variant, Lagged{dropped}, Reset, Closed(err)}` and are infallible at the type level — there is no `Result`-returning `changed()` on any watch.

```rust
enum CacheWatchEvent {
    Event(CacheEvent),
    Lagged { dropped: u64 },
    Reset,
    Closed(ClusterError),
}

enum LeaderWatchEvent {
    Status(LeaderStatus),
    Lagged { dropped: u64 },
    Reset,
    Closed(ClusterError),
}

enum ServiceWatchEvent {
    Change(TopologyChange),
    Lagged { dropped: u64 },
    Reset,
    Closed(ClusterError),
}
```

Why generalize: the original cache-watch problem is not cache-specific. Every long-lived watch over a remote backend faces the same failure modes — slow consumer, provider reconnect, busy fan-in, network partition, backend restart. A `LeaderWatch` that silently misses the `Lost` transition is just as wrong as a cache watch that silently misses a `Changed` event; in fact more wrong, because the consumer may continue acting as leader. A `ServiceWatch` that silently misses a `Left` event causes consumers to keep routing to a deregistered instance.

Three watch types, one contract: same variants, same consumer obligations, same per-backend mapping. The only difference is the value-variant payload (`CacheEvent` vs `LeaderStatus` vs `TopologyChange`).

**Transient errors stay below the contract.** Backend-internal retryable errors (`ConnectionLost`, `Timeout`, `ResourceExhausted`) are retried by each watch's background task and do not surface as events. Only terminal errors arrive via `Closed(err)`. Consumers do not need to distinguish "is this connection error transient?" — the watch task does that.

### Lightweight notifications: events carry no value payload

`CacheEvent` carries only the key and the event kind (`Changed`, `Deleted`, `Expired`) — no value. Consumers call `cache.get(key)` for the current value when they need it. This is the contract twin of `Lagged` / `Reset`: consumers must re-read after any non-`Event` variant anyway, so events deliberately carry no value to avoid ever exposing a value that is older than the consumer's last `get()`.

Why no value:

- **Stale-value avoidance is structural, not advisory**. If `Event` carried a value, consumers would be tempted to use it directly — and after a `Lagged`/`Reset` interrupted the stream, that value could be stale. Removing the value field makes "always re-read" the only path.
- **Maps cleanly to every backend**. Redis keyspace notifications carry no value (only the key). Postgres `NOTIFY` has an 8KB payload limit that values frequently exceed. K8s watch returns the full object but consumers re-`get` anyway for consistency. Removing the value is the lowest-common-denominator that all backends support uniformly.
- **Fixed-size events**. The event channel can be an in-process `tokio::sync::broadcast` or equivalent without worrying about per-event payload size. Backpressure becomes a function of event rate alone, not value-size variance.
- **Composes with `Lagged`**. After a `Lagged { dropped: N }`, the consumer would need to re-read regardless of whether the dropped events had values — so values in non-dropped events would just be wasted bandwidth.

Reliable messaging with values, ordering guarantees, replay, and consumer groups belongs in the event broker, not in the cluster watch. Cluster watches are *change notifications*: "this thing changed; if you care, look up its current value."

### Shutdown sequence

`ClusterHandle::stop().await` (the wiring crate's lifecycle entry point — see ADR-006) delivers terminal watch events in a defined order before returning:

- For every active `LeaderWatch` currently in `Leader` state: `LeaderWatchEvent::Status(Lost)` synchronously, immediately followed by `LeaderWatchEvent::Closed(ClusterError::Shutdown)`. **Two distinct events at the type level.** `Status(Lost)` revokes the leader's confidence — any code path keying on `is_leader()` stops the moment the consumer reads the `Lost` transition. `Closed(Shutdown)` then ends the watch.
- For every active cache watch: `CacheWatchEvent::Closed(ClusterError::Shutdown)`.
- For every active service-discovery watch: `ServiceWatchEvent::Closed(ClusterError::Shutdown)`.

Why the leader two-step: a single `Closed(Shutdown)` event would tell the consumer the watch ended but would NOT separately signal "stop acting as leader." The consumer's leader-only work (e.g., a worker that runs only when leader) needs to see `Lost` before the watch closes, so the consumer can short-circuit any pending leader-only operations before observing shutdown. The two-step sequence makes leader confidence revocation explicit at the type level, not implicit in stream termination.

Why the union shape makes this clean: terminal errors are `Closed(err)`, a regular variant. The shutdown sequence above is just a defined emission order of regular variants — no special API required.

### Consequences

- Consumers explicitly handle lag and reset at the `match` site. The natural `watch.changed().await?` idiom does not compile (infallible return type), eliminating the common footgun of propagating transient signals as errors.
- Every provider must emit the four variants. Providers with native lag/reset signals (NATS, etcd, Postgres NOTIFY marker) map directly. Providers without (Redis keyspace notifications) synthesize signals from local state (broadcast channel overflow → Lagged; connection reset → Reset).
- Standalone plugin emits `Lagged` when its internal `tokio::sync::broadcast` channel overflows and `Closed(ClusterError::Shutdown)` on shutdown. `Reset` does not occur in standalone operation.
- The `CacheWatchEvent::Closed` variant is terminal. Providers MUST ensure no further items are yielded after `Closed`. Consumer loops that do not pattern-match `Closed` will spin forever; doc comments and example code must make this explicit.
- SDK-default sub-capabilities (`CasBasedLeaderElection`, `CacheBasedServiceDiscovery`) treat `Lagged` and `Reset` as "invalidate my cached state and re-read." This is the correct semantics — they already use `get()` after every event — but needs explicit handling.
- Metrics: providers SHOULD export a counter of `Lagged` events with the `dropped` sum, and a counter of `Reset` events. Excessive lag in production is a capacity-planning signal; excessive reset is a stability signal.

### Confirmation

- Unit tests verify each provider emits `Lagged`, `Reset`, and `Closed` variants under the expected conditions (broadcast overflow for Lagged; simulated reconnect for Reset; shutdown for Closed).
- Integration tests verify `CacheBasedServiceDiscovery` correctly invalidates its cached instance list on `Lagged` and `Reset` (i.e., re-reads via prefix `get`).
- A consumer using the idiomatic `watch.changed().await?` syntax does not compile — verified by a compile-fail test in the SDK.

## Pros and Cons of the Options

### Option 1: Result-based (Lagged and Reset as ClusterError variants)

- Good, because it fits the existing `ClusterError` enum and the `Result`-returning style used elsewhere in the trait.
- Good, because consumers that ignore the `Result` get a compile warning via `#[must_use]`.
- Bad, because the Rust `?` operator would propagate `Err(Lagged)` as a fatal error — but lag is not fatal, it's a transient signal. Consumers who write the natural idiom `let ev = watch.changed().await?;` silently convert transient lag into hard failures. This is a real, common footgun.
- Bad, because `ClusterError` is polluted with non-error variants. `Lagged` and `Reset` are not errors — they are normal watcher lifecycle events that every consumer will encounter.
- Bad, because distinguishing "transient lag, keep going" from "terminal connection lost, re-subscribe" requires matching on the full `ClusterError` hierarchy at every `changed()` call.
- Neutral, because `#[must_use]` annotations can mitigate some of the footgun risk — but cannot prevent `?` propagation, which is the core problem.

### Option 2: Union type `CacheWatchEvent` (CHOSEN)

- Good, because the type accurately describes what `changed()` produces: a cache event OR a lifecycle signal. No overloading of `Result` for control flow.
- Good, because the consumer is forced by the compiler to handle all four variants at the `match` site. Lag and reset cannot be silently ignored.
- Good, because `watch.changed().await?` does not compile — the `?` footgun is eliminated by construction.
- Good, because terminal errors are surfaced through `Closed(err)`, a first-class variant. The consumer sees "stream ended because X" in the same pattern-match as the happy path.
- Good, because it matches established Rust patterns — `tokio::sync::broadcast::Receiver::recv()` returns `Result<T, RecvError>` where `RecvError::Lagged(u64)` carries the count; `kube-rs::watcher::Event::Restarted(Vec<obj>)` carries a snapshot.
- Good, because every backend maps cleanly: NATS JetStream sequence gap → `Lagged`; etcd compaction / K8s 410 Gone → `Reset`; Postgres NOTIFY overflow marker → `Reset`; Redis Pub/Sub backpressure → `Lagged`; graceful shutdown → `Closed(ClusterError::Shutdown)`.
- Bad, because `changed()` is infallible at the type level, so terminal errors must be modeled explicitly via `Closed(err)`. This adds one variant but is structurally honest.
- Neutral, because the `dropped: u64` count is useful for metrics but not strictly necessary for correctness (the consumer's reaction to any non-zero lag is the same: re-read). Providers that cannot determine the count report zero.

### Option 3: Two traits (CacheWatch + ReliableCacheWatch)

- Good, because it lets consumers who don't care about lag use a simpler interface.
- Good, because providers that cannot implement reliable watch (e.g., Redis Pub/Sub under extreme backpressure) can explicitly not implement the reliable trait.
- Bad, because every SDK-default backend (per ADR-001) requires reliable watch semantics. `CacheBasedServiceDiscovery` silently missing events means silently wrong service discovery — exactly the failure mode we need to avoid.
- Bad, because it creates a two-tier system where the "correct" tier carries all implementation burden and the "easy" tier is a footgun by design. Consumers who "don't care about lag" are usually wrong about not caring — they would in fact care if they understood the failure modes.
- Bad, because the hybrid compositor must route sub-capabilities that require `ReliableCacheWatch` to providers that implement it, and silently or explicitly refuse to wire sub-capabilities that don't have a reliable cache. This is complexity on top of complexity.
- Bad, because it doubles the trait surface (`watch` and `watch_reliable`, `ReliableCacheWatch`, etc.) for dubious gain.

## More Information

### Good consumer pattern (idiomatic, correct)

```rust
let mut watch = cache.watch_prefix("event-broker/shard-").await?;
loop {
    match watch.changed().await {
        CacheWatchEvent::Event(CacheEvent::Changed { key }) => {
            let entry = cache.get(&key).await?;
            // Process the latest value.
        }
        CacheWatchEvent::Event(CacheEvent::Deleted { key }) => {
            remove_local_state(&key);
        }
        CacheWatchEvent::Event(CacheEvent::Expired { key }) => {
            remove_local_state(&key);
        }
        CacheWatchEvent::Lagged { dropped } => {
            tracing::warn!(dropped, "watch lagged; re-syncing all shard keys");
            resync_prefix(&cache, "event-broker/shard-").await?;
        }
        CacheWatchEvent::Reset => {
            tracing::info!("watch reset (reconnect/compaction); re-syncing");
            resync_prefix(&cache, "event-broker/shard-").await?;
        }
        CacheWatchEvent::Closed(err) => {
            tracing::error!(%err, "watch stream closed");
            return Err(err.into());
        }
    }
}
```

### Bad consumer pattern (would work with Option 1, broken with Option 2 — compile error)

```rust
// This does NOT compile with Option 2 (infallible changed()).
// This would silently treat Lagged as fatal with Option 1.
loop {
    let event = watch.changed().await?;
    handle_event(event).await?;
}
```

### Mapping per backend

| Backend | `Lagged` source | `Reset` source | `Closed` source |
|---|---|---|---|
| In-process (standalone) | `broadcast::RecvError::Lagged(n)` | (not emitted in normal op) | shutdown |
| Redis (keyspace notifications) | pub/sub buffer overflow → synthesize | connection reset → synthesize | AUTH error, connection pool exhausted |
| Postgres (LISTEN/NOTIFY) | NOTIFY queue overflow (empty-payload marker) → Lagged or Reset | connection reset | connection pool shutdown |
| K8s watch | (K8s does not lag — emits 410 Gone instead → Reset) | 410 Gone, resourceVersion expired | watch endpoint deleted |
| NATS KV watch | JetStream consumer sequence gap | consumer recreate, connection reset | bucket deleted, auth error |
| etcd watch | `WatchResponse.canceled` with `compacted` → Reset (not Lagged) | compaction, manual cancel | cluster shutdown, auth error |

### References

- [tokio::sync::broadcast::RecvError::Lagged](https://docs.rs/tokio/latest/tokio/sync/broadcast/enum.error.RecvError.html) — the inspiration for the `Lagged { dropped }` variant.
- [kube-rs::watcher::Event::Restarted](https://docs.rs/kube-runtime/latest/kube_runtime/watcher/enum.Event.html) — the inspiration for `Reset` as a first-class event.
- [etcd watch API — WatchResponse.canceled](https://etcd.io/docs/v3.5/learning/api/#watch-api) — reference for compaction-triggered reset.
- [Kubernetes API conventions — Efficient detection of changes](https://kubernetes.io/docs/reference/using-api/api-concepts/#efficient-detection-of-changes) — resourceVersion, 410 Gone, bookmarks.
- [PostgreSQL NOTIFY — Queue overflow](https://www.postgresql.org/docs/current/sql-notify.html) — documentation of the empty-payload recovery marker.
- [NATS JetStream KV watch](https://docs.nats.io/nats-concepts/jetstream/key-value-store) — consumer sequence numbers and reconnect.
- ADR-001 — the cache-CAS-universal model. SDK-default leader/lock/SD backends are built on `ClusterCacheBackend`, so they depend on the same watch contract this ADR establishes.
- ADR-006 — builder/handle lifecycle. The shutdown sequence above is implemented inside `ClusterHandle::stop()`.
