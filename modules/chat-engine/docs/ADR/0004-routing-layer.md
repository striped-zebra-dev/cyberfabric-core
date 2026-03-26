Created:  2026-02-04 by Constructor Tech
Updated:  2026-03-06 by Constructor Tech
# ADR-0004: Zero Business Logic in Routing Layer


<!-- toc -->

- [Context and Problem Statement](#context-and-problem-statement)
- [Decision Drivers](#decision-drivers)
- [Considered Options](#considered-options)
- [Decision Outcome](#decision-outcome)
  - [Consequences](#consequences)
  - [Confirmation](#confirmation)
- [Pros and Cons of the Options](#pros-and-cons-of-the-options)
  - [Option 1: Zero business logic (pure routing)](#option-1-zero-business-logic-pure-routing)
  - [Option 2: Enrichment layer](#option-2-enrichment-layer)
  - [Option 3: Smart routing](#option-3-smart-routing)
- [Related Design Elements](#related-design-elements)

<!-- /toc -->

**Date**: 2026-02-04

**Status**: accepted

**Review**: Revisit if routing layer needs content transformation or enrichment.

**ID**: `cpt-cf-chat-engine-adr-routing-layer`

## Context and Problem Statement

Chat Engine sits between clients and webhook backends as a proxy service. Should Chat Engine inspect, analyze, or transform message content, or should it remain a pure routing infrastructure focused on session management and message persistence?

## Decision Drivers

* Enable rapid backend experimentation without infrastructure changes
* Keep Chat Engine focused on infrastructure concerns (routing, persistence, scaling)
* Avoid coupling Chat Engine to specific backend implementations or processing logic
* Support diverse backend types (LLMs, rule-based, human-in-the-loop)
* Simplify Chat Engine codebase and reduce maintenance burden
* Enable backends to evolve independently
* Minimize latency overhead from proxying

## Considered Options

* **Option 1: Zero business logic (pure routing)** - Chat Engine only routes, persists, and manages message trees
* **Option 2: Enrichment layer** - Chat Engine adds metadata, moderation, logging before routing
* **Option 3: Smart routing** - Chat Engine analyzes content to select appropriate backend or transform messages

## Decision Outcome

Chosen option: "Zero business logic (pure routing)", because it decouples infrastructure from processing logic, enables backends to change without Chat Engine updates, keeps routing latency minimal, allows diverse backend implementations, and simplifies Chat Engine codebase focusing on reliability and scaling.

### Consequences

* Good, because backends can change processing logic without Chat Engine deployment
* Good, because new backend types require zero Chat Engine code changes
* Good, because routing layer remains simple, testable, and maintainable
* Good, because latency overhead is minimal (no content inspection/transformation)
* Good, because Chat Engine can focus on reliability, scaling, and message tree management
* Good, because content moderation, language detection, etc. can be backend-specific
* Bad, because common processing (moderation, logging enrichment) must be implemented per backend
* Bad, because Chat Engine cannot provide value-added services (e.g., automatic translation)
* Bad, because debugging requires looking at backend logs (Chat Engine doesn't inspect content)

### Confirmation

Confirmed by reviewing routing layer code for zero business logic compliance.

## Pros and Cons of the Options

### Option 1: Zero business logic (pure routing)

Chat Engine only routes messages, persists them, and manages message trees. No inspection or transformation of content.

* Good, because backends can change processing logic without Chat Engine deployment
* Good, because new backend types require zero Chat Engine code changes
* Good, because routing layer remains simple, testable, and maintainable
* Good, because latency overhead is minimal (no content inspection or transformation)
* Bad, because common cross-cutting concerns (moderation, logging enrichment) must be reimplemented per backend
* Bad, because Chat Engine cannot provide value-added services such as automatic translation or content filtering
* Bad, because debugging requires correlating logs across Chat Engine and backend services

### Option 2: Enrichment layer

Chat Engine adds metadata, performs content moderation, and enriches logging before routing messages to backends.

* Good, because cross-cutting concerns (moderation, rate limiting, PII detection) are handled once centrally
* Good, because backends receive pre-processed, enriched payloads reducing per-backend boilerplate
* Good, because centralized logging and audit trail simplifies debugging and compliance
* Bad, because Chat Engine becomes coupled to processing logic, requiring redeployment when enrichment rules change
* Bad, because additional processing adds latency to every message on the critical path
* Bad, because enrichment logic may conflict with backend-specific requirements (e.g., different moderation thresholds)
* Bad, because increases Chat Engine codebase complexity and maintenance burden

### Option 3: Smart routing

Chat Engine analyzes message content to select the appropriate backend or transform messages before forwarding.

* Good, because clients can send messages without specifying a target backend, simplifying client logic
* Good, because content-aware routing enables automatic backend selection (e.g., code questions to code backend, general questions to LLM)
* Good, because message transformation can normalize different client formats into a canonical backend format
* Bad, because routing logic must understand backend capabilities, creating tight coupling between Chat Engine and backends
* Bad, because content analysis adds significant latency and computational overhead to every request
* Bad, because routing rules require frequent updates as backends evolve, making Chat Engine a deployment bottleneck
* Bad, because misrouting errors are hard to debug and can silently degrade user experience

## Related Design Elements

**Actors**:
* `cpt-cf-chat-engine-actor-backend-plugin` - Responsible for ALL message processing logic

**Requirements**:
* Requirements `cpt-cf-chat-engine-fr-send-message`, `cpt-cf-chat-engine-fr-stop-streaming` assume transparent routing
* `cpt-cf-chat-engine-nfr-response-time` - Minimal overhead from routing (< 100ms)

**Design Elements**:
* `cpt-cf-chat-engine-component-webhook-integration` - Chat Engine's HTTP proxy functionality with timeout/circuit breaker
* `cpt-cf-chat-engine-principle-zero-business-logic` - Design principle codifying this decision
* `cpt-cf-chat-engine-design-context-circuit-breaker` - Backend responsibility scope

**Related ADRs**:
* ADR-0002 (Capability Model) - Backends define capabilities, not Chat Engine
* ADR-0007 (Webhook Event Schema with Typed Events) - Events carry full context without interpretation
