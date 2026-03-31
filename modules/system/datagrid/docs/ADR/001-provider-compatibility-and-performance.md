# ADR-001: Provider Compatibility and Performance Analysis

**Status**: Accepted
**Date**: 2026-03-31

**ID**: `cpt-cf-dgrd-adr-provider-compat-perf`

<!-- toc -->

- [Context and Problem Statement](#context-and-problem-statement)
- [Decision Drivers](#decision-drivers)
- [Considered Options](#considered-options)
- [Decision Outcome](#decision-outcome)
  - [Consequences](#consequences)
  - [Confirmation](#confirmation)
- [Pros and Cons of the Options](#pros-and-cons-of-the-options)
  - [Redis](#redis)
  - [PostgreSQL](#postgresql)
  - [K8s API (Lease + CRD + EndpointSlice)](#k8s-api-lease--crd--endpointslice)
  - [NATS KV](#nats-kv)
  - [etcd (Direct)](#etcd-direct)
- [More Information](#more-information)

<!-- /toc -->

## Context and Problem Statement

The datagrid abstraction must map cleanly to multiple backends while meeting real-world performance requirements. Different CyberFabric modules have radically different coordination workloads:

- **Event broker**: Up to 1000 subscriber leases per instance, renewed every few seconds. Multi-instance deployments multiply this — 10 instances = 10,000 concurrent leases.
- **OAGW**: Distributed rate-limit counters (cache CAS) and a handful of configuration locks. Low lock count, high-frequency cache updates.
- **Scheduler**: A single leader election per pool. One lock, but correctness-critical.
- **Service discovery**: Dozens to hundreds of service registrations with periodic health updates.

These workloads span four orders of magnitude in concurrency. A provider that handles 5 leader elections perfectly may collapse under 10,000 subscriber leases. We need to select and recommend providers based on real performance characteristics, not just API compatibility.

Additionally, the subscriber lease use case (1000 concurrent reservations per instance) reveals that the right primitive is `DatagridCache` (put_if_absent + TTL renewal), not `DistributedLock`. This reframing avoids the lock scalability problem entirely.

## Decision Drivers

- Throughput: some workloads require 10,000+ ops/sec (subscriber leases, rate-limit counters), others need < 10 ops/sec (leader elections)
- Latency: sub-millisecond for hot-path cache operations, seconds-acceptable for leader election
- Infrastructure cost: Postgres and K8s are already deployed; Redis/NATS/etcd add operational overhead
- Correctness: leader election requires at-most-one guarantee; locks require fencing tokens; CAS requires atomicity
- Watch/notification overhead: backends vary dramatically in their ability to push change events at scale
- Connection model: Postgres connections are expensive (~5-10MB each); Redis connections are cheap (~20KB each); K8s API server has rate limiting

## Considered Options

1. **Redis** — high-throughput cache + locks, weak watch (keyspace notifications)
2. **PostgreSQL** — zero-new-infra, moderate throughput, strong consistency via advisory locks
3. **K8s API** — zero-new-infra on K8s, native leader election (Lease) and service discovery (EndpointSlice), poor throughput ceiling
4. **NATS KV** — best-in-class watch, native CAS via revisions, adds infrastructure dependency
5. **etcd (direct)** — purpose-built coordination, strong consistency, limited throughput

## Decision Outcome

Adopt a **hybrid model** where each sub-capability routes to the best backend for the workload. No single backend is optimal across all four sub-capabilities and all workload scales. The recommended combinations are:

- **Dev / single-instance**: Standalone (all four, zero deps)
- **Multi-instance, no K8s**: Postgres (cache + locks natively, election + discovery via SDK defaults)
- **K8s, low-throughput**: K8s (Lease for election, EndpointSlice for discovery, CRD for cache)
- **K8s + Redis (recommended production)**: Redis for cache + locks, K8s for election + service discovery
- **Redis-only**: Redis for all four via SDK defaults

Profile-based resolution (D12) allows modules to declare their performance needs. The operator maps profiles to providers. Startup validation ensures the bound provider's `CapabilityClass` meets the profile's minimum.

The event broker's subscriber lease management uses `DatagridCache` (not `DistributedLock`) — cache CAS is the higher-throughput primitive on every backend.

### Consequences

- K8s-only deployments have a clear ceiling for cache/lock workloads (~5,000 writes/sec etcd practical limit). Documentation must recommend adding Redis when workloads exceed this.
- Module authors need guidance: cache for high-count reservations, locks for low-count mutual exclusion, elections for singleton coordination. The wrong choice (locks for 1000 subscribers) performs poorly regardless of backend.
- Configuration surface increases slightly with profile-based routing and optional prefix-based overrides within a capability.
- The prefix-routing feature is optional — single-provider shorthand (`provider: redis`) remains the simplest configuration.

### Confirmation

- Integration tests per provider validate throughput and latency against documented CapabilityClass tiers
- Event broker integration tests confirm subscriber lease management via `DatagridCache` (not locks) sustains 10,000 concurrent leases on Redis
- K8s provider integration tests confirm Lease-based election works at 50 concurrent elections and CRD-based cache works at 100 entries
- Postgres provider integration tests confirm advisory lock performance at 1000 concurrent locks per connection

## Pros and Cons of the Options

### Redis

**Performance envelope**: 100k–200k ops/sec (SET NX EX, single node, no pipeline). ~0.15ms p50, ~0.5ms p99. 1M concurrent entries ≈ 180MB memory. Lua CAS adds ~4-5% overhead per call.

- Good, because cache and lock throughput is 10-100× higher than any other backend
- Good, because native per-key TTL via SET EX — no reaper needed
- Good, because memory overhead per lock entry is ~160 bytes — 10,000 locks costs 1.6MB
- Good, because Lua script CAS is atomic and the canonical Redis pattern
- Bad, because keyspace notifications (for watch) are disabled by default, CPU overhead is unquantified, and managed Redis may restrict CONFIG SET
- Bad, because Redis Cluster pre-7.0 PUBLISH broadcasts to ALL nodes — throughput ceiling ~12,500 publishes/sec on 10-node cluster
- Bad, because RedLock has known correctness issues (Kleppmann) — use single-node + Sentinel instead
- Neutral, because adds an infrastructure dependency (but most production stacks already have Redis)

### PostgreSQL

**Performance envelope**: ~10k-50k cache ops/sec (depends on pool size and hardware). Advisory lock acquire: ~0.01-0.05ms server-side. LISTEN/NOTIFY handles thousands of notifications/sec but has global commit lock under concurrent writers.

- Good, because zero new infrastructure — every CyberFabric deployment has Postgres
- Good, because advisory locks are server-enforced, ACID, auto-release on disconnect
- Good, because a single connection can hold thousands of advisory locks simultaneously (~164KB for 1000 locks)
- Good, because version-based CAS maps natively to `UPDATE WHERE version = $expected` — zero Lua
- Bad, because cache throughput is 10-100× lower than Redis (bounded by connection pool and write latency)
- Bad, because LISTEN/NOTIFY has a global exclusive lock on commit — degrades under high concurrent writer + notify rates
- Bad, because each Postgres connection costs ~5-10MB server memory — scaling to many connections is expensive
- Bad, because session-level advisory locks are incompatible with PgBouncer transaction mode
- Neutral, because no native per-key TTL — needs reaper task (but straightforward)

### K8s API (Lease + CRD + EndpointSlice)

**Performance envelope**: 2-10ms per operation (API server → etcd round-trip). etcd practical write ceiling: ~3,000-5,000 sustained writes/sec under K8s workloads. etcd 8GiB database size limit.

- Good, because zero new infrastructure when running on K8s
- Good, because Lease API is purpose-built for leader election — battle-tested, used by K8s controllers
- Good, because EndpointSlice is the gold standard for service discovery — DNS integration, health-aware, native watch
- Good, because resourceVersion is native CAS on every K8s object
- Bad, because every operation traverses API server → etcd at 2-10ms — not suitable for high-throughput caching
- Bad, because 1000 Lease objects renewed every 10s = 1000 PUTs/sec — consumes 20-33% of etcd write capacity, competes with K8s control-plane operations
- Bad, because K8s issue #47532 documented 100K+ leases causing etcd instability — the "many leases" pattern is a known anti-pattern
- Bad, because etcd issue #18109 (slow watchers blocking writes, 1-2s stalls) is still present in etcd 3.5.x
- Bad, because no native per-key TTL on CRDs — needs reaper controller
- Bad, because API Priority and Fairness may throttle custom Lease operations under load

### NATS KV

**Performance envelope**: 100k+ ops/sec (memory-backed, single-node). File-backed with R=3: 10k-50k ops/sec.

- Good, because native watch (`kv.watch(key)`) is best-in-class — built-in, event-driven, efficient
- Good, because revision-based CAS (`kv.update(key, val, revision)`) maps directly to our version-based CAS
- Good, because `kv.create(key, val)` is native put-if-absent
- Bad, because per-key TTL is bucket-level only (per-key via headers added in NATS 2.10+ but not universally supported)
- Bad, because no native distributed locks or leader election — SDK defaults required
- Bad, because adds an infrastructure dependency that most stacks don't already have
- Neutral, because throughput depends heavily on JetStream storage mode (memory vs file) and replication factor

### etcd (Direct)

**Performance envelope**: ~3,000-5,000 sustained writes/sec (K8s-like workload). <1ms p50 on SSD. Native election, lock, and watch APIs.

- Good, because purpose-built for distributed coordination — linearizable reads and writes
- Good, because native election, lock, and prefix-based watch APIs
- Good, because transactional CAS via `txn(mod_revision == expected → put)` is atomic and server-enforced
- Bad, because not designed for high-volume data caching — 8GiB database limit
- Bad, because requires dedicated infrastructure (sharing K8s etcd is dangerous — competes with control-plane)
- Bad, because each write goes through Raft consensus — replication latency scales with cluster size
- Neutral, because strong consistency guarantees are excellent for coordination but overkill for cache-tier workloads

## More Information

- [Martin Kleppmann — How to do distributed locking](https://martin.kleppmann.com/2016/02/08/how-to-do-distributed-locking.html) — RedLock correctness analysis
- [K8s issue #47532 — events create too many leases on etcd](https://github.com/kubernetes/kubernetes/issues/47532)
- [etcd issue #18109 — slow watchers block PUT latency](https://github.com/etcd-io/etcd/issues/18109)
- [KEP-589 — Efficient Node Heartbeats](https://github.com/kubernetes/enhancements/blob/master/keps/sig-node/589-efficient-node-heartbeats/README.md)
- [Recall.ai — Postgres LISTEN/NOTIFY does not scale](https://www.recall.ai/blog/postgres-listen-notify-does-not-scale)
- [Redis benchmark docs](https://redis.io/docs/latest/operate/oss_and_stack/management/optimization/benchmarks/)
- [etcd performance docs](https://etcd.io/docs/v3.4/op-guide/performance/)
