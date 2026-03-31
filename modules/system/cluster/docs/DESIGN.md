# Technical Design — Cluster


<!-- toc -->

- [1. Architecture Overview](#1-architecture-overview)
  - [1.1 Architectural Vision](#11-architectural-vision)
  - [1.2 Architecture Drivers](#12-architecture-drivers)
  - [1.3 Architecture Layers](#13-architecture-layers)
- [2. Principles & Constraints](#2-principles--constraints)
  - [2.1 Design Principles](#21-design-principles)
  - [2.2 Constraints](#22-constraints)
- [3. Technical Architecture](#3-technical-architecture)
  - [3.1 Domain Model](#31-domain-model)
  - [3.2 Component Model](#32-component-model)
  - [3.3 API Contracts](#33-api-contracts)
  - [3.4 Internal & External Dependencies](#34-internal--external-dependencies)
  - [3.5 Resolution Pattern](#35-resolution-pattern)
  - [3.6 Lifecycle Pattern (Builder/Handle)](#36-lifecycle-pattern-builderhandle)
  - [3.7 Per-primitive Scoping](#37-per-primitive-scoping)
  - [3.8 Watch Event Shape](#38-watch-event-shape)
  - [3.9 Capability Validation](#39-capability-validation)
  - [3.10 SDK Default Backends](#310-sdk-default-backends)
  - [3.11 Polyfill](#311-polyfill)
  - [3.12 Interactions & Sequences](#312-interactions--sequences)
  - [Database schemas & tables](#database-schemas--tables)
- [4. Additional Context](#4-additional-context)
  - [4.1 Backend Feature Compatibility](#41-backend-feature-compatibility)
  - [4.2 Recommended Deployment Combinations](#42-recommended-deployment-combinations)
  - [4.3 Existing Code Migration](#43-existing-code-migration)
- [5. Risks / Trade-offs](#5-risks--trade-offs)
- [6. Open Questions](#6-open-questions)

<!-- /toc -->

## 1. Architecture Overview

> **Open: backend authentication and credential wiring.** How cluster plugins (Redis, Postgres, K8s, NATS, etcd) acquire credentials for their backend connections is **not yet established** and is intentionally out of scope for this design. The shape (`secret_ref` on each backend config struct, resolved via the credstore plugin at start; K8s falling back to `kube-rs`'s in-cluster service-account / kubeconfig chain) is sketched but the concrete wiring, startup ordering, and per-backend mTLS/SASL/IAM specifics are deferred to the broader **OOP (out-of-process) deployment design**, where cluster meets the rest of the platform's credential and transport story (TLS termination, identity propagation, secret rotation). Treat any credential references below as placeholder shape, not committed contract.

### 1.1 Architectural Vision

Cluster is a platform-level system module that provides cluster coordination and shared-state primitives to all CyberFabric modules. It exposes four independent primitives — distributed cache (KV with TTL, version-based CAS, watch notifications), leader election, distributed locks with TTL-bounded mutual exclusion, and service discovery — each as a versioned public-API facade struct (`ClusterCacheV1`, `LeaderElectionV1`, `DistributedLockV1`, `ServiceDiscoveryV1`) wrapping a plugin-implemented backend trait (`ClusterCacheBackend`, `LeaderElectionBackend`, `DistributedLockBackend`, `ServiceDiscoveryBackend`). Plugins register their backend implementations in ClientHub per profile per primitive; consumers resolve via per-primitive fluent resolvers.

The architecture follows the ModKit Gateway + Plugins pattern (same as authn-resolver, authz-resolver, credstore, tenant-resolver). An SDK crate (`cf-cluster-sdk`) defines the facade structs, backend traits, and resolver builders. A wiring crate (`cf-cluster`, planned follow-up change) handles ClientHub registration and plugin orchestration via the outbox-style builder/handle pattern. Backend-specific implementations ship as plugin crates (also follow-up changes) under `plugins/`.

The key architectural differentiator is **per-primitive backend routing as operator config**. Each profile in platform YAML maps each primitive to a specific plugin's backend impl independently. Operators can run Redis for cache, K8s Lease for leader election, and K8s Lease (per instance) for service discovery — all in the same profile, registered side-by-side in ClientHub under that profile's scope. There is no runtime compositor object; the wiring crate iterates the config and registers each backend independently.

The SDK also ships **default backend implementations** of leader election, distributed lock, and service discovery built entirely on `ClusterCacheBackend` CAS operations. This means a minimal plugin only needs to implement the cache backend trait — the SDK builds the other three on top. Native plugin backends override the defaults when a backend excels (e.g., K8s Lease for elections). Operators opt into SDK defaults by **omitting** the primitive in YAML; explicit binding always wins.

Lifecycle is owned by a parent host module via the **outbox-style builder/handle pattern**. The wiring crate is NOT registered as its own `RunnableCapability` — it's a library exposing `ClusterWiring::builder(...).build_and_start() -> ClusterHandle`. The parent host module's `RunnableCapability::start` calls `build_and_start()`; its `RunnableCapability::stop` calls `handle.stop()`. Plugins are nested builder/handle pairs owned by the cluster handle, NOT separate `RunnableCapability` implementors. Code-flow ordering inside the parent module's `start` removes the need for a framework-level dependency mechanism between wiring and plugin lifecycles.

Explicit pub/sub messaging is excluded. The event broker module provides reliable pub/sub with delivery guarantees, consumer groups, offsets, and replay. The cluster provides reactive cache notifications (watch by key or prefix) for data-change observation — "this data changed" vs "deliver this message reliably".

### 1.2 Architecture Drivers

#### Functional Drivers

| Requirement | Design Response |
|-------------|-----------------|
| Cluster-wide shared state for modules | `ClusterCacheV1` with version-based CAS, TTL, and watch notifications |
| Worker pool coordination (event broker, schedulers) | `LeaderElectionV1` with watch-based status model and automatic renewal |
| Distributed rate limiting (OAGW) | `DistributedLockV1` with TTL and explicit async release |
| OOP module-to-module routing with dynamic shard ownership | `ServiceDiscoveryV1` with state-filtered and metadata-filtered instance listing (e.g., dispatcher → delivery instance by `topic-shard`) and topology watches |
| Multiple infrastructure backends per profile | Per-primitive backend routing as operator config; per-primitive ClientHub registration; no runtime compositor |
| Zero-infrastructure dev/test | SDK ships with in-process stub backends for smoke tests; production standalone plugin is a follow-up change |

#### Architecture Decision Records

| ADR | Summary |
|-----|---------|
| `cpt-cf-clst-adr-provider-compat-perf` (ADR-001) | Provider compatibility and performance analysis — per-primitive routing as operator config, per-backend characteristics, prefix-based routing, subscriber leases as cache not locks |
| `cpt-cf-clst-adr-async-boundary-no-remote-critical` (ADR-002) | Async boundary and no remote I/O in critical sections — no-op `Drop` with explicit async release, fencing tokens removed from public API, dylint enforcement (cluster-trait-scoped) |
| `cpt-cf-clst-adr-watch-event-lifecycle-contract` (ADR-003) | Watch event lifecycle contract for all three watches — union-type `*WatchEvent { value-variant, Lagged, Reset, Closed }` instead of `Result`-based signaling, applied to cache, leader, and service-discovery watches; lightweight key-only cache events as the contract twin of `Lagged`/`Reset` |
| `cpt-cf-clst-adr-observability-contract` (ADR-004) | Observability as a versioned naming contract — spans, metrics, log events are part of the SDK contract; cardinality rule forbids keys/names as metric labels |
| `cpt-cf-clst-adr-facade-backend-pattern` (ADR-005) | Per-primitive facade-plus-backend-trait pattern, per-primitive `*V1` versioning, no root `Cluster` trait |
| `cpt-cf-clst-adr-builder-handle-lifecycle` (ADR-006) | Outbox-style builder/handle lifecycle owned by parent host module, no two-tier `RunnableCapability` ordering |
| `cpt-cf-clst-adr-capability-typing-and-profile-resolution` (ADR-007) | Per-primitive capability typing — `*Capability` enums replace bundled `CapabilityClass`; consequences: `ClusterProfile` typed marker, fluent resolver, capability-mismatch fails startup |
| `cpt-cf-clst-adr-sd-state-is-intent-not-health` (ADR-008) | Service discovery: `state` is module-declared serving intent (`Enabled`/`Disabled`), NOT a health observation; cluster does not own liveness probing |
| `cpt-cf-clst-adr-leader-election-backend-safety` (ADR-009) | Per-backend correctness analysis for SDK-default leader election (and lock) under failure; constructor pair `new` (rejects `EventuallyConsistent`) + `new_allow_weak_consistency` (opt-in with warning); promotes the r2 deep-dive to decision-of-record |

#### NFR Allocation

| NFR Summary | Allocated To | Design Response | Verification Approach |
|-------------|--------------|-----------------|----------------------|
| At most one leader per election name (when bound to `Linearizable` cache) | All backends + SDK defaults | Trait contract enforces single-leader guarantee; capability validation rejects `EventuallyConsistent` cache without explicit opt-in | Multi-task contention smoke tests against `MemCacheBackend`; per-backend integration tests in plugin follow-ups |
| Bounded lock holding (no stale writers) | Consumers + dylint rule | Async + timeouts bound critical section; dylint forbids remote I/O inside `try_lock`/`release` scopes (lint scope is initially restricted to the four cluster backend traits; DB-tx enforcement is a follow-up rule extension) | Dylint rule check; smoke tests for lock release-on-timeout |
| No serde in contract types | SDK crate | Dylint layer rules enforce no serde in trait definitions | `make check` (dylint lints) |
| Watch event delivery — at-most-once with per-key ordering and lifecycle signals | All backends | Union-type events (`*WatchEvent`) carry `Lagged{dropped}`, `Reset`, `Closed(err)` so consumers recover from missed events explicitly | Smoke tests across all three watches verifying each variant is observable |
| Backend trait dyn-compatibility | SDK crate | Compile-time assertions (`fn _assert_dyn_compat(_: Arc<dyn _Backend>) {}`) per trait | Build fails if dyn-compat is broken |

### 1.3 Architecture Layers

```
┌─────────────────────────────────────────────────────────────────┐
│            Consumers (Event Broker, OAGW, modules)              │
│  Hold ClusterCacheV1 / LeaderElectionV1 / DistributedLockV1 /   │
│  ServiceDiscoveryV1 facades. Define ClusterProfile markers.     │
├─────────────────────────────────────────────────────────────────┤
│  Parent host module (this change: out of scope; future)         │
│  Owns ClusterHandle from RunnableCapability::start/stop.        │
├─────────────────────────────────────────────────────────────────┤
│  cf-cluster-sdk (THIS CHANGE)                                   │
│  Facade structs, backend traits, resolver builders, profile     │
│  marker, *Capability and *Features enums/structs, SDK default   │
│  backends, scoping helpers, polyfill, shared types.             │
├─────────────────────────────────────────────────────────────────┤
│  cf-cluster wiring (follow-up change)                           │
│  ClusterWiring::builder().build_and_start() -> ClusterHandle.   │
│  Reads operator YAML; instantiates plugins; registers each      │
│  Arc<dyn _Backend> per profile per primitive in ClientHub.      │
├─────────────────────────────────────────────────────────────────┤
│  Plugin crates (follow-up changes)                              │
│  ┌────────────────┐ ┌──────────────┐ ┌────────────────┐         │
│  │ standalone     │ │ postgres     │ │ k8s            │  ...    │
│  │ (in-process)   │ │ (CRD+L/N)    │ │ (Lease+CRD)    │         │
│  └────────────────┘ └──────────────┘ └────────────────┘         │
│  Each plugin: builder/handle pair (outbox pattern).             │
├─────────────────────────────────────────────────────────────────┤
│  External (out of all change scopes)                            │
│  PostgreSQL, K8s API, Redis, NATS, etcd                         │
└─────────────────────────────────────────────────────────────────┘
```

| Layer | Responsibility | Technology |
|-------|---------------|------------|
| SDK | Public-API facade structs (`*V1`), backend traits (`*Backend`), per-primitive resolver builders, `ClusterProfile` marker trait, `*Capability` requirement enums, `*Features` characteristic structs, shared types, SDK default backend implementations, per-primitive `scoped()` helpers, `PollingPrefixWatch` polyfill, `register_*_backend` / `deregister_*_backend` helpers | Rust crate (`cf-cluster-sdk`) |
| Wiring (follow-up) | Operator YAML parsing, plugin orchestration, per-primitive ClientHub registration, builder/handle exposed to parent host module | Rust crate (`cf-cluster`) |
| Plugins (follow-up) | Backend-specific primitive implementations exposed as builder/handle pairs | Rust crates per backend |
| External | Persistence, coordination, cluster state | PostgreSQL, K8s API server, Redis, NATS, etcd |

## 2. Principles & Constraints

### 2.1 Design Principles

#### Cache CAS as Universal Building Block

- [ ] `p1` - **ID**: `cpt-cf-clst-principle-cas-universal`

`ClusterCacheBackend` with version-based CAS is the foundational primitive. Leader election, distributed locks, and service discovery can all be built on top of cache CAS + watch. The SDK ships default backend implementations of all three using only cache operations. This means a minimal plugin needs to implement only `ClusterCacheBackend` to get all four primitives (the wiring crate auto-wraps the cache backend in the SDK defaults when a primitive is omitted in operator config). Native overrides improve performance but are never required.

#### Per-primitive Routing as Operator Config

- [ ] `p1` - **ID**: `cpt-cf-clst-principle-per-primitive-routing`

Each primitive routes independently to the best backend for the job. The wiring crate's `ClusterWiring::builder(...).build_and_start()` reads each profile's per-primitive config and registers the corresponding `Arc<dyn _Backend>` in ClientHub under the profile scope. Mixed backends within one profile (Redis cache + K8s Lease for leader election) are the common case, supported directly by the per-primitive registration model. There is no runtime compositor object — registration is per-primitive and the wiring crate is a thin iterator over operator config.

#### Facade-plus-Backend-Trait Pattern

- [ ] `p1` - **ID**: `cpt-cf-clst-principle-facade-plus-backend-trait`

There is no root `Cluster` trait. Each primitive is split into a public-API facade struct (`ClusterCacheV1`) and a plugin-facing backend trait (`ClusterCacheBackend`). Consumers hold the facade — a cheap-clone Arc-backed struct with inherent async methods. Plugins implement the backend trait. This keeps consumers off the `dyn` surface, lets the public API evolve independently of the plugin contract, and gives consumers a clean fluent resolver entry point: `ClusterCacheV1::resolver(hub).profile(P).require(...).resolve()`. Per-primitive versioning (`*V1`, `*V2`) allows incompatible primitive changes to coexist via separate `TypeKey`/ClientHub registration.

#### Lightweight Notifications, Not Messaging

- [ ] `p1` - **ID**: `cpt-cf-clst-principle-lightweight-notifications`

Cache watch events carry only the key and event type (`Changed`, `Deleted`, `Expired`) — no value payload. Consumers call `cache.get(key)` for the current value. This avoids stale-value issues, maps cleanly to all backends (Redis keyspace notifications carry no value, Postgres NOTIFY has 8KB limit), and keeps events fixed-size. Reliable messaging belongs in the event broker.

#### Version-Based Optimistic Concurrency

- [ ] `p1` - **ID**: `cpt-cf-clst-principle-version-based-cas`

`compare_and_swap` takes an `expected_version: u64` obtained from a prior `get()`, not an expected byte value. `get()` returns `CacheEntry { value, version }`. This maps natively to all backends: `resourceVersion` (K8s), `revision` (NATS), `mod_revision` (etcd), `BIGSERIAL` (Postgres), Lua counter (Redis), `AtomicU64` (in-process). Value-based CAS would require racy get-compare-put loops on revision-based backends.

#### Watch Union Shape Across All Three Watches

- [ ] `p1` - **ID**: `cpt-cf-clst-principle-watch-union-shape`

All three watch event types (`CacheWatchEvent`, `LeaderWatchEvent`, `ServiceWatchEvent`) follow the same union shape: `{value-variant, Lagged{dropped}, Reset, Closed(err)}`. Infallible at the type level — there is no `Result`-returning `changed()` method on any watch. Terminal errors arrive via `Closed(err)`. Transient backend errors (`ConnectionLost`, `Timeout`, `ResourceExhausted`) are retried internally by the watch's background task and do not surface as events. ADR-003 captures the rationale and applies to all three watches.

### 2.2 Constraints

#### No Serde in Contract Types

- [ ] `p1` - **ID**: `cpt-cf-clst-constraint-no-serde`

The `cf-cluster-sdk` crate MUST NOT depend on serde. Serialization concerns belong in plugin implementations. Enforced by dylint lints in the workspace.

#### No Remote I/O in Cluster Critical Sections

- [ ] `p1` - **ID**: `cpt-cf-clst-constraint-no-remote-in-critical-section`

Code protected by a `LockGuard` MUST NOT make additional remote calls. Remote effects MUST occur before `try_lock` or after `release`, never between them. Together with async + timeouts, this eliminates the Kleppmann fencing scenario at the architectural level. Enforced by a workspace dylint rule scoped to the four cluster backend traits within `try_lock`/`release` scopes; DB-tx enforcement is a follow-up rule extension once the wiring crate and consumer migrations land. See ADR-002.

#### Backend Trait Dyn-Compatibility

- [ ] `p1` - **ID**: `cpt-cf-clst-constraint-dyn-compat`

All four backend traits MUST be dyn-compatible. The SDK includes compile-time assertions per trait so any future change that breaks dyn-compatibility fails the build. No `Self: Sized` bounds on async trait methods; no GATs.

## 3. Technical Architecture

### 3.1 Domain Model

| Entity | Description |
|--------|-------------|
| `ClusterCacheV1` | Public-API facade struct; cheap-clone (Arc-backed) wrapper over `Arc<dyn ClusterCacheBackend>`. Inherent async methods: `get`, `put`, `delete`, `contains`, `put_if_absent`, `compare_and_swap`, `watch`, `watch_prefix`. Inherent sync: `consistency()`, `features()`, `resolver(hub)`, `scoped(prefix)`. |
| `LeaderElectionV1` | Public-API facade struct over `Arc<dyn LeaderElectionBackend>`. Inherent async: `elect`, `elect_with_config`. Inherent sync: `resolver(hub)`, `scoped(prefix)`. |
| `DistributedLockV1` | Public-API facade struct over `Arc<dyn DistributedLockBackend>`. Inherent async: `try_lock`, `lock`. Inherent sync: `resolver(hub)`, `scoped(prefix)`. |
| `ServiceDiscoveryV1` | Public-API facade struct over `Arc<dyn ServiceDiscoveryBackend>`. Inherent async: `register`, `discover`, `watch`. Inherent sync: `resolver(hub)`, `scoped(prefix)`. |
| `ClusterCacheBackend` | Plugin-facing async trait. Methods: `consistency()`, `features()`, `get`, `put`, `delete`, `contains`, `put_if_absent`, `compare_and_swap`, `watch`, `watch_prefix`. |
| `LeaderElectionBackend` | Plugin-facing async trait. Methods: `features() -> LeaderElectionFeatures`, `elect`, `elect_with_config`. |
| `DistributedLockBackend` | Plugin-facing async trait. Methods: `features() -> LockFeatures`, `try_lock`, `lock`. |
| `ServiceDiscoveryBackend` | Plugin-facing async trait. Methods: `features() -> ServiceDiscoveryFeatures`, `register`, `discover`, `watch`. |
| `ClusterProfile` | Marker trait: `pub trait ClusterProfile: 'static + Send + Sync + Copy { const NAME: &'static str; }`. Consumer crates impl this on a ZST struct once per profile; the `NAME` is the only place the profile string lives on the consumer side. |
| `CacheCapability` | `#[non_exhaustive] enum { Linearizable, PrefixWatch }`. Per-primitive requirement enum used at resolver call sites. |
| `LeaderElectionCapability` | `#[non_exhaustive] enum { Linearizable }`. |
| `LockCapability` | `#[non_exhaustive] enum { Linearizable }`. |
| `ServiceDiscoveryCapability` | `#[non_exhaustive] enum { MetadataFiltering }`. |
| `CacheFeatures` | `#[non_exhaustive] struct { prefix_watch: bool, ... }`. Backend declares native capability availability. |
| `LeaderElectionFeatures` | `#[non_exhaustive] struct { linearizable: bool, ... }`. |
| `LockFeatures` | `#[non_exhaustive] struct { linearizable: bool, ... }`. |
| `ServiceDiscoveryFeatures` | `#[non_exhaustive] struct { metadata_pushdown: bool, ... }`. |
| `*ResolverBuilder<'a>` | Per-primitive fluent builder: `.profile<P: ClusterProfile>(_: P)`, `.require(cap: *Capability)`, `.resolve() -> Result<*V1, ClusterError>`. |
| `CacheConsistency` | `enum { Linearizable, EventuallyConsistent }`. Cache-only — leader election and lock backends use `*Features { linearizable: bool }` instead. |
| `CacheEntry` | Versioned key-value pair: `{ value: Vec<u8>, version: u64 }`. Version is opaque, monotonically increasing per key, starting at 1. Version 0 is reserved as sentinel. |
| `CacheEvent` | Lightweight notification: `Changed { key }`, `Deleted { key }`, `Expired { key }`. No payload — consumer calls `get(key)` for current value. |
| `CacheWatchEvent` | Watch union: `Event(CacheEvent)`, `Lagged { dropped: u64 }`, `Reset`, `Closed(ClusterError)`. Per ADR-003. |
| `CacheWatch` | Async receiver yielding `CacheWatchEvent` items. Dropping unsubscribes. Per-key ordering guaranteed; no cross-key ordering. |
| `LeaderStatus` | `enum { Leader, Follower, Lost }`. `Lost` is a transient observable transition — the watch auto-reenrolls and the next `Status` event resolves to `Leader` or `Follower`. Not terminal. |
| `LeaderWatchEvent` | Watch union: `Status(LeaderStatus)`, `Lagged { dropped: u64 }`, `Reset`, `Closed(ClusterError)`. |
| `LeaderWatch` | Handle into an ongoing election. `async fn changed() -> LeaderWatchEvent`; `fn status() -> LeaderStatus`; `fn is_leader() -> bool`; `async fn resign(self) -> Result<()>`. `Drop` is a no-op (no I/O in `Drop`). |
| `ElectionConfig` | `{ ttl: Duration (default 30s), max_missed_renewals: u8 (default 2) }`. Constructor `new(ttl, max_missed_renewals)` validates both > 0. Derived: `renewal_interval() = ttl / (max_missed_renewals + 1)`. |
| `LockGuard` | Lock handle. `async fn extend(additional_ttl)`, `async fn release(self)`. `Drop` is a no-op (TTL is the safety net; no I/O in `Drop`). |
| `ServiceRegistration` | `{ name: String, instance_id: Option<String>, address: String, metadata: HashMap<String, String> }`. |
| `ServiceInstance` | Discovered instance: `{ instance_id, address, metadata, state: InstanceState, registered_at }`. |
| `InstanceState` | `enum { Enabled, Disabled }`. Module-declared serving intent. NOT a health observation — liveness comes from heartbeat/TTL renewal. |
| `ServiceHandle` | Registration handle: `async fn deregister(self)`, `async fn update_metadata(...)`, `async fn set_enabled(bool)`. `Drop` is a no-op (no I/O in `Drop`). |
| `MetaMatch` | `#[non_exhaustive] enum { Equals(String), OneOf(Vec<String>) }`. Per-key metadata predicate. |
| `DiscoveryFilter` | `#[non_exhaustive] struct { state: StateFilter, metadata: Vec<(String, MetaMatch)>, ... }`. AND-conjunction across metadata entries. |
| `StateFilter` | `enum { Enabled, Disabled, Any }`. Default `Enabled` (primary routing). |
| `TopologyChange` | `Joined(ServiceInstance)`, `Left(instance_id)`, `Updated(ServiceInstance)`. |
| `ServiceWatchEvent` | Watch union: `Change(TopologyChange)`, `Lagged { dropped: u64 }`, `Reset`, `Closed(ClusterError)`. |
| `ServiceWatch` | Async receiver yielding `ServiceWatchEvent` items. |
| `ClusterError` | Unified error enum. Variants: `InvalidName { name, reason }`, `InvalidConfig { reason }`, `LockContended { name }`, `LockTimeout { name, waited }`, `LockExpired { name }`, `CasConflict { key, current: Option<CacheEntry> }`, `Unsupported { feature: &'static str }`, `ProfileNotSpecified`, `ProfileNotBound { profile: &'static str }`, `CapabilityNotMet { primitive: &'static str, capability: &'static str, provider: &'static str }`, `Shutdown`, `Provider { kind: ProviderErrorKind, message: String, source: Option<Box<dyn Error + Send + Sync>> }`. **No `NotStarted` variant** — pre-resolution access surfaces as `ProfileNotBound` (the resolver enforces presence at consumer construction time, so resolved facades cannot observe a "not started" state). |
| `ProviderErrorKind` | `enum { ConnectionLost, Timeout, AuthFailure, ResourceExhausted, Other }`. Programmatic retryability classification. |
| `ScopedCacheBackend` (and three siblings) | Internal SDK wrapper struct implementing the corresponding `*Backend` trait by delegating to an inner `Arc<dyn _Backend>` with prefix translation. Returned by `*V1::scoped(prefix)`. |
| `PollingPrefixWatch` | SDK polyfill: synthesizes `watch_prefix` behavior on backends declaring `features().prefix_watch == false` by periodically listing the prefix and emitting `CacheWatchEvent::Event` diffs (Changed/Deleted). Explicit opt-in; doc comments describe the cost (N gets per interval). |
| `ClusterWiring` (follow-up) | Wiring crate's builder entry point. `ClusterWiring::builder(config, hub).build_and_start() -> ClusterHandle`. |
| `ClusterHandle` (follow-up) | Wiring crate's lifecycle handle. `handle.stop() -> ()` deregisters all backends and stops nested plugin handles. Owned by the parent host module. |

**Relationships**:
- A `CacheEntry` belongs to exactly one key. Each `put` increments the version.
- A `LeaderWatch` belongs to one election name. At most one `LeaderWatch` across all nodes observes `Leader` (advisory — see staleness bound in §3.3).
- A `LockGuard` belongs to one lock name. Mutual exclusion is bounded by TTL; explicit `release().await` is the idiomatic release path. Consumers MUST NOT make remote I/O calls inside the critical section (see §2 Constraints).
- A `ServiceHandle` belongs to one service registration. Each service name can have multiple instances.
- A `ClusterCacheV1` is `Arc<dyn ClusterCacheBackend>`-backed; cloning the facade is a single atomic increment.

### 3.2 Component Model

```
┌────────────────────────────────────────────────────────────────────┐
│                          cf-cluster-sdk                            │
│  ┌──────────────────┐ ┌──────────────────┐ ┌──────────────────┐    │
│  │ ClusterCacheV1   │ │LeaderElectionV1  │ │ DistributedLockV1│    │
│  │ + CacheBackend   │ │ + LEBackend      │ │ + LockBackend    │    │
│  └──────────────────┘ └──────────────────┘ └──────────────────┘    │
│  ┌──────────────────┐ ┌─────────────────────────────────────────┐  │
│  │ServiceDiscoveryV1│ │ Resolver builders (one per primitive)   │  │
│  │ + SDBackend      │ │ ClusterProfile marker, *Capability,     │  │
│  └──────────────────┘ │ *Features, ClusterError, shared types   │  │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │ SDK default backends (cas-based LE, lock; cache-based SD)   │   │
│  ├─────────────────────────────────────────────────────────────┤   │
│  │ Per-primitive Scoped*Backend wrappers                       │   │
│  │ PollingPrefixWatch polyfill                                 │   │
│  │ register_*_backend / deregister_*_backend helpers           │   │
│  └─────────────────────────────────────────────────────────────┘   │
└────────────────────────────────────────────────────────────────────┘
                                   ▲
                                   │ Arc<dyn _Backend> registered per primitive per profile
                                   │
┌────────────────────────────────────────────────────────────────────┐
│                       cf-cluster (follow-up change)                │
│  ClusterWiring::builder(config, hub).build_and_start() →           │
│       ClusterHandle (owns nested plugin handles)                   │
│  Reads operator YAML; iterates profile×primitive matrix;           │
│  starts each plugin's builder; registers each backend in ClientHub │
└────────────────────────────────────────────────────────────────────┘
                                   ▲
                                   │ owned by parent host module's RunnableCapability::start
                                   │
┌────────────────────────────────────────────────────────────────────┐
│             Plugin crates (each follow-up change)                  │
│  cf-standalone-cluster-plugin / cf-postgres-cluster-plugin /       │
│  cf-k8s-cluster-plugin / cf-cluster-redis / cf-cluster-nats / ...  │
│  Each: builder/handle pair (outbox pattern)                        │
└────────────────────────────────────────────────────────────────────┘
```

#### cf-cluster-sdk (this change)

- [ ] `p1` - **ID**: `cpt-cf-clst-component-sdk`

Per-primitive public-API facade structs, plugin-facing backend traits, resolver builders, profile marker, capability and features types, shared types, SDK default backend implementations, scoping wrappers, polyfill, registration/deregistration helpers, name validation utilities. Zero external dependencies beyond `tokio`, `tokio_util`, `async-trait`, and platform crates (`modkit`, `gts`, `types-registry-sdk`).

#### cf-cluster wiring (follow-up change)

- [ ] `p1` - **ID**: `cpt-cf-clst-component-wiring`

Wiring library. Implements no `RunnableCapability` itself. Exposes `ClusterWiring::builder(config, hub).build_and_start() -> ClusterHandle`. The handle's `stop()` is the single shutdown entry point. A parent host module owns the handle from inside its own `RunnableCapability::start`/`stop`.

#### Plugin crates (follow-up changes)

- [ ] `p1` - **ID**: `cpt-cf-clst-component-plugins`

Each plugin (Postgres, K8s, Redis, NATS, etcd, standalone) exposes a builder/handle pair (`MyCachePlugin::builder(...).build_and_start() -> MyCacheHandle`), with the handle's `stop()` cancelling internal `CancellationToken`s and joining background tasks (TTL reapers, renewal loops, watch fan-out). The wiring crate composes these into the cluster handle.

### 3.3 API Contracts

#### ClusterCacheV1 — Cache primitive

| Method | Signature | Contract |
|--------|-----------|----------|
| `resolver` | `fn resolver(hub: &ClientHub) -> CacheResolverBuilder<'_>` | Static entry point. Returns a fluent builder. |
| `consistency` | `fn consistency(&self) -> CacheConsistency` | Surfaces backend's declared consistency class. |
| `features` | `fn features(&self) -> CacheFeatures` | Surfaces backend's native capability flags. |
| `scoped` | `fn scoped(&self, prefix: &str) -> ClusterCacheV1` | Returns a scoped wrapper that prepends `prefix + "/"` on the write path and strips it on the read path. Validates prefix per the cluster name rule. |
| `get` | `async fn get(&self, key: &str) -> Result<Option<CacheEntry>, ClusterError>` | Returns versioned entry or `None`. Never errors for missing keys. |
| `put` | `async fn put(&self, key: &str, value: &[u8], ttl: Option<Duration>) -> Result<(), ClusterError>` | Stores value, increments version. Emits `Changed`. Overwrites if exists. |
| `delete` | `async fn delete(&self, key: &str) -> Result<bool, ClusterError>` | Removes entry. Emits `Deleted` if existed. Return MAY be `true` unconditionally if backend cannot determine prior existence. |
| `contains` | `async fn contains(&self, key: &str) -> Result<bool, ClusterError>` | Existence check. MAY be `get(key).is_some()`. |
| `put_if_absent` | `async fn put_if_absent(&self, key: &str, value: &[u8], ttl: Option<Duration>) -> Result<Option<CacheEntry>, ClusterError>` | Atomic. `Some(entry)` if created, `None` if key existed. Emits `Changed` on creation only. |
| `compare_and_swap` | `async fn compare_and_swap(&self, key: &str, expected_version: u64, new_value: &[u8], ttl: Option<Duration>) -> Result<CacheEntry, ClusterError>` | Atomic version-based CAS. Emits `Changed` on success. `CasConflict { key, current }` on mismatch — `current` SHOULD contain the entry if cheaply obtainable. |
| `watch` | `async fn watch(&self, key: &str) -> Result<CacheWatch, ClusterError>` | Yields `CacheWatchEvent` for exact key. Drop unsubscribes. |
| `watch_prefix` | `async fn watch_prefix(&self, prefix: &str) -> Result<CacheWatch, ClusterError>` | Yields `CacheWatchEvent` for matching keys. Backends declaring `features().prefix_watch == false` return `Err(Unsupported { feature: "prefix_watch" })`. Callers may polyfill via `PollingPrefixWatch`. |

#### LeaderElectionV1 — Leader election primitive

| Method | Signature | Contract |
|--------|-----------|----------|
| `resolver` | `fn resolver(hub: &ClientHub) -> LeaderElectionResolverBuilder<'_>` | Static entry point. |
| `scoped` | `fn scoped(&self, prefix: &str) -> LeaderElectionV1` | Scopes election names. |
| `elect` | `async fn elect(&self, name: &str) -> Result<LeaderWatch, ClusterError>` | Join election. Auto-renews. `LeaderWatch` auto-reenrolls on `Status(Lost)`. |
| `elect_with_config` | `async fn elect_with_config(&self, name: &str, config: ElectionConfig) -> Result<LeaderWatch, ClusterError>` | Same with custom timing. |
| `LeaderWatch::changed` | `async fn changed(&mut self) -> LeaderWatchEvent` | Next watch event (`Status` / `Lagged` / `Reset` / `Closed`). Infallible at type level per ADR-003. Transient backend errors retried internally. Terminal errors arrive via `Closed(err)`. |
| `LeaderWatch::status` | `fn status(&self) -> LeaderStatus` | Cached snapshot from background task. Synchronous, no I/O. **Advisory** — see staleness bound. |
| `LeaderWatch::is_leader` | `fn is_leader(&self) -> bool` | `matches!(status(), Leader)`. Advisory — do NOT use for correctness-critical mutual exclusion. |
| `LeaderWatch::resign` | `async fn resign(self) -> Result<(), ClusterError>` | Explicit step-down. Triggers immediate re-election. |

**Staleness bound**: `is_leader() == true` at time T does NOT guarantee this node holds leadership at time T on the backend. The background task's state lags by up to one renewal interval plus a provider round-trip in steady state, and up to a full TTL under partition. For correctness-critical mutual exclusion, use `DistributedLockV1` + application-level CAS via `ClusterCacheV1::compare_and_swap`.

#### DistributedLockV1 — Distributed lock primitive

| Method | Signature | Contract |
|--------|-----------|----------|
| `resolver` | `fn resolver(hub: &ClientHub) -> LockResolverBuilder<'_>` | Static entry point. |
| `scoped` | `fn scoped(&self, prefix: &str) -> DistributedLockV1` | Scopes lock names. |
| `try_lock` | `async fn try_lock(&self, name: &str, ttl: Duration) -> Result<LockGuard, ClusterError>` | Non-blocking. `LockContended { name }` if held. |
| `lock` | `async fn lock(&self, name: &str, ttl: Duration, timeout: Duration) -> Result<LockGuard, ClusterError>` | Blocking up to `timeout`. `LockTimeout { name, waited }` if not acquired. |
| `LockGuard::extend` | `async fn extend(&self, additional_ttl: Duration) -> Result<(), ClusterError>` | Extends TTL. `LockExpired { name }` if TTL elapsed. |
| `LockGuard::release` | `async fn release(self) -> Result<(), ClusterError>` | Explicit release. Consumers MUST call this. `Drop` is a no-op (no I/O in `Drop`). |

**Critical-section rule** (see §2 Constraints, ADR-002): Consumers MUST NOT make remote I/O calls inside the critical section between `try_lock` / `lock` and `release`. No fencing tokens — the no-remote-in-critical-section rule eliminates the stale-writer scenario fencing tokens protect against.

#### ServiceDiscoveryV1 — Service discovery primitive

| Method | Signature | Contract |
|--------|-----------|----------|
| `resolver` | `fn resolver(hub: &ClientHub) -> ServiceDiscoveryResolverBuilder<'_>` | Static entry point. |
| `scoped` | `fn scoped(&self, prefix: &str) -> ServiceDiscoveryV1` | Scopes service `name` only. Metadata keys/values pass through unchanged. |
| `register` | `async fn register(&self, reg: ServiceRegistration) -> Result<ServiceHandle, ClusterError>` | Register instance. Auto-generates `instance_id` if not provided. Default state `Enabled`. |
| `discover` | `async fn discover(&self, name: &str, filter: DiscoveryFilter) -> Result<Vec<ServiceInstance>, ClusterError>` | Returns instances matching `state` AND every metadata predicate (AND-conjunction). Default filter = enabled-only with no metadata constraint (primary routing). `DiscoveryFilter::any()` = all instances. |
| `watch` | `async fn watch(&self, name: &str) -> Result<ServiceWatch, ClusterError>` | Yields `ServiceWatchEvent` (`Change(TopologyChange)` / `Lagged` / `Reset` / `Closed`). Watch is unfiltered — consumers apply filters client-side to each `Change` event. |
| `ServiceHandle::deregister` | `async fn deregister(self) -> Result<(), ClusterError>` | Instance removed; watchers receive `Change(Left)`. |
| `ServiceHandle::update_metadata` | `async fn update_metadata(&self, m: HashMap<String, String>) -> Result<(), ClusterError>` | Updates metadata; watchers receive `Change(Updated)`. |
| `ServiceHandle::set_enabled` | `async fn set_enabled(&self, enabled: bool) -> Result<(), ClusterError>` | Flip module-declared serving intent. Watchers receive `Change(Updated)`. NOT a health observation — liveness is signaled by heartbeat/TTL renewal. |

### 3.4 Internal & External Dependencies

| Dependency | Direction | Purpose |
|-----------|-----------|---------|
| `modkit` | SDK → modkit | GTS registration, ClientHub wiring |
| `gts` / `gts-macros` | Wiring → gts | Plugin schema definitions (used by follow-up wiring crate) |
| `tokio` | SDK | Async runtime (watch channels, broadcast, TTL timers in stub backends) |
| `tokio_util` | SDK | `CancellationToken` for `PollingPrefixWatch` and (follow-up) plugin lifecycles |
| `async-trait` | SDK | `#[async_trait]` on the four backend traits |
| `types-registry-sdk` | Wiring → registry | GTS instance discovery (used by follow-up wiring crate) |

External backend dependencies (`sqlx`, `kube`, `redis`, `async-nats`, `etcd-client`, `hazelcast`) belong to the follow-up plugin crates and are NOT SDK dependencies.

### 3.5 Resolution Pattern

There is no root trait. Each primitive has its own public-API facade struct with a static `resolver(hub)` entry point returning a fluent builder.

**Consumer-side definition (one place per consumer crate)**:

```rust
#[derive(Clone, Copy)]
pub struct EventBrokerProfile;
impl ClusterProfile for EventBrokerProfile {
    const NAME: &'static str = "event-broker";
}
```

**Call site**:

```rust
let cache = ClusterCacheV1::resolver(&hub)
    .profile(EventBrokerProfile)
    .require(CacheCapability::Linearizable)
    .require(CacheCapability::PrefixWatch)
    .resolve()?;

let leader = LeaderElectionV1::resolver(&hub)
    .profile(EventBrokerProfile)
    .require(LeaderElectionCapability::Linearizable)
    .resolve()?;
```

**Resolver builder body** (cache; the other three are identical in shape):

```rust
impl<'a> CacheResolverBuilder<'a> {
    pub(crate) fn new(hub: &'a ClientHub) -> Self {
        Self { hub, profile_name: None, requirements: Vec::new() }
    }
    pub fn profile<P: ClusterProfile>(mut self, _: P) -> Self {
        self.profile_name = Some(P::NAME);
        self
    }
    pub fn require(mut self, cap: CacheCapability) -> Self {
        self.requirements.push(cap);
        self
    }
    pub fn resolve(self) -> Result<ClusterCacheV1, ClusterError> {
        let profile = self.profile_name
            .ok_or(ClusterError::ProfileNotSpecified)?;
        // Map ClientHub's ScopedNotFound to our domain-level ProfileNotBound
        // so consumers see one error model.
        let inner: Arc<dyn ClusterCacheBackend> = self.hub
            .get_scoped(profile_scope(profile))
            .map_err(|_| ClusterError::ProfileNotBound { profile })?;
        validate_cache_capabilities(&*inner, &self.requirements)?;
        Ok(ClusterCacheV1 { inner })
    }
}
```

**Resolution flow**:
1. Consumer crate defines a `ClusterProfile` marker once. The `NAME` const is the only place the profile string appears on the consumer side.
2. Module calls `*V1::resolver(hub).profile(P).require(Cap...).resolve()`.
3. The wiring crate's `ClusterWiring::builder(...).build_and_start()` had previously registered the corresponding `Arc<dyn _Backend>` in ClientHub under `profile_scope(P::NAME)`.
4. The resolver looks up the registered backend, validates declared `*Capability` requirements against the backend's actual `features()` (and `consistency()` for cache), and returns the wrapped facade. Mismatch → `CapabilityNotMet { primitive, capability, provider }` at startup.

Multiple resolutions of the same primitive on the same profile are cheap (`Arc`-clone-equivalent) and idempotent.

`profile_scope(name)` is an SDK helper that maps a profile name to a `ClientScope`. Convention: scope name `cluster:{profile}`. Validation: profile name MUST conform to `[a-zA-Z0-9_-]+`; reject invalid names at registration time.

### 3.6 Lifecycle Pattern (Builder/Handle)

The cluster wiring crate (`cf-cluster`, follow-up change) is **not** registered as its own `RunnableCapability` in ModKit. It's a library exposing a builder/handle pair following the outbox pattern. A parent host module (registered as a `RunnableCapability`) owns the cluster handle inside its own `start`/`stop`.

```rust
// In the parent host module's RunnableCapability impl (follow-up):
impl RunnableCapability for HostModule {
    async fn start(&self, cancel: CancellationToken) -> anyhow::Result<()> {
        let cluster_handle = ClusterWiring::builder(&self.config.cluster, &self.hub)
            .build_and_start()
            .await?;
        self.cluster_handle.set(cluster_handle).ok();
        Ok(())
    }

    async fn stop(&self, cancel: CancellationToken) -> anyhow::Result<()> {
        if let Some(handle) = self.cluster_handle.take() {
            tokio::select! {
                () = handle.stop() => {} // graceful: deregister, cancel tokens, join
                () = cancel.cancelled() => {} // framework deadline
            }
        }
        Ok(())
    }
}
```

`ClusterHandle::stop().await` is the single entry point that:
1. Deregisters every registered backend from ClientHub via `deregister_*_backend` helpers (subsequent `*V1::resolver(...).resolve()` calls return `ProfileNotBound`).
2. Calls each plugin's internal stop sequence — cancels the plugin's `CancellationToken`, joins its background tasks (renewal loops, watch fan-out, TTL reapers).
3. Delivers `LeaderWatchEvent::Status(Lost)` then `LeaderWatchEvent::Closed(Shutdown)` to active leaders (two distinct events — `Status(Lost)` revokes confidence before the consumer can observe shutdown; `Closed(Shutdown)` ends the watch), `CacheWatchEvent::Closed(Shutdown)` to active cache watches, `ServiceWatchEvent::Closed(Shutdown)` to active service-discovery watches before returning.

**Why this shape**:
- Outbox is the codebase's production-mature long-running background-task pattern (`cluster/libs/modkit-db/src/outbox/manager.rs:455–596`). Mini-chat owns its outbox via `Outbox::builder(...).start()` from inside its own `RunnableCapability::start`.
- Ordering is by code flow inside the parent module's `start`, NOT framework declarations. The parent module is registered as a `RunnableCapability` dependency of consumer modules (via existing modkit module-dependency mechanism), so consumers can't try to resolve before cluster is up.
- Plugins are NOT separate `RunnableCapability` implementors. They expose builder/handle types like outbox does. The cluster wiring's builder calls each plugin's builder; the cluster handle owns each plugin's handle and stops them in reverse-start order.

**Post-shutdown behavior (narrowed best-effort `Ok`)**:
- `LockGuard::release(self)` / `ServiceHandle::deregister(self)` / `LeaderWatch::resign(self)` MAY return `Ok(())` on a best-effort basis ONLY after the backend has observed `RunnableCapability::stop` (e.g., via an internal `AtomicBool::shutdown_observed`). Outside the shutdown window, real errors (`LockExpired`, foreign-holder release attempts, connection-lost mid-release) MUST propagate normally — silently masking them under the "best-effort" rule would hide real consumer bugs.

### 3.7 Per-primitive Scoping

Each public-API facade exposes `pub fn scoped(&self, prefix: &str) -> Self` returning a wrapped instance auto-prepending `prefix + "/"` on the write path and stripping it on the read path. Scoping composes: `cache.scoped("event-broker").scoped("shard-0")` produces effective prefix `"event-broker/shard-0/"`.

**Per-primitive scoping rules**:

| Primitive | Scoped argument(s) | Read-path strip | NOT scoped |
|---|---|---|---|
| `ClusterCacheV1` | `key` on `get`/`put`/`delete`/`contains`/`put_if_absent`/`compare_and_swap`/`watch`; `prefix` on `watch_prefix` | `CacheEvent::{Changed,Deleted,Expired}{key}` — strip prefix on the way back to the consumer | (none — cache has only keys) |
| `LeaderElectionV1` | `name` on `elect`/`elect_with_config` | n/a — `LeaderWatch` events don't carry names; the consumer already holds the watch handle | (none — election has only a name) |
| `DistributedLockV1` | `name` on `try_lock`/`lock` | n/a — `LockGuard` is opaque, consumer doesn't see backend names | (none — lock has only a name) |
| `ServiceDiscoveryV1` | `name` field of `ServiceRegistration` on `register`; `name` argument on `discover`/`watch`. Returned `ServiceInstance.name` is stripped back. | `ServiceWatchEvent::Change(TopologyChange::Joined/Updated/Left { instance })` — `instance.name` is stripped | `ServiceRegistration::metadata` (keys and values), `DiscoveryFilter::metadata` predicates, `ServiceInstance::metadata`. Metadata is an attribute namespace per-instance; coordination namespacing uses `name`. |

**Examples**:

```rust
// Cache: keys
let cache = ClusterCacheV1::resolver(...).resolve()?.scoped("event-broker");
cache.put("shard-assignments", ...);          // backend sees "event-broker/shard-assignments"
cache.watch_prefix("");                        // backend sees "event-broker/"

// Leader election: election names
let leader = LeaderElectionV1::resolver(...).resolve()?.scoped("event-broker");
let watch = leader.elect("shard-leader").await?;  // backend sees "event-broker/shard-leader"

// Service discovery: service name only — metadata is unscoped
let sd = ServiceDiscoveryV1::resolver(...).resolve()?.scoped("event-broker");
sd.register(ServiceRegistration {
    name: "delivery".into(),                   // backend sees "event-broker/delivery"
    metadata: hashmap!{"region".into() => "us-east".into()}, // unchanged
    ..
}).await?;
```

**Why metadata is NOT scoped on service discovery**: metadata keys are an *attribute namespace per instance* (e.g., `topic-shard`, `region`, `version`), not a *coordination namespace*. Two unrelated services in different scopes both legitimately use the metadata key `region` — scoping it would either silently rename `region` → `event-broker/region` (breaking interoperability with platform tools) or rename inconsistently (different prefixes per consumer). The coordination namespace lives on the service `name`; everything below it is per-instance attributes.

**Wrapper implementation**: each public-API struct's `scoped()` returns a new instance whose `inner: Arc<dyn _Backend>` is a `Scoped*Backend` wrapper that prepends/strips the prefix. The wrapper is internal to the SDK — consumers see only `ClusterCacheV1`, etc.

**Scope validation**: the `prefix` argument MUST conform to `[a-zA-Z0-9_/-]+`. Invalid prefixes fail at scope construction with `ClusterError::InvalidName { name, reason }`.

### 3.8 Watch Event Shape

All three watches yield events via union enums of the same shape (per ADR-003).

```rust
enum CacheWatchEvent {
    Event(CacheEvent),                // a cache mutation; consumer calls cache.get(key) for value
    Lagged { dropped: u64 },          // watcher fell behind; treat watched keys as stale, re-read
    Reset,                            // subscription re-established (reconnect, compaction); re-read
    Closed(ClusterError),             // terminal — watch is no longer usable
}

enum LeaderWatchEvent {
    Status(LeaderStatus),             // leadership transition; Lost is transient (auto-reenroll)
    Lagged { dropped: u64 },
    Reset,
    Closed(ClusterError),
}

enum ServiceWatchEvent {
    Change(TopologyChange),           // topology event (Joined/Left/Updated)
    Lagged { dropped: u64 },
    Reset,
    Closed(ClusterError),
}
```

All three are `#[non_exhaustive]` and infallible at the type level — there is no `Result<_, _>`-returning `changed()` method on any watch. **Terminal errors arrive via `Closed(err)`. Transient backend errors (`ConnectionLost`, `Timeout`, `ResourceExhausted`) are retried internally by the watch's background task and do not surface as events.**

**Consumer obligations**:
- On `Lagged { dropped }` or `Reset`: treat current state as potentially stale and recover. Cache: re-read affected keys via `get`. Leader watch: wait for the next `Status` event before resuming leader-only work. Service watch: re-read membership via `discover`.
- After `Closed(err)`: the watch is no longer usable; no further events follow. Consumer MAY restart at the application level (call `elect()` / `watch()` again) once cluster is up.

**Shutdown sequence** for `LeaderWatch`: the wiring crate's `ClusterHandle::stop()` delivers `LeaderWatchEvent::Status(Lost)` synchronously to every active `LeaderWatch` currently in `Leader` state, immediately followed by `LeaderWatchEvent::Closed(ClusterError::Shutdown)` as the terminal event. Two distinct events at the type level — `Status(Lost)` revokes the leader's confidence before the consumer can observe shutdown; `Closed(Shutdown)` ends the watch.

ADR-003 captures the rationale for the union shape over `Result`/`?`-based signaling and applies to all three watches for consistency.

### 3.9 Capability Validation

Each primitive declares its own `*Capability` enum carrying the requirements a consumer can demand at resolution time. Each variant maps to a concrete backend characteristic check:

| Capability | Backend method | Check |
|---|---|---|
| `CacheCapability::Linearizable` | `ClusterCacheBackend::consistency()` | `== CacheConsistency::Linearizable` |
| `CacheCapability::PrefixWatch` | `ClusterCacheBackend::features()` | `.prefix_watch == true` |
| `LeaderElectionCapability::Linearizable` | `LeaderElectionBackend::features()` | `.linearizable == true` |
| `LockCapability::Linearizable` | `DistributedLockBackend::features()` | `.linearizable == true` |
| `ServiceDiscoveryCapability::MetadataFiltering` | `ServiceDiscoveryBackend::features()` | `.metadata_pushdown == true` |

**Validation helpers** (one per primitive):

```rust
fn validate_cache_capabilities(
    backend: &dyn ClusterCacheBackend,
    reqs: &[CacheCapability],
) -> Result<(), ClusterError> {
    for cap in reqs {
        match cap {
            CacheCapability::Linearizable
                if backend.consistency() != CacheConsistency::Linearizable =>
            {
                return Err(ClusterError::CapabilityNotMet {
                    primitive: "ClusterCacheV1",
                    capability: "Linearizable",
                    provider: std::any::type_name_of_val(backend),
                });
            }
            CacheCapability::PrefixWatch if !backend.features().prefix_watch => {
                return Err(ClusterError::CapabilityNotMet {
                    primitive: "ClusterCacheV1",
                    capability: "PrefixWatch",
                    provider: std::any::type_name_of_val(backend),
                });
            }
            _ => {}
        }
    }
    Ok(())
}
```

Same shape for `validate_leader_election_capabilities`, `validate_lock_capabilities`, `validate_service_discovery_capabilities`. The `provider` field uses `std::any::type_name_of_val` to give the operator a concrete diagnostic name for the bound backend.

**Why per-primitive (not bundled `CapabilityClass`)**: the prior bundled `CapabilityClass { Standalone, Durable, InMemory, Coordination }` collapsed three orthogonal axes (topology, persistence, consistency) into one fuzzy ordering. Per-primitive `*Capability` enums are type-safe (a cache resolver cannot accept `MetadataFiltering`) and grounded in concrete backend characteristic checks rather than coarse tier claims.

### 3.10 SDK Default Backends

The SDK ships three default backend implementations built on `Arc<dyn ClusterCacheBackend>`:

- `CasBasedLeaderElectionBackend` — `put_if_absent(election_key, node_id, ttl)` for candidacy, `watch(election_key)` for status changes, background renewal task at `ttl / (max_missed_renewals + 1)`, TTL expiry → `Status(Lost)` followed by auto-reenroll. `features()` returns `LeaderElectionFeatures { linearizable: cache.consistency() == Linearizable }` — derives from the underlying cache's consistency.
- `CasBasedDistributedLockBackend` — `put_if_absent(lock_key, holder_id, ttl)` for `try_lock`, `watch(lock_key)` to notify blocked waiters on release, background TTL reaper. Release via delete-if-still-holder using CAS (a foreign holder cannot release another's lock). No fencing tokens (the no-remote-in-critical-section rule eliminates the stale-writer scenario). `features()` returns `LockFeatures { linearizable: cache.consistency() == Linearizable }`.
- `CacheBasedServiceDiscoveryBackend` — `put(svc/{name}/{instance_id}, metadata, ttl)` for registration, `watch_prefix(svc/{name}/)` for topology change events, background TTL renewal for heartbeat. Metadata filtering is client-side; `features()` returns `ServiceDiscoveryFeatures { metadata_pushdown: false }`.

**Constructor pair per default backend**:
- `new(cache: Arc<dyn ClusterCacheBackend>) -> Result<Self, ClusterError>` — returns `Err(ClusterError::InvalidConfig)` if `cache.consistency() == EventuallyConsistent`. Default-safe.
- `new_allow_weak_consistency(cache: Arc<dyn ClusterCacheBackend>) -> Self` — always succeeds. Caller acknowledges the safety implications. Construction emits a warning log at instantiation. Required by spec for use cases where the underlying cache is intentionally `EventuallyConsistent` (Redis Sentinel, NATS R=1, Postgres `synchronous_commit=off`) and the consumer accepts the split-brain risk.

**SDK-default selection at the wiring layer (omit-primitive auto-wrap)**: operator YAML uses **omission** to opt into SDK defaults. If a profile binds a `cache` provider but does not bind `leader_election` / `lock` / `service_discovery`, the wiring crate auto-wraps the bound cache backend in the corresponding SDK default backend and registers each under the same profile scope. Explicit binding always wins. If both `cache` and another primitive are omitted (no anchor to wrap), the wiring crate fails startup with `ClusterError::InvalidConfig`.

```yaml
cluster:
  profiles:
    # Single-backend profile via omission
    default:
      cache: { provider: postgres }
      # leader_election omitted → CasBasedLeaderElectionBackend over postgres cache
      # lock              omitted → CasBasedDistributedLockBackend  over postgres cache
      # service_discovery omitted → CacheBasedServiceDiscoveryBackend over postgres cache

    # Mixed: native LE + auto-wrapped lock
    in-memory:
      cache: { provider: redis }
      leader_election: { provider: k8s-lease }
      service_discovery: { provider: k8s-lease }
      # lock omitted → CasBasedDistributedLockBackend over redis cache
```

### 3.11 Polyfill

`PollingPrefixWatch` synthesizes `watch_prefix` semantics on backends that declare `features().prefix_watch == false`:

```rust
PollingPrefixWatch::spawn(
    cache: Arc<dyn ClusterCacheBackend>,
    prefix: &str,
    interval: Duration,
) -> CacheWatch
```

Periodically lists keys under the prefix, diffs against the previous list, and emits `CacheWatchEvent::Event(CacheEvent::Changed | Deleted)` for observed changes. Cost: N `get` calls per interval, no millisecond-level precision. Doc comments explicitly warn about the cost and recommend routing to a backend with native prefix watch at scale. Drop on the watch stops the polling task.

### 3.12 Interactions & Sequences

#### Per-primitive Resolution

- [ ] `p1` - **ID**: `cpt-cf-clst-seq-per-primitive-resolution`

```
  Consumer Module                    SDK                         ClientHub
       │                              │                              │
       │  ClusterCacheV1::resolver(&hub)                              │
       │   .profile(EventBrokerProfile)                              │
       │   .require(CacheCapability::Linearizable)                   │
       │   .resolve()                 │                              │
       │ ────────────────────────────>│                              │
       │                              │  hub.get_scoped::<dyn        │
       │                              │     ClusterCacheBackend>(    │
       │                              │     profile_scope("event-broker"))│
       │                              │ ────────────────────────────>│
       │                              │  Arc<dyn ClusterCacheBackend>│
       │                              │ <────────────────────────────│
       │                              │  validate_cache_capabilities │
       │                              │     (consistency() check)    │
       │                              │  wrap in ClusterCacheV1      │
       │  ClusterCacheV1              │                              │
       │ <────────────────────────────│                              │
```

#### Lifecycle: Parent host module → Cluster wiring → Plugins

- [ ] `p1` - **ID**: `cpt-cf-clst-seq-lifecycle-startup`

```
  Module Host         Parent Module               Cluster Wiring          Plugins
       │                   │                          │                      │
       │ start(cancel)     │                          │                      │
       │ ─────────────────>│                          │                      │
       │                   │ ClusterWiring::builder() │                      │
       │                   │  .build_and_start()      │                      │
       │                   │ ────────────────────────>│                      │
       │                   │                          │ read profile config  │
       │                   │                          │ (cache: redis,       │
       │                   │                          │  leader: k8s-lease)  │
       │                   │                          │                      │
       │                   │                          │ Plugin::builder()    │
       │                   │                          │  .build_and_start()  │
       │                   │                          │ ────────────────────>│
       │                   │                          │                      │  spawn
       │                   │                          │                      │  CancellationToken
       │                   │                          │                      │  + JoinHandles
       │                   │                          │                      │
       │                   │                          │ register_*_backend   │
       │                   │                          │  (per profile per    │
       │                   │                          │   primitive in       │
       │                   │                          │   ClientHub)         │
       │                   │                          │                      │
       │                   │ ClusterHandle            │                      │
       │                   │ <────────────────────────│                      │
       │                   │ store handle             │                      │
       │ Ok                │                          │                      │
       │ <─────────────────│                          │                      │

  Consumer modules now resolve via *V1::resolver(...).profile(P).resolve()
```

#### Shutdown Sequence

- [ ] `p1` - **ID**: `cpt-cf-clst-seq-shutdown`

```
  Module Host       Parent Module        Cluster Handle         Active Watches
       │                 │                    │                        │
       │ stop(cancel)    │                    │                        │
       │ ───────────────>│                    │                        │
       │                 │ handle.stop()      │                        │
       │                 │ ──────────────────>│                        │
       │                 │                    │ deregister all backends│
       │                 │                    │  from ClientHub         │
       │                 │                    │                        │
       │                 │                    │ deliver Status(Lost)   │
       │                 │                    │  to active leaders     │
       │                 │                    │ ──────────────────────>│ Status(Lost)
       │                 │                    │ deliver Closed(Shutdown│
       │                 │                    │  to all watches        │
       │                 │                    │ ──────────────────────>│ Closed(Shutdown)
       │                 │                    │                        │
       │                 │                    │ cancel plugin tokens   │
       │                 │                    │ join plugin tasks      │
       │                 │                    │                        │
       │                 │ Ok                 │                        │
       │                 │ <──────────────────│                        │
       │ Ok              │                    │                        │
       │ <───────────────│                    │                        │
```

### Database schemas & tables

N/A — the cluster SDK has no persistent database schemas. Cluster is an in-process library that delegates all storage to plugin-owned backends (Redis, Postgres, K8s API, NATS, etcd), each of which manages its own schema or storage layout independently. The SDK's only durable types are the wire-stable contract surfaces (facade methods, backend traits, error variants) documented in §3.3 and §3.1; those are Rust types, not database tables.

Per-backend storage layout (e.g., the Postgres plugin's `cluster_cache` and `cluster_cache_subscriber_lease` tables, the K8s plugin's CRDs) is documented in each follow-up plugin's own DESIGN, not here.

## 4. Additional Context

### 4.1 Backend Feature Compatibility

**Sub-capability implementation strategy per backend:**

| Backend | Cache | Leader Election | Distributed Lock | Service Discovery |
|---------|-------|----------------|-----------------|-------------------|
| **Standalone** (in-process, follow-up) | Native (HashMap + AtomicU64) | Native (watch channel) | Native (Mutex + Notify) | Native (HashMap) |
| **Postgres** (follow-up) | Native (table + LISTEN/NOTIFY) | SDK default (on PG cache) | Native (`pg_advisory_lock`) | SDK default (on PG cache) |
| **K8s** (follow-up) | Native (CRD + `resourceVersion`) | Native (Lease API) | Native (Lease API) | Native (Lease per instance, annotations carry state + metadata) |
| **Redis** (follow-up) | Native (GET/SET/Lua) | SDK default (on Redis cache) | Native (SET NX EX + Lua) | SDK default (on Redis cache) |
| **NATS KV** (follow-up) | Native (KV bucket + revision) | SDK default (on NATS cache) | SDK default (on NATS cache) | SDK default (on NATS cache) |
| **etcd** (follow-up) | Native (KV + `mod_revision`) | Native (election API) | Native (lock API) | SDK default (on etcd cache) |

**ProviderErrorKind mapping per backend:**

| ProviderErrorKind | Redis (fred) | Postgres (sqlx) | NATS (async-nats) | K8s (kube) | etcd (etcd-client) |
|---|---|---|---|---|---|
| `ConnectionLost` | `ErrorKind::IO` | `Error::Io` | `ConnectErrorKind::Io` | `HyperError` | `TransportError` |
| `Timeout` | `ErrorKind::Timeout` | `Error::PoolTimedOut` | `*ErrorKind::TimedOut` | hyper timeout | gRPC `DeadlineExceeded` |
| `AuthFailure` | `ErrorKind::Auth` | SQLSTATE `28xxx` | `Authentication` | HTTP `401`/`403` | gRPC `Unauthenticated` |
| `ResourceExhausted` | `ErrorKind::Backpressure` | — | — | HTTP `429` | gRPC `ResourceExhausted` |

### 4.2 Recommended Deployment Combinations

| Deployment | Config | Cache | LE | Lock | SD | Notes |
|-----------|--------|-------|----|----|----|----|
| Dev / single-instance | `provider: standalone` | Standalone | Standalone | Standalone | Standalone | Zero deps |
| Multi-instance, no K8s | `provider: postgres` | Postgres | SDK default | Postgres | SDK default | Zero new infra |
| K8s, low-throughput | `provider: k8s` | K8s CRD | K8s Lease | K8s Lease | K8s Lease (per instance) | Zero new infra |
| K8s + Redis (recommended) | hybrid | Redis | K8s Lease | Redis | K8s Lease (per instance) | Best of both |
| Redis-only | `provider: redis` | Redis | SDK default | Redis | SDK default | Single infra dep |
| NATS stack | `provider: nats` | NATS KV | SDK default | SDK default | SDK default | Single infra dep |
| etcd available | `provider: etcd` | etcd | etcd (native) | etcd (native) | SDK default | Best coordination guarantees |

### 4.3 Existing Code Migration

The following existing code overlaps with cluster capabilities and will be migrated in **separate follow-up changes**:

| Existing Code | Location | Overlap | Migration Plan |
|------|----------|---------|---|
| `LeaderElector` trait + `K8sLeaseElector` | `mini-chat/src/infra/leader/` | Leader election (production-quality K8s Lease impl) | Extract into `cf-k8s-cluster-plugin`; mini-chat consumes via `LeaderElectionV1::resolver(&hub).profile(MiniChatProfile).resolve()` |
| File-based advisory locks | `libs/modkit-db/src/advisory_locks.rs` | Distributed lock (single-host only, no fencing) | Not reusable — cluster provides true distributed locks via `DistributedLockV1`. Modules migrate on adoption. |
| In-memory `NodesRegistry` | `modules/system/nodes-registry/` | Service discovery (node-specific, in-memory) | nodes-registry may become a consumer of `ServiceDiscoveryV1` for cross-instance routing |

## 5. Risks / Trade-offs

**[Risk: Abstraction leakage]** Different backends have fundamentally different consistency guarantees (Redis RedLock is "probably correct", Postgres advisory locks are strictly serializable, Hazelcast IMap is CP or AP depending on config). Trait documentation must be explicit about minimum guarantees, and plugins must document their actual guarantees.
- Mitigation: Define minimum guarantees in trait docs (e.g., "at most one leader at any point per `LeaderElectionFeatures::linearizable == true` plus advisory staleness bound"). Plugin authors document their `*Features` declarations honestly. Capability requirements at the resolver site enforce honest characteristic claims at startup.

**[Risk: SDK contract verifies API shape, not distributed correctness]** Smoke tests against minimal in-process stubs verify that consumer code compiles against the SDK, handles the happy path, and exercises the error variants stubs emit (`Lagged`, `Closed(Shutdown)`, `CasConflict`, `CapabilityNotMet`). They do NOT verify behavior under network partition, clock skew, split-brain, message reordering across subscribers, or backend-specific failure semantics (Redis AOF loss, Postgres `synchronous_commit` windows, NATS JetStream sequence gaps, K8s API-server throttling). These failure modes cannot be faithfully simulated in-process — stubs have one state map, one clock, and one FIFO event channel.
- Mitigation: Each plugin follow-up change ships feature-gated integration tests against the real backend using CI infrastructure (Postgres containers for Phase 3, kind/minikube for Phase 4 K8s, future Redis/NATS/etcd containers). These tests are the authoritative source of distributed-correctness verification for each backend.
- Future work (out of initial scope): Jepsen-style correctness harness exercising partition, clock skew, and process-kill scenarios against each plugin.

**[Trade-off: Per-primitive routing config complexity]** Per-primitive backend routing in operator YAML adds configuration surface. Operators could create confusing combinations (e.g., three different backends for four primitives).
- Mitigation: Documented recommended combinations in §4.2. Capability validation surfaces incompatible combinations at startup with clear error messages naming the bound backend. SDK-default omit-primitive auto-wrap simplifies single-backend profiles to a 1-line YAML config.

**[Trade-off: SDK-only this change ships without runnable cluster]** Until the wiring crate (`cf-cluster`) and at least one production plugin (`cf-standalone-cluster-plugin`) ship, the cluster is not deployable beyond SDK consumption — consumers can compile against the SDK but cannot run.
- Mitigation: Showcase example crates demonstrate consumer usage and plugin author shape (builder/handle pattern). Smoke tests prove the SDK contract works. Follow-up plugin changes can begin in parallel against the stable SDK contract.

## 6. Open Questions

| Question | Owner | Target Resolution |
|----------|-------|-------------------|
| Backend authentication and credential wiring | Platform OOP deployment design | Resolved as part of the broader OOP design |
| Whether ADR-003 (cache watch backpressure) broadens to cover all three watches, or a new ADR captures the generalization | Cluster module owner | Resolved during ADR audit — recommendation: broaden ADR-003 with a "Generalization to all three watches" section |
