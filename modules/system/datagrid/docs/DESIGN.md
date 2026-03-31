# Technical Design — Datagrid


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
  - [3.5 Interactions & Sequences](#35-interactions--sequences)
  - [3.6 Database schemas & tables](#36-database-schemas--tables)
- [4. Additional Context](#4-additional-context)
  - [4.1 Backend Feature Compatibility](#41-backend-feature-compatibility)
  - [4.2 Recommended Deployment Combinations](#42-recommended-deployment-combinations)
  - [4.3 Existing Code Migration](#43-existing-code-migration)
- [5. Risks / Trade-offs](#5-risks--trade-offs)
- [6. Open Questions](#6-open-questions)

<!-- /toc -->

## 1. Architecture Overview

### 1.1 Architectural Vision

Datagrid is a platform-level system module that provides cluster coordination and shared-state primitives to all CyberFabric modules. It bundles four sub-capabilities — distributed cache (KV with TTL, CAS, and watch notifications), leader election, distributed locks with fencing tokens, and service discovery — behind a single `Datagrid` trait registered via GTS and resolved through ClientHub.

The architecture follows the ModKit Gateway + Plugins pattern (same as authn-resolver, authz-resolver, credstore, tenant-resolver). An SDK crate (`datagrid-sdk`) defines the trait boundaries. A wiring crate (`datagrid`) handles GTS registration, ClientHub injection, and configuration. Backend-specific implementations ship as plugins under `plugins/`.

The key architectural differentiator is the **hybrid compositor**: a `HybridDatagridPlugin` that routes each sub-capability independently to the best backend for the job. Operators can run Redis for cache, K8s Lease for leader election, and K8s EndpointSlice for service discovery — all through a single `Datagrid` entry in ClientHub.

The SDK also ships **default implementations** of leader election, distributed lock, and service discovery built entirely on `DatagridCache` CAS operations. This means a minimal provider only needs to implement the cache trait — the SDK builds the other three on top. Native implementations override the defaults when a backend excels (e.g., K8s Lease for elections).

Explicit pub/sub messaging is excluded. The event broker module provides reliable pub/sub with delivery guarantees, consumer groups, offsets, and replay. The datagrid provides reactive cache notifications (watch by key or prefix) for data-change observation — "this data changed" vs "deliver this message reliably".

### 1.2 Architecture Drivers

#### Functional Drivers

| Requirement | Design Response |
|-------------|-----------------|
| Cluster-wide shared state for modules | `DatagridCache` with version-based CAS, TTL, and watch notifications |
| Worker pool coordination (event broker, schedulers) | `LeaderElection` with watch-based status model and automatic renewal |
| Distributed rate limiting (OAGW) | `DistributedLock` with fencing tokens and TTL |
| OOP module-to-module routing | `ServiceDiscovery` with health-aware instance listing and topology watches |
| Multiple infrastructure backends | Hybrid compositor with per-capability provider routing |
| Zero-infra dev/test deployment | `StandaloneDatagridPlugin` using in-process Tokio primitives |

#### Architecture Decision Records

| ADR | Summary |
|-----|---------|
| `cpt-cf-dgrd-adr-provider-compat-perf` | Provider compatibility and performance analysis — hybrid model, per-provider performance characteristics, prefix-based routing, subscriber leases as cache not locks |

#### NFR Allocation

| NFR Summary | Allocated To | Design Response | Verification Approach |
|-------------|--------------|-----------------|----------------------|
| Sub-millisecond in-process latency for dev/test | Standalone plugin | Tokio channels + HashMap, zero network | Benchmark tests |
| At most one leader per election name | All providers | Trait contract enforces single-leader guarantee; integration tests verify | Multi-task contention tests |
| Lock fencing correctness | All providers | Monotonic fencing tokens; trait contract documents Kleppmann pattern | Fencing token monotonicity tests |
| No serde in contract types | SDK crate | Dylint layer rules enforce no serde in trait definitions | `make check` (dylint lints) |

### 1.3 Architecture Layers

```
┌─────────────────────────────────────────────────────────────┐
│                Consumers (Event Broker, OAGW, modules)      │
├─────────────────────────────────────────────────────────────┤
│  datagrid-sdk     │ Trait definitions, types, SDK defaults  │
│                   │ HybridDatagridPlugin compositor         │
├─────────────────────────────────────────────────────────────┤
│  datagrid         │ GTS registration, ClientHub wiring,     │
│                   │ configuration parsing, lifecycle        │
├─────────────────────────────────────────────────────────────┤
│  Plugins          │ Backend-specific implementations        │
│  ┌────────────────┐  ┌───────────────┐  ┌────────────────┐ │
│  │ standalone     │  │ postgres      │  │ k8s            │ │
│  │ (in-process)   │  │ (advisory +   │  │ (CRD + Lease + │ │
│  │                │  │  table + L/N) │  │  EndpointSlice)│ │
│  └────────────────┘  └───────────────┘  └────────────────┘ │
├─────────────────────────────────────────────────────────────┤
│  External          │ PostgreSQL, K8s API, Redis, NATS, etcd │
└─────────────────────────────────────────────────────────────┘
```

| Layer | Responsibility | Technology |
|-------|---------------|------------|
| SDK | Trait definitions (`Datagrid`, `DatagridCache`, `LeaderElection`, `DistributedLock`, `ServiceDiscovery`), shared types, SDK default implementations, hybrid compositor | Rust crate (`datagrid-sdk`) |
| Wiring | GTS plugin registration, ClientHub resolver, configuration parsing (shorthand vs hybrid mode), lifecycle orchestration | Rust crate (`datagrid`) |
| Plugins | Backend-specific sub-capability implementations | Rust crates per backend |
| External | Persistence, coordination, cluster state | PostgreSQL, K8s API server, Redis, etc. |

## 2. Principles & Constraints

### 2.1 Design Principles

#### Cache CAS as Universal Building Block

- [ ] `p1` - **ID**: `cpt-cf-dgrd-principle-cas-universal`

`DatagridCache` with version-based CAS is the foundational primitive. Leader election, distributed locks, and service discovery can all be built on top of cache CAS + watch. The SDK ships default implementations of all three using only cache operations. This means a minimal provider needs to implement only `DatagridCache` to get all four sub-capabilities. Native overrides improve performance but are never required.

#### Hybrid Composition Over Monolithic Providers

- [ ] `p1` - **ID**: `cpt-cf-dgrd-principle-hybrid-composition`

Each sub-capability routes independently to the best backend for the job. The `HybridDatagridPlugin` compositor wires `Arc<dyn DatagridCache>`, `Arc<dyn LeaderElection>`, `Arc<dyn DistributedLock>`, and `Arc<dyn ServiceDiscovery>` from potentially different providers. Shorthand config (`provider: redis`) covers the common single-backend case. Per-capability overrides are opt-in.

#### Lightweight Notifications, Not Messaging

- [ ] `p1` - **ID**: `cpt-cf-dgrd-principle-lightweight-notifications`

Cache watch events carry only the key and event type (`Changed`, `Deleted`, `Expired`) — no value payload. Consumers call `cache.get(key)` for the current value. This avoids stale-value issues, maps cleanly to all backends (Redis keyspace notifications carry no value, Postgres NOTIFY has 8KB limit), and keeps events fixed-size. Reliable messaging belongs in the event broker.

#### Version-Based Optimistic Concurrency

- [ ] `p1` - **ID**: `cpt-cf-dgrd-principle-version-based-cas`

`compare_and_swap` takes an `expected_version: u64` obtained from a prior `get()`, not an expected byte value. `get()` returns `CacheEntry { value, version }`. This maps natively to all backends: `resourceVersion` (K8s), `revision` (NATS), `mod_revision` (etcd), `BIGSERIAL` (Postgres), Lua counter (Redis), `AtomicU64` (in-process). Value-based CAS would require racy get-compare-put loops on revision-based backends.

### 2.2 Constraints

#### No Serde in Contract Types

- [ ] `p1` - **ID**: `cpt-cf-dgrd-constraint-no-serde`

SDK trait definitions and shared types MUST NOT depend on serde. This follows the existing ModKit contract-layer convention enforced by dylint lints. Serialization concerns belong in provider implementations.

#### Traits Only in SDK

- [ ] `p1` - **ID**: `cpt-cf-dgrd-constraint-traits-in-sdk`

The SDK crate contains only trait definitions, shared types, SDK default implementations, and the hybrid compositor. No backend-specific code, no I/O, no external dependencies beyond `tokio`.

#### Name Validation at Trait Boundary

- [ ] `p1` - **ID**: `cpt-cf-dgrd-constraint-name-validation`

All names (cache keys, election names, lock names, service names) are validated at the trait boundary against `[a-zA-Z0-9_/-]+`. Hierarchical namespacing uses `/` as separator (e.g., `event-broker/shard-assignments`). Consistent with credstore key naming. Providers do not re-validate — the SDK enforces the contract.

## 3. Technical Architecture

### 3.1 Domain Model

**Core Entities:**

| Entity | Description |
|--------|-------------|
| `CacheEntry` | A versioned key-value pair: `{ value: Vec<u8>, version: u64 }`. Version is opaque, monotonically increasing per key, starting at 1. Version 0 is reserved as sentinel. |
| `CacheEvent` | Lightweight notification: `Changed { key }`, `Deleted { key }`, `Expired { key }`. No payload — consumer calls `get(key)` for current value. |
| `CacheWatch` | Async receiver yielding `CacheEvent` items. Dropping unsubscribes. Per-key ordering guaranteed; no cross-key ordering. |
| `LeaderStatus` | Election state: `Leader`, `Follower`, `Lost`. `Lost` is terminal — consumer must re-elect. |
| `LeaderWatch` | Async receiver yielding `LeaderStatus` changes. Dropping steps down from leadership. |
| `ElectionConfig` | Timing: `ttl: Duration` (default 30s), `renewal_interval: Duration` (default ttl/3). |
| `LockGuard` | RAII lock handle with `fencing_token() -> u64` and `extend(additional_ttl)`. Drop releases lock. |
| `ServiceRegistration` | Registration request: `name`, optional `instance_id`, `address`, `metadata: HashMap<String, String>`. |
| `ServiceInstance` | Discovered instance: `instance_id`, `address`, `metadata`, `health: HealthStatus`, `registered_at`. |
| `HealthStatus` | Instance health: `Healthy`, `Unhealthy`, `Unknown`. |
| `ServiceHandle` | Registration handle: `deregister()`, `update_metadata()`, `set_health()`. Drop deregisters. |
| `TopologyChange` | Service topology event: `Joined(ServiceInstance)`, `Left(instance_id)`, `Updated(ServiceInstance)`. |
| `ServiceWatch` | Async receiver yielding `TopologyChange` items. |
| `DatagridError` | Unified error enum with semantic variants (`InvalidName`, `CasConflict`, `LockContended`, `LockTimeout`, `LockExpired`, `NotStarted`, `Shutdown`) and structured provider errors (`Provider { kind: ProviderErrorKind, message, source }`). |
| `ProviderErrorKind` | Infrastructure error classification: `ConnectionLost`, `Timeout`, `AuthFailure`, `ResourceExhausted`, `Other`. Enables programmatic retryability decisions. |
| `CapabilityClass` | Provider performance tier: `Standalone` (in-process), `Standard` (1k-10k ops/sec, Postgres/etcd), `HighThroughput` (10k-200k ops/sec, Redis/NATS), `Coordination` (low volume, strong consistency, K8s Lease/etcd). Used for startup validation against module-declared profiles. |
| `DatagridClient` | SDK resolver. Modules call `resolve(hub, profile)` with a profile from their own config. The resolver maps the profile to a scoped `Arc<dyn Datagrid>` and validates the provider's `CapabilityClass` meets the profile's minimum. `resolve_default(hub)` returns the global instance. |

**Relationships:**
- A `CacheEntry` belongs to exactly one key. Each `put` increments the version.
- A `LeaderWatch` belongs to one election name. At most one `LeaderWatch` across all nodes observes `Leader`.
- A `LockGuard` belongs to one lock name. Fencing tokens are strictly monotonic per lock name across all acquisitions and all nodes.
- A `ServiceHandle` belongs to one service registration. Each service name can have multiple instances.

### 3.2 Component Model

```
┌────────────────────────────────────────────────────────────────────┐
│                          datagrid-sdk                              │
│  ┌──────────────┐ ┌─────────────┐ ┌───────────┐ ┌──────────────┐ │
│  │DatagridCache │ │LeaderElect. │ │Distrib.   │ │ServiceDisc.  │ │
│  │   (trait)    │ │  (trait)    │ │Lock(trait)│ │   (trait)    │ │
│  └──────┬───────┘ └──────┬──────┘ └─────┬─────┘ └──────┬───────┘ │
│         │                │              │              │          │
│  ┌──────┴────────────────┴──────────────┴──────────────┴───────┐  │
│  │                 SDK Default Implementations                  │  │
│  │  CasBasedLeaderElection  CasBasedDistributedLock             │  │
│  │  CacheBasedServiceDiscovery                                  │  │
│  └──────────────────────────┬──────────────────────────────────┘  │
│                             │                                     │
│  ┌──────────────────────────┴──────────────────────────────────┐  │
│  │              HybridDatagridPlugin (compositor)               │  │
│  │  Routes each sub-capability to configured backend            │  │
│  │  Falls back to SDK defaults for unconfigured sub-caps        │  │
│  └─────────────────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────────────┘
```

**Components:**

- [ ] `p1` - **ID**: `cpt-cf-dgrd-component-sdk`

`datagrid-sdk` (`cf-datagrid-sdk`) — All trait definitions (`Datagrid`, `DatagridCache`, `LeaderElection`, `DistributedLock`, `ServiceDiscovery`), shared types (`CacheEntry`, `CacheEvent`, `DatagridError`, `ProviderErrorKind`, `LockGuard`, `LeaderWatch`, `ServiceInstance`, `TopologyChange`, etc.), SDK default implementations (`CasBasedLeaderElection`, `CasBasedDistributedLock`, `CacheBasedServiceDiscovery`), `HybridDatagridPlugin` compositor, and name validation utilities. Zero external dependencies beyond `tokio`.

- [ ] `p1` - **ID**: `cpt-cf-dgrd-component-wiring`

`datagrid` (`cf-datagrid`) — GTS type registration (following `BaseModkitPluginV1` pattern), ClientHub resolver for `dyn Datagrid`, configuration parsing (shorthand `provider: standalone` vs hybrid `cache: redis, leader_election: k8s`), lifecycle orchestration (`start`/`shutdown` delegation).

- [ ] `p1` - **ID**: `cpt-cf-dgrd-component-standalone-plugin`

`standalone-datagrid-plugin` (`cf-standalone-datagrid-plugin`) — In-process implementation using `HashMap` with TTL + version counter for cache, `tokio::sync::broadcast` for watch, `tokio::sync::watch` for leader election, `Mutex` + `Notify` for locks, `HashMap` for service registry. Zero external dependencies. Default when no provider is configured.

- [ ] `p2` - **ID**: `cpt-cf-dgrd-component-postgres-plugin`

`postgres-datagrid-plugin` (`cf-postgres-datagrid-plugin`) — Postgres-backed implementation: `datagrid_cache` table with `BIGSERIAL` version for cache, `LISTEN/NOTIFY` with triggers for watch, `pg_advisory_lock` for distributed locks, SDK defaults for leader election and service discovery. Depends on `sqlx`.

- [ ] `p2` - **ID**: `cpt-cf-dgrd-component-k8s-plugin`

`k8s-datagrid-plugin` (`cf-k8s-datagrid-plugin`) — K8s-backed implementation: Custom Resource for cache (using `resourceVersion` for CAS), `coordination.k8s.io/v1` Lease for leader election and locks, `EndpointSlice` for service discovery. Depends on `kube` + `k8s-openapi`.

### 3.3 API Contracts

**Technology**: Rust async traits (ClientHub, in-process)

#### Datagrid (root trait)

| Method | Signature | Description |
|--------|-----------|-------------|
| `capability_class` | `fn capability_class(&self) -> CapabilityClass` | Self-declared performance tier. Used for startup validation. |
| `cache` | `fn cache(&self) -> &dyn DatagridCache` | Access cache sub-capability |
| `leader_election` | `fn leader_election(&self) -> &dyn LeaderElection` | Access leader election sub-capability |
| `distributed_lock` | `fn distributed_lock(&self) -> &dyn DistributedLock` | Access distributed lock sub-capability |
| `service_discovery` | `fn service_discovery(&self) -> &dyn ServiceDiscovery` | Access service discovery sub-capability |
| `start` | `async fn start(&self) -> Result<(), DatagridError>` | Initialize provider. MUST be called before any sub-capability method. |
| `shutdown` | `async fn shutdown(&self) -> Result<(), DatagridError>` | Terminate all background tasks and release resources. |

#### DatagridClient (SDK resolver)

Modules resolve `Datagrid` through the `DatagridClient` helper, not raw ClientHub. The profile name comes from the module's own YAML config. The operator maps profiles to providers in platform config. At resolution time, the resolver validates that the bound provider's `CapabilityClass` meets the profile's minimum.

| Method | Signature | Description |
|--------|-----------|-------------|
| `resolve` | `fn resolve(hub: &ClientHub, profile: &str) -> Result<Arc<dyn Datagrid>>` | Resolve a named profile to a scoped provider. Validates capability class. |
| `resolve_default` | `fn resolve_default(hub: &ClientHub) -> Result<Arc<dyn Datagrid>>` | Resolve the global default provider. No profile needed. |

`CapabilityClass` tiers: `Standalone` (in-process, dev/test), `Standard` (1k-10k ops/sec, Postgres/etcd), `HighThroughput` (10k-200k ops/sec, Redis/NATS), `Coordination` (low volume, strong consistency, K8s Lease/etcd).

#### DatagridCache

| Method | Signature | Contract |
|--------|-----------|----------|
| `get` | `async fn get(&self, key: &str) -> Result<Option<CacheEntry>>` | Returns versioned entry or `None`. Never errors for missing keys. |
| `put` | `async fn put(&self, key: &str, value: &[u8], ttl: Option<Duration>)` | Stores value, increments version. Emits `Changed`. Overwrites if exists. |
| `delete` | `async fn delete(&self, key: &str) -> Result<bool>` | Returns `true` if existed. Emits `Deleted` if existed. |
| `contains` | `async fn contains(&self, key: &str) -> Result<bool>` | `true` if exists and not expired. |
| `put_if_absent` | `async fn put_if_absent(&self, key: &str, value: &[u8], ttl: Option<Duration>) -> Result<Option<CacheEntry>>` | Atomic. `Some(entry)` if created, `None` if key existed. Emits `Changed` on creation only. |
| `compare_and_swap` | `async fn compare_and_swap(&self, key: &str, expected_version: u64, new_value: &[u8], ttl: Option<Duration>) -> Result<CacheEntry>` | Atomic version-based CAS. Returns new entry on success. `CasConflict { current }` on version mismatch. |
| `watch` | `async fn watch(&self, key: &str) -> Result<CacheWatch>` | Yields `CacheEvent` for exact key. Drop unsubscribes. |
| `watch_prefix` | `async fn watch_prefix(&self, prefix: &str) -> Result<CacheWatch>` | Yields `CacheEvent` for all keys matching prefix. |

#### LeaderElection

| Method | Signature | Contract |
|--------|-----------|----------|
| `elect` | `async fn elect(&self, name: &str) -> Result<LeaderWatch>` | Join election. At most one participant holds `Leader` across all nodes. Auto-renews. Drop steps down. |
| `elect_with_config` | `async fn elect_with_config(&self, name: &str, config: ElectionConfig) -> Result<LeaderWatch>` | Same as `elect` with custom TTL and renewal interval. |

Guarantees: at most one leader per name; leader loss detected within TTL; automatic renewal transparent to consumer; `Lost` is terminal — consumer must re-elect.

#### DistributedLock

| Method | Signature | Contract |
|--------|-----------|----------|
| `try_lock` | `async fn try_lock(&self, name: &str, ttl: Duration) -> Result<LockGuard>` | Non-blocking. `LockContended` if held. Returns guard with monotonic fencing token. |
| `lock` | `async fn lock(&self, name: &str, ttl: Duration, timeout: Duration) -> Result<LockGuard>` | Blocking up to `timeout`. `LockTimeout` if not acquired. |

`LockGuard`: `fencing_token() -> u64` (strictly monotonic per lock name across all nodes), `extend(additional_ttl)` (`LockExpired` if TTL elapsed), `Drop` releases lock.

#### ServiceDiscovery

| Method | Signature | Contract |
|--------|-----------|----------|
| `register` | `async fn register(&self, reg: ServiceRegistration) -> Result<ServiceHandle>` | Register instance. Auto-generates `instance_id` if not provided. |
| `discover` | `async fn discover(&self, name: &str) -> Result<Vec<ServiceInstance>>` | All instances (any health). Empty vec if none. |
| `discover_healthy` | `async fn discover_healthy(&self, name: &str) -> Result<Vec<ServiceInstance>>` | Only `Healthy` instances. |
| `watch` | `async fn watch(&self, name: &str) -> Result<ServiceWatch>` | Yields `TopologyChange` events: `Joined`, `Left`, `Updated`. |

`ServiceHandle`: `deregister()`, `update_metadata(HashMap)`, `set_health(HealthStatus)`. `Drop` deregisters (best-effort).

### 3.4 Internal & External Dependencies

| Dependency | Direction | Purpose |
|-----------|-----------|---------|
| `modkit` | SDK → modkit | GTS registration, ClientHub wiring |
| `gts` / `gts-macros` | Wiring → gts | Plugin schema definitions |
| `tokio` | SDK + all plugins | Async runtime, channels, mutexes |
| `sqlx` | Postgres plugin → sqlx | Database access |
| `kube` + `k8s-openapi` | K8s plugin → kube | K8s API access |
| `types-registry-sdk` | Wiring → types-registry | GTS instance discovery |

### 3.5 Interactions & Sequences

#### Cache Put with Watch Notification

- [ ] `p1` - **ID**: `cpt-cf-dgrd-seq-cache-put-watch`

```
  Writer                     Datagrid                  Watcher
    │                           │                         │
    │  put("config/flags",      │                         │
    │       value, 60s)         │                         │
    │ ─────────────────────────>│                         │
    │                           │  store value,           │
    │                           │  increment version      │
    │                           │  emit CacheEvent::      │
    │                           │    Changed{key}         │
    │                           │ ───────────────────────>│
    │                           │                         │
    │  Ok(())                   │     CacheEvent::        │
    │ <─────────────────────────│     Changed{key:        │
    │                           │     "config/flags"}     │
    │                           │                         │
    │                           │     get("config/flags") │
    │                           │ <───────────────────────│
    │                           │                         │
    │                           │     CacheEntry{value,   │
    │                           │       version: N}       │
    │                           │ ───────────────────────>│
```

#### Leader Election Lifecycle

- [ ] `p1` - **ID**: `cpt-cf-dgrd-seq-leader-election`

```
  Candidate A             Datagrid              Candidate B
    │                        │                       │
    │  elect("worker-pool")  │                       │
    │ ──────────────────────>│                       │
    │                        │  elect("worker-pool") │
    │                        │ <─────────────────────│
    │                        │                       │
    │  LeaderStatus::Leader  │  LeaderStatus::       │
    │ <──────────────────────│    Follower            │
    │                        │ ─────────────────────>│
    │                        │                       │
    │  (automatic renewal    │                       │
    │   every ttl/3)         │                       │
    │                        │                       │
    │  drop(LeaderWatch)     │                       │
    │ ──────────────────────>│                       │
    │                        │  LeaderStatus::Leader │
    │                        │ ─────────────────────>│
```

#### Hybrid Compositor Wiring

- [ ] `p1` - **ID**: `cpt-cf-dgrd-seq-hybrid-wiring`

```
  Module Host              datagrid (wiring)            Providers
    │                           │                          │
    │  parse config             │                          │
    │  (cache: redis,           │                          │
    │   leader_election: k8s)   │                          │
    │ ─────────────────────────>│                          │
    │                           │  init Redis cache impl   │
    │                           │ ────────────────────────>│
    │                           │  init K8s LE impl        │
    │                           │ ────────────────────────>│
    │                           │  build HybridDatagrid:   │
    │                           │    cache = Redis          │
    │                           │    LE = K8s               │
    │                           │    lock = SDK default     │
    │                           │      (on Redis cache)     │
    │                           │    SD = SDK default       │
    │                           │      (on Redis cache)     │
    │                           │                          │
    │                           │  register in GTS         │
    │                           │  register in ClientHub   │
    │  hub.get::<dyn Datagrid>()│                          │
    │ ─────────────────────────>│                          │
    │  Arc<dyn Datagrid>        │                          │
    │ <─────────────────────────│                          │
```

### 3.6 Database schemas & tables

The datagrid SDK and standalone plugin have no database tables. Database schemas are plugin-specific:

**Postgres Plugin** (`datagrid_cache` table):

| Column | Type | Description |
|--------|------|-------------|
| `key` | `TEXT PRIMARY KEY` | Cache key |
| `value` | `BYTEA NOT NULL` | Opaque byte value |
| `version` | `BIGINT NOT NULL DEFAULT nextval('datagrid_cache_version_seq')` | Monotonic version |
| `expires_at` | `TIMESTAMPTZ NULL` | TTL expiry timestamp. `NULL` = no expiry. |

Index: `CREATE INDEX idx_datagrid_cache_expires ON datagrid_cache (expires_at) WHERE expires_at IS NOT NULL` — for TTL reaper queries.

Trigger: `AFTER INSERT OR UPDATE OR DELETE` on `datagrid_cache` → `pg_notify('datagrid_cache_events', '{event_type}:{key}')`. Payload is key-only (fits within 8KB NOTIFY limit).

**Postgres Plugin** (`datagrid_locks` table):

| Column | Type | Description |
|--------|------|-------------|
| `name` | `TEXT PRIMARY KEY` | Lock name |
| `fencing_token` | `BIGINT NOT NULL DEFAULT nextval('datagrid_lock_token_seq')` | Monotonic fencing token |
| `holder` | `TEXT NOT NULL` | Holder identifier |
| `expires_at` | `TIMESTAMPTZ NOT NULL` | Lock TTL expiry |

**K8s Plugin**: No database tables. Uses K8s Custom Resources (`DatagridEntry` CRD), `coordination.k8s.io/v1` Lease objects, and `EndpointSlice` resources.

## 4. Additional Context

### 4.1 Backend Feature Compatibility

**Sub-capability implementation strategy per backend:**

| Backend | Cache | Leader Election | Distributed Lock | Service Discovery |
|---------|-------|----------------|-----------------|-------------------|
| **Standalone** | Native (HashMap + AtomicU64) | Native (watch channel) | Native (Mutex + Notify) | Native (HashMap) |
| **Postgres** | Native (table + LISTEN/NOTIFY) | SDK default (on PG cache) | Native (pg_advisory_lock) | SDK default (on PG cache) |
| **K8s** | Native (CRD + resourceVersion) | Native (Lease API) | Native (Lease API) | Native (EndpointSlice) |
| **Redis** | Native (GET/SET/Lua) | SDK default (on Redis cache) | Native (SET NX EX + Lua) | SDK default (on Redis cache) |
| **NATS KV** | Native (KV bucket + revision) | SDK default (on NATS cache) | SDK default (on NATS cache) | SDK default (on NATS cache) |
| **etcd** | Native (KV + mod_revision) | Native (election API) | Native (lock API) | SDK default (on etcd cache) |

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
| K8s, low-throughput | `provider: k8s` | K8s CRD | K8s Lease | K8s Lease | K8s EndpointSlice | Zero new infra |
| K8s + Redis (recommended) | hybrid | Redis | K8s Lease | Redis | K8s EndpointSlice | Best of both |
| Redis-only | `provider: redis` | Redis | SDK default | Redis | SDK default | Single infra dep |

### 4.3 Existing Code Migration

The following existing code overlaps with datagrid capabilities and will be migrated in **separate follow-up changes**:

| Existing Code | Location | Overlap | Migration Plan |
|------|----------|---------|---|
| `LeaderElector` trait + `K8sLeaseElector` | `mini-chat/src/infra/leader/` | Leader election (production-quality K8s Lease impl) | Extract into K8s datagrid plugin; mini-chat consumes via `Datagrid` |
| File-based advisory locks | `libs/modkit-db/src/advisory_locks.rs` | Distributed lock (single-host only, no fencing) | Not reusable — datagrid provides true distributed locks. Modules migrate on adoption. |
| `NodesRegistryClient` | `modules/system/nodes-registry/` | Service discovery (node-specific, in-memory) | Nodes registry may become a consumer of datagrid service discovery |

## 5. Risks / Trade-offs

**Abstraction leakage**: Different backends have different consistency guarantees (Redis RedLock is "probably correct", Postgres advisory locks are strictly serializable). Trait docs define minimum guarantees; providers document actuals; integration tests verify contract.

**Standalone hides distributed bugs**: Code tested only against standalone may fail under real distribution. Mitigated by chaos mode in standalone (artificial delays/failures) and feature-gated integration tests against real infrastructure.

**Cache watch notification ordering**: Different backends may deliver events in different orders under concurrent writes. Contract specifies per-key ordering only; no cross-key guarantee.

**K8s CRD name limitations**: K8s resource names must be DNS-compatible (lowercase, max 253 chars). Provider translates `/` to `--`, lowercases, hashes long keys. Mapping is deterministic and documented.

**Postgres NOTIFY payload limit**: 8KB cap. Events carry only key + event type (under 100 bytes). No value in events (D11).

**Hybrid config complexity**: Operators could create confusing backend combinations. Mitigated by shorthand config for common case; per-capability overrides are opt-in; documentation provides recommended combinations.

## 6. Open Questions

- **Q1 (Resolved: Yes)**: Should `ServiceDiscovery` support metadata on service instances? Yes — `HashMap<String, String>` metadata field on registration.
- **Q2 (Resolved: Removed)**: Should pub/sub support message acknowledgment? No longer relevant — pub/sub removed; cache notifications are fire-and-forget observation.
- **Q3**: Exact placement in the module host lifecycle — should datagrid initialize before or after database migrations? Before seems correct (leader election could gate migrations), but needs validation.
