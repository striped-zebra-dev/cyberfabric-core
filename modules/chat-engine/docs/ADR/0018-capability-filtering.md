<!-- Created: 2026-02-04 by Constructor Tech -->
<!-- Updated: 2026-04-07 by Constructor Tech -->

# ADR-0018: Per-Request Capability Filtering


<!-- toc -->

- [Context and Problem Statement](#context-and-problem-statement)
- [Decision Drivers](#decision-drivers)
- [Considered Options](#considered-options)
- [Decision Outcome](#decision-outcome)
  - [Consequences](#consequences)
  - [Confirmation](#confirmation)
- [Pros and Cons of the Options](#pros-and-cons-of-the-options)
  - [Option 1: enabled_capabilities array per message](#option-1-enabled_capabilities-array-per-message)
  - [Option 2: Session-level toggle](#option-2-session-level-toggle)
  - [Option 3: Implicit capabilities](#option-3-implicit-capabilities)
- [Related Design Elements](#related-design-elements)

<!-- /toc -->

**Date**: 2026-02-04

**Status**: accepted

**Review**: Revisit if capability filtering granularity needs to be per-message

**ID**: `cpt-cf-chat-engine-adr-capability-filtering`

## Context and Problem Statement

Sessions have `enabled_capabilities` — typed `Capability` definitions (bool/enum/str/int) returned by the backend plugin on session creation (e.g., web_search: bool, response_style: enum, max_length: int). Users may want to selectively configure capabilities per message rather than using all defaults. How should clients pass capability settings for specific messages?

## Decision Drivers

* User control over expensive features (disable web_search to save costs)
* Backend receives explicit capability intent per message
* Capabilities available at session level, enabled at message level
* Client validates capabilities against available set
* Backend can optimize based on enabled capabilities
* Support for capability subsets (enable only web_search, not code_execution)
* Future-proof for new capability types
* Clear error messaging for unsupported capabilities

## Considered Options

* **Option 1: enabled_capabilities array per message** - Client sends array of `CapabilityValue` objects (`{id, value}`) with each message
* **Option 2: Session-level toggle** - Update session to enable/disable capabilities globally
* **Option 3: Implicit capabilities** - Backend infers from message content

## Decision Outcome

Chosen option: "enabled_capabilities array per message", because it provides per-message granularity for capability control, enables user cost optimization, gives backends explicit capability values (typed: bool/enum/str/int), supports capability subsets, maintains session `enabled_capabilities` as the authoritative capability registry, and supports all value types without protocol changes.

### Consequences

* Good, because users disable expensive capabilities per message (cost optimization)
* Good, because backend receives explicit intent (no capability inference needed)
* Good, because supports capability subsets (enable some, disable others)
* Good, because future capabilities work without protocol changes
* Good, because session `enabled_capabilities` remains authoritative (typed Capability definitions)
* Good, because client can validate before sending (check id + value type against session's `enabled_capabilities`)
* Bad, because client must send `CapabilityValue[]` with every message
* Bad, because value type validation (id exists in session's `enabled_capabilities`, value matches declared type) adds overhead
* Bad, because invalid capability IDs or type mismatches rejected (error handling complexity)
* Bad, because capability defaults not enforced (client must specify values explicitly)

### Confirmation

Confirmed when each message request accepts a CapabilityValue[] array validated against the session's enabled_capabilities, and the backend receives the per-message capability values.

## Pros and Cons of the Options

### Option 1: enabled_capabilities array per message

Client sends an array of `CapabilityValue` objects (`{id, value}`) with each message request.

* Good, because per-message granularity lets users optimize cost on a per-request basis
* Good, because backend receives explicit, typed capability values — no inference needed
* Good, because supports capability subsets (enable web_search but disable code_execution)
* Good, because new capability types work without protocol changes
* Bad, because client must construct and send `CapabilityValue[]` with every message
* Bad, because validation overhead — each capability id and value type must be checked against session definitions
* Bad, because clients that want defaults must still explicitly send all default values

### Option 2: Session-level toggle

Update session settings to enable/disable capabilities globally for all subsequent messages.

* Good, because simpler client implementation — set capabilities once, apply to all messages
* Good, because reduces per-message payload size (no capability array in each request)
* Good, because provides a consistent capability configuration visible to all session participants
* Bad, because no per-message granularity — users cannot disable an expensive feature for a single query
* Bad, because concurrent clients may conflict when toggling session-level capability settings
* Bad, because changing capabilities requires an extra API call to update the session before sending a message
* Bad, because capability state is implicit per message, making audit and replay harder

### Option 3: Implicit capabilities

Backend infers which capabilities to use based on message content analysis.

* Good, because zero client effort — no capability configuration needed in the request
* Good, because simplest possible client protocol (just send message text)
* Good, because backend can apply domain-specific heuristics to detect capability needs
* Bad, because inference is unreliable — backend may enable expensive capabilities unnecessarily
* Bad, because users lose explicit control over cost and behavior
* Bad, because inference logic must be maintained and updated as new capabilities are added
* Bad, because difficult to debug or predict which capabilities will be activated for a given message

## Related Design Elements

**Actors**:
* `cpt-cf-chat-engine-actor-client` - Sends `CapabilityValue[]` per message
* `cpt-cf-chat-engine-actor-backend-plugin` - Receives `CapabilityValue[]`, optimizes processing accordingly

**Design Elements**:
* Chat Engine validates `CapabilityValue.id` against session's `enabled_capabilities` and value type against `Capability.type`

**Requirements**:
* `cpt-cf-chat-engine-fr-send-message` - Message includes `enabled_capabilities: CapabilityValue[]`
* `cpt-cf-chat-engine-fr-create-session` - Session stores `enabled_capabilities: Capability[]`

**Design Elements**:
* `cpt-cf-chat-engine-design-entity-session` - `enabled_capabilities: Capability[]` (authoritative type registry)
* HTTP POST /messages/send — `enabled_capabilities: CapabilityValue[]`
* Webhook `message.new` event — `enabled_capabilities: CapabilityValue[]`

**Related ADRs**:
* ADR-0002 (Capability Model) - Backend defines `enabled_capabilities` (Capability definitions)
* ADR-0015 (Session Type Switching with Capability Updates) - Capabilities update when switching backends
