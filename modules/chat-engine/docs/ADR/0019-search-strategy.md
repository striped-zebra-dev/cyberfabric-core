Created:  2026-02-04 by Constructor Tech
Updated:  2026-03-06 by Constructor Tech
# ADR-0019: PostgreSQL Full-Text Search with GIN Indexes


<!-- toc -->

- [Context and Problem Statement](#context-and-problem-statement)
- [Decision Drivers](#decision-drivers)
- [Considered Options](#considered-options)
- [Decision Outcome](#decision-outcome)
  - [Consequences](#consequences)
  - [Confirmation](#confirmation)
- [Pros and Cons of the Options](#pros-and-cons-of-the-options)
  - [Option 1: PostgreSQL tsvector with GIN indexes](#option-1-postgresql-tsvector-with-gin-indexes)
  - [Option 2: Elasticsearch](#option-2-elasticsearch)
  - [Option 3: Simple LIKE queries](#option-3-simple-like-queries)
- [Related Design Elements](#related-design-elements)

<!-- /toc -->

**Date**: 2026-02-04

**Status**: accepted

**Review**: Revisit if full-text search needs multi-language or fuzzy matching

**ID**: `cpt-cf-chat-engine-adr-search-strategy`

## Context and Problem Statement

Users need to search conversation history within sessions and across sessions to find relevant messages. How should Chat Engine implement full-text search to balance query performance, relevance ranking, and infrastructure simplicity?

## Decision Drivers

* Search within session (< 1 second for 10K messages)
* Search across sessions (< 3 seconds for 1K sessions)
* Relevance ranking (most relevant results first)
* Case-insensitive search with stemming
* Phrase search support ("exact match")
* Pagination for large result sets
* Infrastructure simplicity (avoid separate search engine)
* Leverage existing PostgreSQL database

## Considered Options

* **Option 1: PostgreSQL tsvector with GIN indexes** - Native full-text search with tsvector, GIN indexing, ts_rank_cd
* **Option 2: Elasticsearch** - Dedicated search engine with advanced features
* **Option 3: Simple LIKE queries** - Basic pattern matching with ILIKE

## Decision Outcome

Chosen option: "PostgreSQL tsvector with GIN indexes", because it provides built-in full-text search without additional infrastructure, GIN indexes enable fast queries meeting performance requirements, ts_rank_cd relevance ranking with document length normalization, case-insensitive search with English stemming, and cursor-based pagination for consistency.

### Consequences

* Good, because no additional infrastructure (search within PostgreSQL)
* Good, because GIN indexes provide fast full-text queries
* Good, because ts_rank_cd ranking considers document length
* Good, because case-insensitive with stemming ("running" matches "run")
* Good, because phrase search supported (to_tsquery with "running & fast")
* Good, because pagination with cursors (created_at + message_id)
* Good, because client_id partitioning prevents noisy neighbors
* Bad, because less feature-rich than Elasticsearch (no typo tolerance, advanced ranking)
* Bad, because GIN indexes consume storage (additional ~20% per index)
* Bad, because index updates add write latency (~5ms per message)
* Bad, because cross-language stemming limited (English default)

### Confirmation

Confirmed when PostgreSQL tsvector with GIN indexes returns ranked full-text search results within 1 second for session-scoped queries and within 3 seconds for cross-session queries.

## Pros and Cons of the Options

### Option 1: PostgreSQL tsvector with GIN indexes

Native full-text search using tsvector columns, GIN indexing, and ts_rank_cd for relevance ranking.

* Good, because no additional infrastructure — search runs within the existing PostgreSQL database
* Good, because GIN indexes provide fast full-text lookups meeting performance targets (< 1s session, < 3s cross-session)
* Good, because ts_rank_cd provides relevance ranking with document length normalization
* Good, because built-in stemming and case-insensitive matching ("running" matches "run")
* Bad, because less feature-rich than dedicated search engines (no typo tolerance, fuzzy matching)
* Bad, because GIN indexes consume additional storage (~20% overhead per indexed column)
* Bad, because index updates add write latency (~5ms per message insert)
* Bad, because cross-language stemming is limited (single language dictionary per column)

### Option 2: Elasticsearch

Dedicated search engine deployed alongside PostgreSQL for advanced full-text search.

* Good, because rich search features out of the box (fuzzy matching, typo tolerance, synonyms, highlighting)
* Good, because horizontally scalable — search throughput scales independently of the database
* Good, because supports advanced relevance tuning (BM25, boosting, custom scoring)
* Good, because multi-language analysis with per-field language detection
* Bad, because requires separate infrastructure (cluster deployment, monitoring, upgrades)
* Bad, because data synchronization between PostgreSQL and Elasticsearch adds complexity and eventual consistency
* Bad, because significant operational overhead (JVM tuning, shard management, index lifecycle)
* Bad, because increased latency for writes due to dual-write or change-data-capture pipeline

### Option 3: Simple LIKE queries

Basic pattern matching using PostgreSQL ILIKE operator on message content columns.

* Good, because trivial to implement — no indexes, tsvector columns, or external systems required
* Good, because exact substring matching is predictable and easy to understand
* Good, because zero additional storage overhead (no extra indexes or columns)
* Bad, because ILIKE triggers sequential scans on large tables — does not meet performance requirements
* Bad, because no relevance ranking — results returned in insertion order, not by relevance
* Bad, because no stemming or linguistic processing ("running" does not match "run")
* Bad, because phrase search and boolean operators must be hand-implemented with complex SQL

## Related Design Elements

**Actors**:
* `cpt-cf-chat-engine-actor-client` - Submits search queries, receives ranked results
* `cpt-cf-chat-engine-component-message-search` - Executes full-text queries

**Requirements**:
* `cpt-cf-chat-engine-fr-search-session` - Session-scoped full-text search
* `cpt-cf-chat-engine-fr-search-sessions` - Cross-session full-text search
* `cpt-cf-chat-engine-nfr-search` - Performance requirements (1s session, 3s cross-session)

**Design Elements**:
* `cpt-cf-chat-engine-dbtable-messages` - Full-text index on content field
* `cpt-cf-chat-engine-design-context-search` - Implementation details (tsvector, GIN, ts_rank_cd)
* HTTP POST /sessions/{id}/search and POST /search endpoints

**Related ADRs**:
* ADR-0009 (Stateless Horizontal Scaling with Database State) - PostgreSQL full-text search features
* ADR-0017 (Session Metadata JSONB for Extensibility) - Search includes metadata fields (title, tags)
