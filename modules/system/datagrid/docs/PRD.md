# PRD — Datagrid


<!-- toc -->

- [1. Overview](#1-overview)
  - [1.1 Purpose](#11-purpose)
  - [1.2 Background / Problem Statement](#12-background--problem-statement)
  - [1.3 Goals (Business Outcomes)](#13-goals-business-outcomes)
  - [1.4 Glossary](#14-glossary)
- [2. Actors](#2-actors)
  - [2.1 Human Actors](#21-human-actors)
  - [2.2 System Actors](#22-system-actors)
- [3. Operational Concept & Environment](#3-operational-concept--environment)
  - [3.1 Module-Specific Environment Constraints](#31-module-specific-environment-constraints)
- [4. Scope](#4-scope)
  - [4.1 In Scope](#41-in-scope)
  - [4.2 Out of Scope](#42-out-of-scope)
- [5. Functional Requirements](#5-functional-requirements)
  - [5.1 P1 — Distributed Cache](#51-p1--distributed-cache)
  - [5.2 P1 — Leader Election](#52-p1--leader-election)
  - [5.3 P1 — Distributed Locks](#53-p1--distributed-locks)
  - [5.4 P1 — Service Discovery](#54-p1--service-discovery)
  - [5.5 P1 — Hybrid Composition and SDK Defaults](#55-p1--hybrid-composition-and-sdk-defaults)
  - [5.6 P1 — Provider Resolution and Lifecycle](#56-p1--provider-resolution-and-lifecycle)
- [6. Non-Functional Requirements](#6-non-functional-requirements)
  - [6.1 Module-Specific NFRs](#61-module-specific-nfrs)
- [7. Public Library Interfaces](#7-public-library-interfaces)
  - [7.1 Public API Surface](#71-public-api-surface)
  - [7.2 External Integration Contracts](#72-external-integration-contracts)
- [8. Use Cases](#8-use-cases)
- [9. Acceptance Criteria](#9-acceptance-criteria)
- [10. Dependencies](#10-dependencies)
- [11. Assumptions](#11-assumptions)
- [12. Risks](#12-risks)
- [13. Open Questions](#13-open-questions)
- [14. Traceability](#14-traceability)

<!-- /toc -->

<!--
=============================================================================
PRODUCT REQUIREMENTS DOCUMENT (PRD)
=============================================================================
PURPOSE: Define WHAT the system must do and WHY — business requirements,
functional capabilities, and quality attributes.

SCOPE:
  ✓ Business goals and success criteria
  ✓ Actors (users, systems) that interact with this module
  ✓ Functional requirements (WHAT, not HOW)
  ✓ Non-functional requirements (quality attributes, SLOs)
  ✓ Scope boundaries (in/out of scope)
  ✓ Assumptions, dependencies, risks

NOT IN THIS DOCUMENT (see other templates):
  ✗ Stakeholder needs (managed at project/task level by steering committee)
  ✗ Technical architecture, design decisions → DESIGN.md
  ✗ Why a specific technical approach was chosen → ADR/
  ✗ Detailed implementation flows, algorithms → features/

STANDARDS ALIGNMENT:
  - IEEE 830 / ISO/IEC/IEEE 29148:2018 (requirements specification)
  - IEEE 1233 (system requirements)
  - ISO/IEC 15288 / 12207 (requirements definition)

REQUIREMENT LANGUAGE:
  - Use "MUST" or "SHALL" for mandatory requirements (implicit default)
  - Do not use "SHOULD" or "MAY" — use priority p2/p3 instead
  - Be specific and clear; no fluff, bloat, duplication, or emoji
=============================================================================
-->

## 1. Overview

### 1.1 Purpose

Datagrid is a platform-level system module providing cluster coordination and shared-state primitives to all CyberFabric modules. It bundles four sub-capabilities — distributed cache (KV with TTL, version-based CAS, and watch notifications), leader election, distributed locks with fencing tokens, and service discovery — behind a unified trait resolved through ClientHub.

### 1.2 Background / Problem Statement

CyberFabric modules increasingly need cluster-level coordination and shared-state primitives. The event broker requires leader election for worker pool coordination and cache-based shard assignment. OAGW needs distributed locks for rate limiting and shared counters via cache CAS. Future OOP (out-of-process) deployments require service discovery for module-to-module routing.

Today each module either reinvents these primitives or simply lacks them, making true multi-instance and OOP deployments unreliable. Existing code fragments (K8s Lease-based leader election in mini-chat, file-based advisory locks in modkit-db, in-memory nodes registry) are not reusable across modules and lack consistent abstractions.

A single, unified datagrid abstraction registered via the ModKit plugin pattern lets every module consume cluster coordination through one provider per deployment, matching the platform's existing extensibility model used by authn-resolver, authz-resolver, credstore, and tenant-resolver. The hybrid compositor architecture allows operators to mix backends (e.g., Redis for cache, K8s Lease for leader election) without changing consumer code.

### 1.3 Goals (Business Outcomes)

- Enable reliable multi-instance deployments by providing consistent cluster coordination primitives to all modules
- Eliminate duplicated coordination code across modules by offering a single reusable abstraction
- Support zero-infrastructure development and testing via an in-process standalone provider
- Allow operators to select the best backend per sub-capability without changing application code
- Enable OOP module-to-module routing through built-in service discovery

### 1.4 Glossary

| Term | Definition |
|------|------------|
| Sub-capability | One of the four coordination primitives: cache, leader election, distributed lock, service discovery |
| CacheEntry | A versioned key-value pair: value (opaque bytes) and version (monotonically increasing `u64` per key, starting at 1) |
| CacheEvent | Lightweight notification carrying only the key and event type (`Changed`, `Deleted`, `Expired`) — no value payload |
| CAS (compare-and-swap) | Atomic conditional update: succeeds only if the current version matches the expected version obtained from a prior `get()` |
| Fencing token | A strictly monotonic `u64` issued with each lock acquisition. Used to detect stale lock holders per the Kleppmann pattern |
| LeaderStatus | Election state: `Leader`, `Follower`, or `Lost` (terminal — consumer must re-elect) |
| LockGuard | RAII lock handle providing `fencing_token()` and `extend(additional_ttl)`. Drop releases the lock |
| CapabilityClass | Provider performance tier: `Standalone` (in-process), `Standard` (1k-10k ops/sec), `HighThroughput` (10k-200k ops/sec), `Coordination` (low volume, strong consistency). Used for startup validation against module-declared profiles |
| Hybrid compositor | Component that routes each sub-capability independently to a different backend provider |
| SDK default implementation | Built-in implementation of leader election, distributed lock, or service discovery using only cache CAS operations. Allows minimal providers to implement only cache |
| Profile | A named configuration mapping that modules declare in their own config. The operator binds profiles to providers. The SDK resolver validates the provider's `CapabilityClass` meets the profile's minimum |

## 2. Actors

### 2.1 Human Actors

#### Platform Operator

**ID**: `cpt-cf-dgrd-actor-operator`

<!-- cpt-cf-id-content -->
**Role**: Configures datagrid provider selection and hybrid routing. Maps profiles to providers. Selects backends appropriate for the deployment environment (standalone for dev, Redis/Postgres/K8s for production).
**Needs**: Ability to configure which backend handles each sub-capability. Shorthand config for common single-provider deployments. Validation that selected providers meet module-declared performance requirements.
<!-- cpt-cf-id-content -->

### 2.2 System Actors

#### Event Broker

**ID**: `cpt-cf-dgrd-actor-event-broker`

<!-- cpt-cf-id-content -->
**Role**: Consumes leader election for worker pool coordination and cache for shard-assignment state. Requires strong leader guarantees and reactive notifications for assignment changes.
<!-- cpt-cf-id-content -->

#### Outbound API Gateway (OAGW)

**ID**: `cpt-cf-dgrd-actor-oagw`

<!-- cpt-cf-id-content -->
**Role**: Consumes distributed locks for rate limiting and cache CAS for shared counters. Requires high-throughput cache operations and correct lock fencing.
<!-- cpt-cf-id-content -->

#### Platform Module

**ID**: `cpt-cf-dgrd-actor-platform-module`

<!-- cpt-cf-id-content -->
**Role**: Any internal module consuming datagrid primitives via ClientHub. Resolves `Datagrid` through the SDK resolver with a profile from its own config.
<!-- cpt-cf-id-content -->

#### Datagrid Backend

**ID**: `cpt-cf-dgrd-actor-backend`

<!-- cpt-cf-id-content -->
**Role**: External infrastructure providing cluster state persistence and coordination. Examples: Redis, PostgreSQL, Kubernetes API server, NATS, etcd. Accessed exclusively through plugins.
<!-- cpt-cf-id-content -->

## 3. Operational Concept & Environment

> **Note**: Project-wide runtime, OS, architecture, lifecycle policy, and integration patterns defined in root PRD. Document only module-specific deviations here.

### 3.1 Module-Specific Environment Constraints

- The standalone plugin requires no external infrastructure and is the default when no provider is configured
- Postgres plugin requires an active database connection pool
- K8s plugin requires in-cluster access to the Kubernetes API server with appropriate RBAC permissions (Lease, EndpointSlice, custom resources)
- Redis plugin requires network access to a Redis instance
- Only one provider configuration is active per named profile. The hybrid compositor can route sub-capabilities to different providers within a single profile
- Datagrid MUST initialize before modules that depend on it (e.g., leader election may gate database migrations)

## 4. Scope

### 4.1 In Scope

- Distributed cache: key-value storage with TTL, version-based CAS, put-if-absent, contains check, and reactive watch notifications (by exact key and prefix)
- Leader election: single-leader guarantee per named election, automatic renewal, leader-lost detection, watch-based status model
- Distributed locks: named locks with TTL, non-blocking try-lock and blocking acquire, fencing tokens for correctness, lock extension
- Service discovery: instance registration with metadata, health-aware discovery, topology change watches
- Hybrid compositor routing each sub-capability to independently configured backends
- SDK default implementations of leader election, distributed lock, and service discovery built on cache CAS
- Profile-based resolution with CapabilityClass validation at startup
- Standalone in-process plugin for dev/test (zero external dependencies)
- GTS registration and ClientHub wiring following ModKit plugin pattern
- Unified error type with structured provider error classification enabling programmatic retryability decisions

### 4.2 Out of Scope

- Explicit pub/sub messaging (provided by the event broker module — reliable delivery, consumer groups, offsets, replay)
- Data replication or sharding across multiple backend instances (delegated to the backend infrastructure)
- Transaction support across multiple cache keys
- Cache eviction policies (LRU, LFU) — delegated to backend
- Cross-key ordering guarantees for watch events (only per-key ordering is guaranteed)
- Automatic failover between backend providers
- Backend-specific configuration tuning (connection pools, timeouts — handled per plugin)
- Migration tooling for existing coordination code in other modules (tracked as separate follow-up changes)

## 5. Functional Requirements

### 5.1 P1 — Distributed Cache

#### Cache Put

- [ ] `p1` - **ID**: `cpt-cf-dgrd-fr-cache-put`

<!-- cpt-cf-id-content -->
The system **MUST** allow modules to store a value under a named key with an optional TTL. The key MUST conform to the `[a-zA-Z0-9_/-]+` pattern. The value is an opaque byte slice. If TTL is provided, the entry MUST expire after the specified duration. If TTL is `None`, the entry MUST persist until explicitly deleted or the provider restarts. Each put MUST increment the entry's version.

**Rationale**: Core shared-state primitive enabling all coordination patterns — counters, config propagation, shard assignments, token caching.
**Actors**: `cpt-cf-dgrd-actor-platform-module`, `cpt-cf-dgrd-actor-oagw`, `cpt-cf-dgrd-actor-event-broker`
<!-- cpt-cf-id-content -->

#### Cache Get

- [ ] `p1` - **ID**: `cpt-cf-dgrd-fr-cache-get`

<!-- cpt-cf-id-content -->
The system **MUST** allow modules to retrieve a versioned entry by key. The result MUST include both the value and a monotonically increasing version number. Missing or expired keys MUST return `None` (not an error).

**Rationale**: Consumers need the version for CAS operations and the value for application logic.
**Actors**: `cpt-cf-dgrd-actor-platform-module`, `cpt-cf-dgrd-actor-oagw`, `cpt-cf-dgrd-actor-event-broker`
<!-- cpt-cf-id-content -->

#### Cache Delete

- [ ] `p1` - **ID**: `cpt-cf-dgrd-fr-cache-delete`

<!-- cpt-cf-id-content -->
The system **MUST** allow modules to delete a key. The operation MUST return `true` if the key existed and was removed, `false` otherwise.

**Rationale**: Enables explicit cleanup of shared state entries.
**Actors**: `cpt-cf-dgrd-actor-platform-module`
<!-- cpt-cf-id-content -->

#### Cache Put-If-Absent

- [ ] `p1` - **ID**: `cpt-cf-dgrd-fr-cache-put-if-absent`

<!-- cpt-cf-id-content -->
The system **MUST** provide an atomic conditional insert operation. If the key does not exist, the value MUST be stored and the new entry (with version) returned. If the key already exists, the operation MUST be a no-op and return `None`.

**Rationale**: Enables atomic leader candidacy, lock acquisition, and idempotent initialization without race conditions.
**Actors**: `cpt-cf-dgrd-actor-platform-module`, `cpt-cf-dgrd-actor-event-broker`
<!-- cpt-cf-id-content -->

#### Cache Compare-and-Swap

- [ ] `p1` - **ID**: `cpt-cf-dgrd-fr-cache-cas`

<!-- cpt-cf-id-content -->
The system **MUST** provide an atomic version-based compare-and-swap operation. The operation accepts a key, an expected version (obtained from a prior `get()`), a new value, and an optional TTL. If the current version matches `expected_version`, the value MUST be atomically replaced and the new entry returned. If the version does not match, the operation MUST return a conflict error with the current entry.

**Rationale**: Enables optimistic concurrency for shared counters, distributed rate limiting, and forms the foundation for SDK default implementations of leader election and locks.
**Actors**: `cpt-cf-dgrd-actor-platform-module`, `cpt-cf-dgrd-actor-oagw`, `cpt-cf-dgrd-actor-event-broker`
<!-- cpt-cf-id-content -->

#### Cache Contains

- [ ] `p1` - **ID**: `cpt-cf-dgrd-fr-cache-contains`

<!-- cpt-cf-id-content -->
The system **MUST** provide an existence check operation that returns `true` if the key exists and has not expired, `false` otherwise. This MUST NOT retrieve the value.

**Rationale**: Lightweight existence check for cases where the value is not needed.
**Actors**: `cpt-cf-dgrd-actor-platform-module`
<!-- cpt-cf-id-content -->

#### Cache Watch by Exact Key

- [ ] `p1` - **ID**: `cpt-cf-dgrd-fr-cache-watch-key`

<!-- cpt-cf-id-content -->
The system **MUST** provide a watch operation for an exact key that yields change events (`Changed`, `Deleted`, `Expired`). Events carry only the key — no value payload. Dropping the watch handle MUST unsubscribe the watcher. Per-key event ordering MUST be preserved.

**Rationale**: Enables reactive change propagation for config flags, shard assignments, and leader status without polling.
**Actors**: `cpt-cf-dgrd-actor-platform-module`, `cpt-cf-dgrd-actor-event-broker`
<!-- cpt-cf-id-content -->

#### Cache Watch by Key Prefix

- [ ] `p1` - **ID**: `cpt-cf-dgrd-fr-cache-watch-prefix`

<!-- cpt-cf-id-content -->
The system **MUST** provide a watch operation for all keys matching a given prefix. The prefix MUST conform to the same naming pattern as keys. Events carry only the key and event type.

**Rationale**: Enables watching entire namespaces (e.g., all shard assignments under `event-broker/shard-assignments/`) without individual key subscriptions.
**Actors**: `cpt-cf-dgrd-actor-platform-module`, `cpt-cf-dgrd-actor-event-broker`
<!-- cpt-cf-id-content -->

#### Cache Event Types

- [ ] `p1` - **ID**: `cpt-cf-dgrd-fr-cache-event-types`

<!-- cpt-cf-id-content -->
The system **MUST** define three cache event types: `Changed` (key created or updated), `Deleted` (key explicitly removed), and `Expired` (key removed due to TTL). Events are lightweight notifications carrying only the key. Consumers that need the current value after a `Changed` event MUST call `get(key)`.

**Rationale**: Lightweight events avoid stale-value issues, map cleanly to all backends, and keep events fixed-size.
**Actors**: `cpt-cf-dgrd-actor-platform-module`
<!-- cpt-cf-id-content -->

#### Key Name Validation

- [ ] `p1` - **ID**: `cpt-cf-dgrd-fr-key-validation`

<!-- cpt-cf-id-content -->
The system **MUST** validate all names (cache keys, election names, lock names, service names) against the `[a-zA-Z0-9_/-]+` pattern at the trait boundary. Hierarchical namespacing uses `/` as separator. Invalid names MUST be rejected with a validation error before reaching any provider.

**Rationale**: Consistent naming across all sub-capabilities and backends prevents mapping issues (e.g., K8s DNS name restrictions, Postgres column constraints).
**Actors**: `cpt-cf-dgrd-actor-platform-module`
<!-- cpt-cf-id-content -->

### 5.2 P1 — Leader Election

#### Elect Leader

- [ ] `p1` - **ID**: `cpt-cf-dgrd-fr-leader-elect`

<!-- cpt-cf-id-content -->
The system **MUST** provide a leader election operation that takes a named election and returns a watch handle yielding `LeaderStatus` changes (`Leader`, `Follower`, `Lost`). At most one participant across all nodes MUST hold `Leader` status for a given election name at any time. The system MUST automatically renew leadership. Dropping the watch handle MUST step down from leadership.

**Rationale**: Worker pool coordination (event broker), singleton task scheduling, and migration gating all require single-leader guarantees.
**Actors**: `cpt-cf-dgrd-actor-event-broker`, `cpt-cf-dgrd-actor-platform-module`
<!-- cpt-cf-id-content -->

#### Configurable Election Timing

- [ ] `p1` - **ID**: `cpt-cf-dgrd-fr-leader-config`

<!-- cpt-cf-id-content -->
The system **MUST** allow callers to specify custom TTL and renewal interval for leader elections. Default TTL MUST be 30 seconds. Default renewal interval MUST be TTL divided by 3. The `Lost` status MUST be terminal — the consumer MUST create a new election to re-compete.

**Rationale**: Different use cases require different timing: fast failover for worker pools vs conservative timing for migration gating.
**Actors**: `cpt-cf-dgrd-actor-event-broker`, `cpt-cf-dgrd-actor-platform-module`
<!-- cpt-cf-id-content -->

### 5.3 P1 — Distributed Locks

#### Try Lock (Non-Blocking)

- [ ] `p1` - **ID**: `cpt-cf-dgrd-fr-lock-try`

<!-- cpt-cf-id-content -->
The system **MUST** provide a non-blocking lock acquisition operation. If the lock is available, the system MUST return a guard with a monotonic fencing token. If the lock is held, the system MUST return a contention error immediately.

**Rationale**: Non-blocking acquisition enables rate limiting and resource guarding where waiting is unacceptable.
**Actors**: `cpt-cf-dgrd-actor-oagw`, `cpt-cf-dgrd-actor-platform-module`
<!-- cpt-cf-id-content -->

#### Lock with Timeout (Blocking)

- [ ] `p1` - **ID**: `cpt-cf-dgrd-fr-lock-blocking`

<!-- cpt-cf-id-content -->
The system **MUST** provide a blocking lock acquisition operation that waits up to a specified timeout. If the lock is not acquired within the timeout, the system MUST return a timeout error.

**Rationale**: Some coordination patterns (exclusive resource access, serialized operations) require waiting for lock availability.
**Actors**: `cpt-cf-dgrd-actor-platform-module`
<!-- cpt-cf-id-content -->

#### Lock Guard with Fencing Token

- [ ] `p1` - **ID**: `cpt-cf-dgrd-fr-lock-guard`

<!-- cpt-cf-id-content -->
Each lock acquisition MUST return a guard providing a fencing token (`u64`), a lock extension operation, and automatic release on drop. The fencing token MUST be strictly monotonic per lock name across all acquisitions and all nodes. Attempting to extend an expired lock MUST return an expiry error.

**Rationale**: Fencing tokens enable the Kleppmann pattern for distributed lock correctness — downstream resources can reject operations from stale lock holders.
**Actors**: `cpt-cf-dgrd-actor-oagw`, `cpt-cf-dgrd-actor-platform-module`
<!-- cpt-cf-id-content -->

### 5.4 P1 — Service Discovery

#### Register Service Instance

- [ ] `p1` - **ID**: `cpt-cf-dgrd-fr-sd-register`

<!-- cpt-cf-id-content -->
The system **MUST** allow modules to register a service instance with a name, address, and optional metadata (`HashMap<String, String>`). An instance ID MUST be auto-generated if not provided. The registration MUST return a handle for deregistration, metadata updates, and health status changes. Dropping the handle MUST deregister the instance (best-effort).

**Rationale**: OOP deployments require modules to advertise their instances for routing.
**Actors**: `cpt-cf-dgrd-actor-platform-module`
<!-- cpt-cf-id-content -->

#### Discover Service Instances

- [ ] `p1` - **ID**: `cpt-cf-dgrd-fr-sd-discover`

<!-- cpt-cf-id-content -->
The system **MUST** provide operations to discover all instances of a named service and to discover only healthy instances. If no instances are registered, the result MUST be an empty list (not an error).

**Rationale**: Consumers need to find available instances for load balancing and routing.
**Actors**: `cpt-cf-dgrd-actor-platform-module`
<!-- cpt-cf-id-content -->

#### Watch Service Topology

- [ ] `p1` - **ID**: `cpt-cf-dgrd-fr-sd-watch`

<!-- cpt-cf-id-content -->
The system **MUST** provide a watch operation that yields topology change events (`Joined`, `Left`, `Updated`) for a named service. Consumers receive real-time membership changes without polling.

**Rationale**: Reactive topology awareness enables efficient connection pool management and load balancer updates.
**Actors**: `cpt-cf-dgrd-actor-platform-module`
<!-- cpt-cf-id-content -->

#### Service Health Management

- [ ] `p1` - **ID**: `cpt-cf-dgrd-fr-sd-health`

<!-- cpt-cf-id-content -->
Each service instance MUST have a health status (`Healthy`, `Unhealthy`, `Unknown`). The registration handle MUST allow updating the health status. The `discover_healthy` operation MUST return only instances with `Healthy` status.

**Rationale**: Health-aware discovery prevents routing to degraded instances.
**Actors**: `cpt-cf-dgrd-actor-platform-module`
<!-- cpt-cf-id-content -->

### 5.5 P1 — Hybrid Composition and SDK Defaults

#### Hybrid Compositor

- [ ] `p1` - **ID**: `cpt-cf-dgrd-fr-hybrid-compositor`

<!-- cpt-cf-id-content -->
The system **MUST** provide a compositor that routes each sub-capability (cache, leader election, distributed lock, service discovery) independently to a configured backend. Shorthand config (e.g., `provider: redis`) MUST route all sub-capabilities to a single backend. Per-capability overrides MUST be opt-in.

**Rationale**: Different backends excel at different primitives — Redis for cache throughput, K8s Lease for leader election consistency, EndpointSlice for native service discovery.
**Actors**: `cpt-cf-dgrd-actor-operator`
<!-- cpt-cf-id-content -->

#### SDK Default Implementations

- [ ] `p1` - **ID**: `cpt-cf-dgrd-fr-sdk-defaults`

<!-- cpt-cf-id-content -->
The SDK **MUST** provide default implementations of leader election, distributed lock, and service discovery built entirely on cache CAS operations. When a provider implements only the cache trait, the compositor MUST automatically use SDK defaults for the remaining sub-capabilities. When a provider offers native implementations, those MUST take precedence over SDK defaults.

**Rationale**: Minimizes the barrier for new provider implementations — a provider needs only implement cache to get all four sub-capabilities. Native overrides improve performance but are never required.
**Actors**: `cpt-cf-dgrd-actor-backend`
<!-- cpt-cf-id-content -->

#### Standalone Plugin as Default

- [ ] `p1` - **ID**: `cpt-cf-dgrd-fr-standalone-default`

<!-- cpt-cf-id-content -->
The system **MUST** include an in-process standalone plugin that implements all four sub-capabilities using only in-process primitives. This plugin MUST be the default when no provider is configured. It MUST have no external dependencies beyond the async runtime.

**Rationale**: Enables zero-infrastructure development and testing. All datagrid-aware code can be tested without external services.
**Actors**: `cpt-cf-dgrd-actor-platform-module`
<!-- cpt-cf-id-content -->

### 5.6 P1 — Provider Resolution and Lifecycle

#### Profile-Based Resolution

- [ ] `p1` - **ID**: `cpt-cf-dgrd-fr-profile-resolution`

<!-- cpt-cf-id-content -->
Modules **MUST** resolve `Datagrid` through the SDK resolver using a profile name from their own config. The operator maps profiles to providers in platform config. At resolution time, the resolver MUST validate that the bound provider's `CapabilityClass` meets the profile's declared minimum. A `resolve_default` operation MUST return the global default provider without requiring a profile.

**Rationale**: Profile-based resolution decouples module code from specific providers and enables startup validation that the deployment meets module performance requirements.
**Actors**: `cpt-cf-dgrd-actor-platform-module`, `cpt-cf-dgrd-actor-operator`
<!-- cpt-cf-id-content -->

#### Provider Lifecycle

- [ ] `p1` - **ID**: `cpt-cf-dgrd-fr-provider-lifecycle`

<!-- cpt-cf-id-content -->
The `Datagrid` trait **MUST** include `start()` and `shutdown()` lifecycle operations. The module host MUST call `start()` during initialization and `shutdown()` during graceful shutdown. Operations invoked before `start()` MUST return a not-started error. The hybrid compositor MUST delegate lifecycle calls to all held sub-capability implementations.

**Rationale**: Providers need to establish connections, start background tasks (renewal, TTL reaping), and clean up resources on shutdown.
**Actors**: `cpt-cf-dgrd-actor-platform-module`
<!-- cpt-cf-id-content -->

#### GTS Registration

- [ ] `p1` - **ID**: `cpt-cf-dgrd-fr-gts-registration`

<!-- cpt-cf-id-content -->
The datagrid provider **MUST** be registered in the Global Type System following the existing ModKit plugin pattern. Registration MUST occur during module host initialization, before modules that depend on datagrid are started. The provider MUST be resolvable via ClientHub by any module.

**Rationale**: Consistent with the platform's extensibility model used by authn-resolver, authz-resolver, credstore, and tenant-resolver.
**Actors**: `cpt-cf-dgrd-actor-platform-module`
<!-- cpt-cf-id-content -->

#### Unified Error Type

- [ ] `p1` - **ID**: `cpt-cf-dgrd-fr-error-type`

<!-- cpt-cf-id-content -->
The system **MUST** define a unified error type covering all sub-capability error conditions: invalid name, CAS conflict, lock contention, lock timeout, lock expiry, not-started, and shutdown. Provider-specific errors MUST be wrapped with a structured error kind (`ConnectionLost`, `Timeout`, `AuthFailure`, `ResourceExhausted`, `Other`) enabling programmatic retryability decisions without matching on backend-specific error strings.

**Rationale**: Consumers need to handle errors consistently regardless of which backend is active.
**Actors**: `cpt-cf-dgrd-actor-platform-module`
<!-- cpt-cf-id-content -->

## 6. Non-Functional Requirements

### 6.1 Module-Specific NFRs

#### Performance Tiers via CapabilityClass

- [ ] `p1` - **ID**: `cpt-cf-dgrd-nfr-capability-class`

<!-- cpt-cf-id-content -->
Each provider **MUST** self-declare a `CapabilityClass` tier: `Standalone` (in-process, dev/test only), `Standard` (1k-10k operations per second, suitable for Postgres/etcd), `HighThroughput` (10k-200k operations per second, suitable for Redis/NATS), or `Coordination` (low volume, strong consistency, suitable for K8s Lease/etcd). The SDK resolver **MUST** reject provider binding at startup if the provider's tier does not meet the module's declared minimum.

**Threshold**: Startup validation rejects mismatched capability class with a clear error message within 1 second
**Rationale**: Prevents runtime performance issues by validating deployment suitability at startup rather than under load.
**Architecture Allocation**: See DESIGN.md for implementation approach
<!-- cpt-cf-id-content -->

#### No Serde in Contract Types

- [ ] `p1` - **ID**: `cpt-cf-dgrd-nfr-no-serde`

<!-- cpt-cf-id-content -->
SDK trait definitions and shared types **MUST NOT** depend on serde. This follows the existing ModKit contract-layer convention enforced by dylint lints. Serialization concerns belong in provider implementations.

**Threshold**: Zero serde imports or derives in the SDK crate; enforced by `make check` (dylint lints)
**Rationale**: Contract types form the stable API boundary. Coupling them to serde forces all consumers and providers to agree on a serialization format and version.
**Architecture Allocation**: See DESIGN.md for implementation approach
<!-- cpt-cf-id-content -->

#### At-Most-One Leader Guarantee

- [ ] `p1` - **ID**: `cpt-cf-dgrd-nfr-leader-guarantee`

<!-- cpt-cf-id-content -->
At most one participant **MUST** hold `Leader` status for a given election name at any time across all nodes. Leader loss **MUST** be detected within the configured TTL. The guarantee applies to all providers including SDK default implementations.

**Threshold**: Zero split-brain occurrences under contention testing with 10+ concurrent candidates across 3+ nodes
**Rationale**: Split-brain leadership causes data corruption in worker pool coordination and shard assignment.
**Architecture Allocation**: See DESIGN.md for implementation approach
<!-- cpt-cf-id-content -->

#### Fencing Token Monotonicity

- [ ] `p1` - **ID**: `cpt-cf-dgrd-nfr-fencing-monotonic`

<!-- cpt-cf-id-content -->
Fencing tokens **MUST** be strictly monotonically increasing per lock name across all acquisitions and all nodes. A newer acquisition MUST always produce a higher token than any previous acquisition of the same lock.

**Threshold**: Zero token regression across 1000+ acquisition cycles under concurrent contention
**Rationale**: Non-monotonic tokens break the Kleppmann fencing pattern, allowing stale lock holders to corrupt protected resources.
**Architecture Allocation**: See DESIGN.md for implementation approach
<!-- cpt-cf-id-content -->

#### At-Most-Once Watch Delivery

- [ ] `p1` - **ID**: `cpt-cf-dgrd-nfr-watch-delivery`

<!-- cpt-cf-id-content -->
Cache watch events **MUST** be delivered at most once per subscriber. The system provides no delivery guarantee (events may be missed during network partitions or subscriber backpressure). Per-key ordering **MUST** be preserved. No cross-key ordering is guaranteed.

**Threshold**: Zero duplicate events per subscriber per key in normal operation; per-key ordering verified under concurrent writes
**Rationale**: At-most-once with per-key ordering maps cleanly to all backends (Redis keyspace notifications, Postgres NOTIFY, K8s watch, NATS KV watch). Reliable messaging with delivery guarantees belongs in the event broker.
**Architecture Allocation**: See DESIGN.md for implementation approach
<!-- cpt-cf-id-content -->

#### Standalone In-Process Latency

- [ ] `p1` - **ID**: `cpt-cf-dgrd-nfr-standalone-latency`

<!-- cpt-cf-id-content -->
The standalone plugin **MUST** provide sub-millisecond latency for all cache operations (get, put, delete, CAS) when running in-process.

**Threshold**: p99 latency under 1ms for all cache operations in single-process benchmarks
**Rationale**: Development and test workloads must not be bottlenecked by the coordination layer.
**Architecture Allocation**: See DESIGN.md for implementation approach
<!-- cpt-cf-id-content -->

## 7. Public Library Interfaces

### 7.1 Public API Surface

#### DatagridV1

- [ ] `p1` - **ID**: `cpt-cf-dgrd-interface-datagrid`

<!-- cpt-cf-id-content -->
**Type**: Rust trait (async)
**Stability**: stable
**Description**: Root trait providing access to all four sub-capabilities via accessor methods (`cache()`, `leader_election()`, `distributed_lock()`, `service_discovery()`), lifecycle management (`start()`, `shutdown()`), and capability class declaration. Registered in ClientHub. Resolved through `DatagridClient` SDK helper.
**Breaking Change Policy**: Major version bump required
<!-- cpt-cf-id-content -->

#### DatagridCacheV1

- [ ] `p1` - **ID**: `cpt-cf-dgrd-interface-cache`

<!-- cpt-cf-id-content -->
**Type**: Rust trait (async)
**Stability**: stable
**Description**: Cache sub-capability trait providing `get`, `put`, `delete`, `contains`, `put_if_absent`, `compare_and_swap`, `watch`, and `watch_prefix` operations. Returned by `Datagrid::cache()`.
**Breaking Change Policy**: Major version bump required
<!-- cpt-cf-id-content -->

#### DatagridPluginSpiV1

- [ ] `p1` - **ID**: `cpt-cf-dgrd-interface-plugin-spi`

<!-- cpt-cf-id-content -->
**Type**: Rust traits (async)
**Stability**: unstable
**Description**: Provider SPI consisting of all sub-capability traits. External providers implement one or more sub-capability traits and register them through the hybrid compositor. Minimal providers implement only `DatagridCache`; the SDK fills in the rest.
**Breaking Change Policy**: Minor version bump (unstable API)
<!-- cpt-cf-id-content -->

### 7.2 External Integration Contracts

#### Kubernetes API

- [ ] `p1` - **ID**: `cpt-cf-dgrd-contract-k8s-api`

<!-- cpt-cf-id-content -->
**Direction**: required from client (outbound to K8s API server)
**Protocol/Format**: HTTP/REST, JSON. Uses `coordination.k8s.io/v1` Lease for leader election and locks, `EndpointSlice` for service discovery, Custom Resources for cache.
**Compatibility**: Plugin adapts to K8s API version. Requires RBAC permissions for Lease, EndpointSlice, and CRD operations.
<!-- cpt-cf-id-content -->

#### PostgreSQL

- [ ] `p2` - **ID**: `cpt-cf-dgrd-contract-postgres`

<!-- cpt-cf-id-content -->
**Direction**: required from client (outbound to PostgreSQL)
**Protocol/Format**: PostgreSQL wire protocol via connection pool. Uses tables for cache, `LISTEN/NOTIFY` for watch events, `pg_advisory_lock` for distributed locks.
**Compatibility**: Requires PostgreSQL 12+ for `LISTEN/NOTIFY` and advisory lock features.
<!-- cpt-cf-id-content -->

## 8. Use Cases

#### UC-001: Event Broker Elects Worker Pool Leader

- [ ] `p1` - **ID**: `cpt-cf-dgrd-usecase-worker-leader`

<!-- cpt-cf-id-content -->
**Actor**: `cpt-cf-dgrd-actor-event-broker`

**Preconditions**:
- Datagrid is started and provider is initialized
- Multiple event broker instances are running

**Main Flow**:
1. Each event broker instance calls `elect("event-broker/worker-pool")` on startup
2. Exactly one instance receives `LeaderStatus::Leader`; all others receive `Follower`
3. The leader manages shard assignments by writing to cache keys under `event-broker/shard-assignments/`
4. Followers watch the shard assignment prefix for changes
5. If the leader crashes, its lease expires within TTL
6. Remaining followers detect leader loss; one becomes the new leader

**Postconditions**:
- Exactly one event broker instance coordinates shard assignments at any time
- Shard assignment changes are propagated to all instances via cache watch

**Alternative Flows**:
- **Leader steps down gracefully**: Watch handle is dropped, leadership transfers immediately
- **Leader status becomes `Lost`**: Consumer re-enters election with a new `elect()` call
<!-- cpt-cf-id-content -->

#### UC-002: OAGW Acquires Rate Limit Lock

- [ ] `p1` - **ID**: `cpt-cf-dgrd-usecase-rate-limit`

<!-- cpt-cf-id-content -->
**Actor**: `cpt-cf-dgrd-actor-oagw`

**Preconditions**:
- Datagrid is started with a provider supporting distributed locks
- OAGW is processing an API request that requires rate limiting

**Main Flow**:
1. OAGW calls `try_lock("rate-limit/tenant-42/openai", Duration::from_secs(1))`
2. Lock is available — system returns a `LockGuard` with a fencing token
3. OAGW reads and increments the rate counter via cache CAS
4. OAGW drops the `LockGuard`, releasing the lock
5. Other OAGW instances can now acquire the lock

**Postconditions**:
- Rate counter is atomically incremented; no double-counting

**Alternative Flows**:
- **Lock is held**: `try_lock` returns `LockContended`; OAGW can retry or reject the request
- **Lock expires before release**: Next acquirer gets a higher fencing token; downstream resources reject stale-token operations
<!-- cpt-cf-id-content -->

#### UC-003: Module Resolves Datagrid with Profile Validation

- [ ] `p1` - **ID**: `cpt-cf-dgrd-usecase-profile-resolve`

<!-- cpt-cf-id-content -->
**Actor**: `cpt-cf-dgrd-actor-platform-module`

**Preconditions**:
- Module config declares `datagrid_profile: high-throughput`
- Operator config maps `high-throughput` profile to a Redis provider

**Main Flow**:
1. Module calls `DatagridClient::resolve(hub, "high-throughput")`
2. SDK resolver looks up the provider bound to the `high-throughput` profile
3. Resolver checks the provider's `CapabilityClass` (e.g., `HighThroughput`) against the profile's minimum
4. Validation passes — resolver returns `Arc<dyn Datagrid>`
5. Module uses `datagrid.cache()`, `datagrid.leader_election()`, etc.

**Postconditions**:
- Module has a validated datagrid instance matching its performance requirements

**Alternative Flows**:
- **Capability class mismatch**: Resolver returns an error at startup (e.g., module requires `HighThroughput` but only `Standalone` is configured)
- **No profile needed**: Module calls `resolve_default(hub)` for the global default provider
<!-- cpt-cf-id-content -->

#### UC-004: Hybrid Config Routes Sub-Capabilities to Different Backends

- [ ] `p1` - **ID**: `cpt-cf-dgrd-usecase-hybrid-routing`

<!-- cpt-cf-id-content -->
**Actor**: `cpt-cf-dgrd-actor-operator`

**Preconditions**:
- K8s cluster with Redis available
- Operator wants Redis for cache throughput and K8s Lease for leader election consistency

**Main Flow**:
1. Operator configures hybrid mode: `cache: redis`, `leader_election: k8s`
2. Wiring crate parses config and initializes Redis cache plugin and K8s leader election plugin
3. Compositor builds `HybridDatagridPlugin` with Redis cache, K8s leader election, and SDK defaults for lock and service discovery (built on Redis cache)
4. Provider is registered in GTS and ClientHub
5. Modules resolve a single `Arc<dyn Datagrid>` and use each sub-capability transparently

**Postconditions**:
- Cache operations route to Redis; leader elections route to K8s API
- Lock and service discovery use SDK defaults built on Redis cache

**Alternative Flows**:
- **Shorthand config**: `provider: redis` routes all sub-capabilities to Redis with SDK defaults for sub-capabilities Redis does not natively implement
<!-- cpt-cf-id-content -->

#### UC-005: Service Discovery for OOP Module Routing

- [ ] `p1` - **ID**: `cpt-cf-dgrd-usecase-service-discovery`

<!-- cpt-cf-id-content -->
**Actor**: `cpt-cf-dgrd-actor-platform-module`

**Preconditions**:
- Multiple instances of a module are running in OOP mode
- Datagrid is started with a provider supporting service discovery

**Main Flow**:
1. Each module instance calls `register(ServiceRegistration { name: "chat-engine", address: "10.0.1.5:8080", metadata: {...} })`
2. Registration returns a `ServiceHandle`; instance periodically calls `set_health(Healthy)`
3. A consumer module calls `discover_healthy("chat-engine")`
4. System returns only instances with `Healthy` status
5. Consumer watches for topology changes via `watch("chat-engine")`
6. When an instance shuts down, its handle is dropped — `Left` event is emitted to watchers

**Postconditions**:
- Consumer has an up-to-date view of healthy service instances
- Topology changes are propagated reactively

**Alternative Flows**:
- **No instances registered**: `discover` returns an empty list
- **Instance becomes unhealthy**: `Updated` event emitted; `discover_healthy` excludes it
<!-- cpt-cf-id-content -->

## 9. Acceptance Criteria

- [ ] Modules can store, retrieve, and delete cache entries with optional TTL through ClientHub
- [ ] Version-based CAS rejects stale updates and returns the current entry on conflict
- [ ] Cache watch by key and prefix delivers `Changed`, `Deleted`, and `Expired` events
- [ ] Leader election maintains at-most-one leader guarantee under concurrent contention across multiple tasks
- [ ] Distributed locks provide strictly monotonic fencing tokens across all acquisitions
- [ ] Service discovery registers, discovers (all and healthy-only), and watches instances
- [ ] Hybrid compositor routes sub-capabilities to independently configured backends
- [ ] SDK default implementations provide working leader election, locks, and service discovery using only cache operations
- [ ] Standalone plugin passes all functional tests with sub-millisecond cache latency
- [ ] Profile resolution validates provider CapabilityClass at startup and rejects mismatches
- [ ] Name validation rejects keys not matching `[a-zA-Z0-9_/-]+` before reaching providers
- [ ] Operations before `start()` return a not-started error
- [ ] No serde dependencies in SDK crate (enforced by dylint lints)

## 10. Dependencies

| Dependency | Description | Criticality |
|------------|-------------|-------------|
| ModKit | GTS registration, ClientHub wiring, plugin pattern | `p1` |
| `types-registry-sdk` | GTS instance discovery for plugin registration | `p1` |
| `tokio` | Async runtime, channels, mutexes (standalone plugin, SDK defaults) | `p1` |
| PostgreSQL | Database backend for Postgres plugin (tables, advisory locks, LISTEN/NOTIFY) | `p2` |
| Kubernetes API | Backend for K8s plugin (Lease, EndpointSlice, CRD) | `p2` |
| Redis | Backend for Redis plugin (KV, Lua scripts, keyspace notifications) | `p2` |

## 11. Assumptions

- The standalone plugin is sufficient for single-instance and development deployments
- A minimal provider implementing only `DatagridCache` can provide all four sub-capabilities via SDK defaults with acceptable performance
- Modules declare their performance requirements via profiles in their own config; operators bind profiles to providers in platform config
- The module host initializes datagrid before modules that depend on it
- Existing coordination code in other modules (mini-chat leader election, modkit-db advisory locks, nodes-registry) will be migrated to datagrid in separate follow-up changes
- Explicit pub/sub messaging with delivery guarantees, consumer groups, and replay is handled by the event broker, not datagrid
- Per-key event ordering is sufficient for all watch use cases; cross-key ordering is not required

## 12. Risks

| Risk | Impact | Mitigation |
|------|--------|------------|
| Abstraction leakage across backends | Different consistency guarantees (Redis RedLock vs Postgres advisory locks) may surprise consumers | Trait contracts define minimum guarantees; providers document actuals; integration tests verify contract |
| Standalone hides distributed bugs | Code tested only against standalone may fail under real distribution | Chaos mode in standalone (artificial delays/failures); feature-gated integration tests against real infrastructure |
| K8s resource name limitations | K8s names must be DNS-compatible (lowercase, 253 chars max), limiting key naming | Provider translates names deterministically; naming constraints documented |
| Postgres NOTIFY payload limit | 8KB cap on NOTIFY payload | Events carry only key + event type (under 100 bytes); no value in events |
| Hybrid config complexity | Operators could create confusing backend combinations | Shorthand config for common single-provider case; recommended deployment combinations documented |
| SDK default performance under load | CAS-based leader election and locks may have higher latency than native implementations | Native implementations override defaults for performance-critical deployments; CapabilityClass validation warns at startup |

## 13. Open Questions

- Exact placement in the module host lifecycle: datagrid likely initializes before database migrations (leader election could gate migrations), but the ordering needs validation with the module host team.
- Whether the standalone plugin needs a chaos/fault-injection mode for testing distributed failure scenarios from the initial release, or if this can be a follow-up.

## 14. Traceability

- **Design**: [DESIGN.md](./DESIGN.md)
- **ADRs**: [ADR/](./ADR/)
- **Features**: [features/](./features/)
