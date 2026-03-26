<!-- Created: 2026-02-04 by Constructor Tech -->
<!-- Updated: 2026-04-07 by Constructor Tech -->

# ADR-0011: Message Variants with Index and Active Flag


<!-- toc -->

- [Context and Problem Statement](#context-and-problem-statement)
- [Decision Drivers](#decision-drivers)
- [Considered Options](#considered-options)
- [Decision Outcome](#decision-outcome)
  - [Consequences](#consequences)
  - [Confirmation](#confirmation)
- [Pros and Cons of the Options](#pros-and-cons-of-the-options)
  - [Option 1: variant_index + is_active flags](#option-1-variant_index--is_active-flags)
  - [Option 2: Separate variants table](#option-2-separate-variants-table)
  - [Option 3: Version field with timestamps](#option-3-version-field-with-timestamps)
- [Related Design Elements](#related-design-elements)

<!-- /toc -->

**Date**: 2026-02-04

**Status**: accepted

**Review**: Revisit if variant storage grows beyond expected bounds

**ID**: `cpt-cf-chat-engine-adr-message-variants`

## Context and Problem Statement

Chat Engine supports message regeneration, creating multiple assistant responses for the same user message. How should these variant messages be stored, identified, and navigated to enable users to explore alternatives while maintaining a clear active path?

## Decision Drivers

* Natural representation of variants (siblings in message tree)
* Deterministic ordering (variants numbered 0, 1, 2, ...)
* Active path tracking (which variant is currently selected)
* Unique identification (prevent duplicate variants)
* Navigation metadata (variant position: "2 of 3")
* Database constraints enforce variant integrity
* Support for unlimited variants per parent
* Efficient variant querying

## Considered Options

* **Option 1: variant_index + is_active flags** - Each message has 0-based index and active boolean
* **Option 2: Separate variants table** - Message variants stored in separate table linking to original
* **Option 3: Version field with timestamps** - Timestamp-based versioning for variants

## Decision Outcome

Chosen option: "variant_index + is_active flags", because it provides deterministic ordering via 0-based index, enables unique constraint (session_id, parent_message_id, variant_index), supports active path tracking via is_active flag, keeps variants in message table (no joins needed), and enables efficient sibling queries.

### Consequences

* Good, because variants naturally represented as siblings (same parent_message_id)
* Good, because deterministic ordering (variant_index 0, 1, 2, ...)
* Good, because unique constraint prevents duplicate variants
* Good, because is_active flag marks current variant in UI
* Good, because variant position calculation simple (index + total count)
* Good, because no separate table or joins needed for variant queries
* Bad, because variant_index must be calculated (MAX(variant_index) + 1)
* Bad, because changing active variant requires UPDATE (set old to false, new to true)
* Bad, because deleting variants leaves gaps in variant_index sequence
* Bad, because is_active is session-level concept but stored per message

### Confirmation

Confirmed when the unique constraint on (session_id, parent_message_id, variant_index) is enforced and is_active flags correctly track the selected variant per parent.

## Pros and Cons of the Options

### Option 1: variant_index + is_active flags

* Good, because variants live in the same messages table (no joins needed for queries)
* Good, because deterministic ordering via 0-based integer index (0, 1, 2, ...)
* Good, because unique constraint (session_id, parent_message_id, variant_index) prevents duplicates
* Good, because is_active flag enables straightforward active-path tracking per session
* Bad, because variant_index must be calculated via MAX query on each new variant creation
* Bad, because changing the active variant requires two UPDATEs (deactivate old, activate new)
* Bad, because deleting variants leaves gaps in the index sequence (0, 1, 3)

### Option 2: Separate variants table

* Good, because clean separation of concerns (messages and variants are distinct entities)
* Good, because variant metadata (ordering, active flag) is isolated in its own schema
* Good, because deleting all variants of a message is a simple table-scoped operation
* Bad, because querying a message with its variants requires a JOIN, adding latency
* Bad, because two tables must be kept in sync (insert message + insert variant row)
* Bad, because increases schema complexity and migration surface area
* Bad, because variant count queries require cross-table aggregation

### Option 3: Version field with timestamps

* Good, because no explicit index calculation needed (timestamp assigned automatically)
* Good, because natural chronological ordering reflects creation order
* Good, because simple schema (single timestamp field, no separate active flag needed if latest wins)
* Bad, because timestamp precision collisions are possible under concurrent regeneration
* Bad, because ordering is non-deterministic if clock skew occurs across instances
* Bad, because position display ("2 of 5") requires sorting and counting rather than simple index math
* Bad, because "latest wins" semantics prevent users from selecting a non-latest variant as active

## Related Design Elements

**Actors**:
* `cpt-cf-chat-engine-actor-client` - Navigates variants, requests position metadata
* `cpt-cf-chat-engine-component-message-processing` - Assigns variant_index, manages is_active

**Requirements**:
* `cpt-cf-chat-engine-fr-recreate-response` - Creates new variant with incremented variant_index
* `cpt-cf-chat-engine-fr-navigate-variants` - Query siblings, return position metadata
* `cpt-cf-chat-engine-nfr-data-integrity` - Unique constraint on (session_id, parent_message_id, variant_index)

**Design Elements**:
* `cpt-cf-chat-engine-design-entity-message` - variant_index and is_active fields
* `cpt-cf-chat-engine-dbtable-messages` - Unique constraint enforcing variant integrity

**Related ADRs**:
* ADR-0001 (Message Tree Structure) - Variants are siblings in tree
* ADR-0012 (Variant Index for Sequential Navigation) - UI navigation using variant_index
* ADR-0013 (Recreation Creates Variants, Branching Creates Children) - Recreation creates variant (same parent)
