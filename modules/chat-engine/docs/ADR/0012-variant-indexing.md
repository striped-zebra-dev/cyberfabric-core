<!-- Created: 2026-02-04 by Constructor Tech -->
<!-- Updated: 2026-04-07 by Constructor Tech -->

# ADR-0012: Variant Index for Sequential Navigation


<!-- toc -->

- [Context and Problem Statement](#context-and-problem-statement)
- [Decision Drivers](#decision-drivers)
- [Considered Options](#considered-options)
- [Decision Outcome](#decision-outcome)
  - [Consequences](#consequences)
  - [Confirmation](#confirmation)
- [Pros and Cons of the Options](#pros-and-cons-of-the-options)
  - [Option 1: 0-based variant_index](#option-1-0-based-variant_index)
  - [Option 2: UUID-based ordering](#option-2-uuid-based-ordering)
  - [Option 3: Timestamp-based ordering](#option-3-timestamp-based-ordering)
- [Related Design Elements](#related-design-elements)

<!-- /toc -->

**Date**: 2026-02-04

**Status**: accepted

**Review**: Revisit if variant indexing query patterns change

**ID**: `cpt-cf-chat-engine-adr-variant-indexing`

> **Note**: This ADR extends ADR-0011 (Message Variants) with indexing-specific decisions. See ADR-0011 for the variant storage model.

## Context and Problem Statement

Users need to navigate between message variants (alternative responses to same question). How should variants be ordered and identified to enable intuitive sequential navigation (previous/next variant) and clear position indicators?

## Decision Drivers

* Intuitive navigation (variant 1 of 3, previous/next buttons)
* Deterministic ordering (not random or timestamp-based)
* Efficient queries (find next/previous variant)
* Position calculation simple (current index + total count)
* Support for unlimited variants (no fixed array size)
* Stable ordering (doesn't change as variants added)
* Database indexing efficient
* UI affordances clear (2 of 5)

## Considered Options

* **Option 1: 0-based variant_index** - Integer field starting at 0, incremented per variant
* **Option 2: UUID-based ordering** - UUIDs sorted lexicographically
* **Option 3: Timestamp-based ordering** - created_at determines order

## Decision Outcome

Chosen option: "0-based variant_index", because it provides intuitive sequential ordering (0, 1, 2, ...), enables simple position calculation (index + 1 of total), supports efficient next/previous queries (WHERE variant_index = current ± 1), maintains stable ordering independent of creation time, and maps naturally to UI navigation (variant 2 of 5).

### Consequences

* Good, because intuitive numbering (variant 1, 2, 3 for users)
* Good, because simple position calculation (SELECT COUNT(*) for total)
* Good, because efficient next/previous queries (variant_index ± 1)
* Good, because stable ordering (independent of creation time)
* Good, because database indexing straightforward (INTEGER index)
* Good, because UI naturally shows "2 of 5" from index and count
* Bad, because variant_index calculation requires MAX query (find highest index)
* Bad, because gaps possible if variants deleted (index 0, 1, 3 after delete)
* Bad, because no semantic meaning (index 0 not necessarily "best")
* Bad, because reordering requires UPDATE (change all indices)

### Confirmation

Confirmed when variant navigation queries use variant_index arithmetic (current ± 1) and position display returns correct "N of M" values from index and COUNT(*).

## Pros and Cons of the Options

### Option 1: 0-based variant_index

* Good, because intuitive sequential numbering maps directly to UI display ("2 of 5")
* Good, because next/previous navigation is a simple arithmetic query (variant_index ± 1)
* Good, because stable ordering independent of creation time or instance clocks
* Good, because efficient database indexing on INTEGER column
* Bad, because new variant requires MAX(variant_index) + 1 query to determine next index
* Bad, because deletion leaves gaps in the sequence (0, 1, 3 after deleting index 2)
* Bad, because reordering variants requires updating multiple rows

### Option 2: UUID-based ordering

* Good, because globally unique identifiers with no collision risk across instances
* Good, because no coordination needed to generate identifiers (no MAX query)
* Good, because UUIDs can encode creation time if using UUIDv7, providing natural ordering
* Bad, because lexicographic UUID sorting does not reflect meaningful variant order
* Bad, because position display ("2 of 5") requires sorting all UUIDs and counting position
* Bad, because next/previous navigation requires fetching and sorting all sibling UUIDs
* Bad, because UUIDs are opaque to users and harder to debug than sequential integers

### Option 3: Timestamp-based ordering

* Good, because timestamps are assigned automatically without explicit index management
* Good, because chronological order naturally reflects variant creation sequence
* Good, because no coordination or MAX query needed (each insert uses current time)
* Bad, because clock skew across stateless instances can produce inconsistent ordering
* Bad, because sub-millisecond concurrent regenerations may produce identical timestamps
* Bad, because position calculation requires ORDER BY + row numbering rather than direct index lookup
* Bad, because ordering is fragile and may change if timestamps are corrected or adjusted

## Related Design Elements

**Actors**:
* `cpt-cf-chat-engine-actor-client` - Requests next/previous variant, displays position
* `cpt-cf-chat-engine-component-message-processing` - Calculates variant_index for new variants

**Requirements**:
* `cpt-cf-chat-engine-fr-navigate-variants` - Query API returns position metadata
* `cpt-cf-chat-engine-fr-recreate-response` - New variant gets incremented index

**Design Elements**:
* `cpt-cf-chat-engine-design-entity-message` - variant_index field (INTEGER, 0-based)
* `cpt-cf-chat-engine-dbtable-messages` - Unique constraint (session_id, parent_message_id, variant_index)

**Related ADRs**:
* ADR-0011 (Message Variants with Index and Active Flag) - variant_index is core field for variants
* ADR-0013 (Recreation Creates Variants, Branching Creates Children) - Recreation increments variant_index
