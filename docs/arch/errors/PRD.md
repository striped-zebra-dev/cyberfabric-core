# PRD — Universal Error Architecture

## 1. Overview

### 1.1 Purpose

The `modkit-errors` library provides the universal error vocabulary for the CyberFabric platform. Every error produced by any module is expressed as one of 16 canonical categories,
each carrying a fixed-structure context payload. Errors are part of the public API surface — every error response is a contract with API consumers. This PRD defines what the error
system must guarantee: contract stability, canonical categories, machine-readable details, and protection against accidental breaking changes.

### 1.2 Background / Problem Statement

Without a universal error architecture, each module would define its own ad-hoc error types with module-specific variants and manual mappings to wire formats. This produces three
problems:

1. **Inconsistent error shapes**: API consumers cannot rely on a stable, predictable error format across modules. The same logical error (e.g., "resource not found") may carry
   different fields, different status codes, or different detail messages depending on which module produced it.

2. **No breaking change detection**: There is no mechanism to detect when a code change alters the error contract — a developer or LLM agent can change an error category, rename a
   context field, or alter a GTS identifier without triggering any warning. Consumers who depend on specific error shapes break silently.

3. **No contract governance**: Without explicit product requirements around error stability and versioning, the error API surface drifts over time. Design-level documentation alone
   does not prevent accidental regressions.

### 1.3 Goals (Business Outcomes)

- Zero unreviewed breaking changes to the error API surface per release cycle
- All modules return errors in the same canonical shape from day one
- API consumers can implement stable error handling (retry, display, routing) against documented error contracts without fear of silent breakage

### 1.4 Glossary

| Term               | Definition                                                                                                           |
|--------------------|----------------------------------------------------------------------------------------------------------------------|
| Canonical Category | One of 16 transport-agnostic error classifications (e.g., `not_found`, `invalid_argument`)                           |
| Context Type       | Fixed-structure payload per category (e.g., `ResourceInfo`, `Validation`, `ErrorInfo`)                               |
| GTS Identifier     | Globally unique compound type URI in the GTS system (e.g., `gts.cf.core.errors.err.v1~cf.core.errors.not_found.v1~`) |
| Contract Part      | Fixed category-level structure: `type`, `title`, `status`, context **schema**. Changes are breaking.                 |
| Variable Part      | Per-occurrence values: `detail`, `instance`, `trace_id`, context **values**. Not contractual.                        |
| Error Contract     | Combination of category, context type schema, and GTS identifier (all contract parts)                                |
| Breaking Change    | Any contract part modification that would break existing consumer error-handling code                                |
| Snapshot Test      | Automated test capturing the exact error response shape; fails if the shape changes                                  |

## 2. Actors

### 2.1 Human Actors

#### Module Developer

**ID**: `cpt-cf-actor-module-developer`

- **Role**: Writes domain logic in CyberFabric modules. Constructs canonical errors when business rules fail, validation errors occur, or resources are not found.
- **Needs**: Low-boilerplate error construction with no transport details. Confidence that error construction follows the canonical vocabulary without accidentally introducing
  breaking changes.

#### API Consumer

**ID**: `cpt-cf-actor-api-consumer`

- **Role**: External or internal client that receives error responses from CyberFabric APIs. Implements error handling logic (retry on `service_unavailable`, display field
  violations
  on `invalid_argument`, redirect on `unauthenticated`).
- **Needs**: Stable, predictable error response shapes. Machine-readable context fields. Confidence that error contracts do not change without versioned notice.

### 2.2 System Actors

#### CI Pipeline

**ID**: `cpt-cf-actor-ci-pipeline`

- **Role**: Automated build and validation system that runs on every commit and pull request. Enforces error contract stability through compile-time checks, snapshot tests, and
  semver verification.

#### LLM Agent

**ID**: `cpt-cf-actor-llm-agent`

- **Role**: AI coding assistant that generates or modifies code constructing errors. Must be constrained by the same compile-time and CI checks as human developers to prevent
  accidental error contract changes.

## 3. Operational Concept & Environment

> This is a cross-cutting platform library (`libs/modkit-errors`), not a standalone module. No module-specific environment constraints beyond project defaults.

## 4. Scope

### 4.1 In Scope

- 16 canonical error categories as the single error vocabulary
- Fixed-structure context types per category
- Error contract stability requirements and breaking change policy
- Protection mechanisms against accidental changes (compile-time, snapshot tests, CI gates)
- GTS identifier assignment and versioning for error categories
- Public API surface definition for `CanonicalError` and context types
- Library error absorption (`From` impls for common library errors)

### 4.2 Out of Scope

- Transport-specific mapping implementation (REST/Problem, gRPC/Status) — covered by DESIGN
- Error UI rendering or client-side error display logic
- Error analytics, dashboards, or aggregation

## 5. Functional Requirements

> **Testing strategy**: All requirements verified via automated tests (unit, integration, snapshot) unless otherwise specified.

### 5.1 Canonical Error Vocabulary

#### Single Error Vocabulary

- [ ] `p1` - **ID**: `cpt-cf-fr-single-error-vocabulary`

The system **MUST** provide exactly 16 canonical error categories as the only way to express errors in the API layer. No module-specific error types are permitted to reach API
consumers.

- **Rationale**: API consumers depend on a finite, documented set of error shapes. Allowing ad-hoc error types breaks consumer error-handling code.
- **Actors**: `cpt-cf-actor-module-developer`, `cpt-cf-actor-api-consumer`

#### Machine-Readable Context

- [ ] `p1` - **ID**: `cpt-cf-fr-machine-readable-context`

Every canonical error **MUST** carry a fixed-structure context type with a schema defined per category. Context type fields are typed and documented. No ad-hoc metadata keys are
permitted outside the defined schema (except `ErrorInfo.metadata` which is an explicit escape hatch).

- **Rationale**: API consumers parse error details programmatically. Unpredictable field names or missing fields break automated error handling.
- **Actors**: `cpt-cf-actor-api-consumer`

#### Transport-Agnostic Construction

- [ ] `p1` - **ID**: `cpt-cf-fr-transport-agnostic-construction`

Module code **MUST** construct errors using canonical categories and context types without referencing HTTP status codes, gRPC codes, or any transport-specific detail.

- **Rationale**: Error construction is a domain concern. Transport mapping is an infrastructure concern. Mixing them couples domain logic to a specific transport.
- **Actors**: `cpt-cf-actor-module-developer`

#### Library Error Absorption

- [ ] `p1` - **ID**: `cpt-cf-fr-library-error-absorption`

The system **MUST** provide `From` implementations for common library error types (`anyhow::Error`, `sea_orm::DbErr`, `sqlx::Error`, `serde_json::Error`, `std::io::Error`) that map
to appropriate canonical categories. The `?` operator **MUST** work without manual `map_err` in common cases.

- **Rationale**: Repetitive `map_err` calls are boilerplate that developers (and LLM agents) get wrong. Blanket impls ensure consistent mapping.
- **Actors**: `cpt-cf-actor-module-developer`, `cpt-cf-actor-llm-agent`

#### Trace Correlation

- [ ] `p1` - **ID**: `cpt-cf-fr-trace-correlation`

Every error response **MUST** include a `trace_id` field containing the W3C trace ID from the request context. The trace ID **MUST** be present in both production and debug modes,
enabling consumers to correlate error responses with support requests and server-side logs.

- **Rationale**: Without a trace ID, consumers cannot reference specific error occurrences when contacting support. Server-side logs are only useful if they can be correlated with
  the client-visible response.
- **Actors**: `cpt-cf-actor-api-consumer`

### 5.2 Error Contract Stability

#### Error Contract as API Surface

- [ ] `p1` - **ID**: `cpt-cf-fr-error-contract-api-surface`

Every error response has two distinct parts:

- **Contract part** (stable, category-level): `type` (GTS identifier), `title`, `status`, and the context type **schema** (field names and types). These are fixed per category and
  identical for every occurrence of that category, regardless of which module produced it.
- **Variable part** (per-occurrence): `detail`, `instance`, `trace_id`, and the context field **values**. These are filled in by module code, framework, or middleware at the point
  of error and change with every request.

The contract part **MUST** be treated as public API surface. Any change to a contract part **MUST** follow the same breaking change policy as endpoint signature changes. API
consumers **MAY** depend on contract parts (match on `type`, branch on `status`, parse context fields by name) but **MUST NOT** depend on variable parts (parse `detail` text, rely
on specific context values).

- **Rationale**: API consumers write code that matches on error categories, reads context fields by name, and uses GTS identifiers for routing. The contract/variable separation
  defines the exact boundary of what the error architecture protects and what module code is free to change.
- **Actors**: `cpt-cf-actor-api-consumer`, `cpt-cf-actor-module-developer`

#### Breaking Change Classification

- [ ] `p1` - **ID**: `cpt-cf-fr-breaking-change-classification`

The following changes to the error system **MUST** be classified as breaking changes:

- Removing or renaming a canonical category
- Changing the context type associated with a category
- Removing or renaming a field in a context type schema
- Changing the type of a field in a context type schema
- Changing the GTS identifier of a category
- Changing the HTTP status code mapped to a category

The following changes are non-breaking:

- Adding a new optional field to a context type
- Adding a new canonical category (additive)
- **Rationale**: Clear classification prevents ambiguity about what constitutes a breaking change and enables automated enforcement.
- **Actors**: `cpt-cf-actor-module-developer`, `cpt-cf-actor-ci-pipeline`

#### Accidental Change Protection

- [ ] `p1` - **ID**: `cpt-cf-fr-accidental-change-protection`

The system **MUST** provide automated mechanisms to detect and prevent accidental changes to the error contract:

1. **Compile-time enforcement**: Adding or removing a canonical category **MUST** cause a compile error in all code that matches on the error enum (exhaustive match).
2. **Macro-based GTS construction**: Context types carrying GTS identifiers (e.g., `ResourceInfo`) **MUST** only be constructable via attribute macros that declare the GTS
   identifier at the type level. Raw constructors **MUST NOT** be public. The macro **MUST** validate the GTS format at compile time.
3. **Snapshot tests**: The CI pipeline **MUST** include snapshot tests that capture the exact JSON shape of error responses for each canonical category. Any change to the snapshot
   **MUST** require explicit developer acknowledgment (snapshot update).
4. **Semver-aware CI**: The CI pipeline **MUST** run semver compatibility checks on `cf-modkit-errors` to detect breaking changes to public types.

- **Rationale**: Human developers and LLM agents can both accidentally change error shapes. Compile-time checks catch structural changes. Macro-based construction prevents GTS
  identifier typos and drift. Snapshot tests catch serialization changes. Semver checks catch public API changes.
- **Actors**: `cpt-cf-actor-ci-pipeline`, `cpt-cf-actor-llm-agent`, `cpt-cf-actor-module-developer`

#### No Internal Details in Production

- [ ] `p1` - **ID**: `cpt-cf-fr-no-internal-details`

Error responses in production **MUST NOT** include stack traces, internal exception messages, database query text, or file paths. The `internal` and `unknown` categories **MUST**
return an opaque message with a trace ID for correlation. `DebugInfo.stack_entries` **MUST** be populated only in debug mode.

- **Rationale**: Internal details are a security risk (information disclosure) and a stability risk (consumers parsing internal messages as contract).
- **Actors**: `cpt-cf-actor-api-consumer`

## 6. Non-Functional Requirements

### 6.1 Module-Specific NFRs

#### Compile-Time Category Safety

- [ ] `p1` - **ID**: `cpt-cf-nfr-compile-time-category-safety`

Adding or removing a canonical category **MUST** produce a compile error in all uncovered match sites across the entire codebase. Zero runtime category validation is acceptable —
all category correctness **MUST** be enforced at compile time.

- **Threshold**: 100% of match sites produce compile errors on category change; zero runtime panics from unhandled categories
- **Rationale**: Runtime errors from unhandled categories are production incidents. Compile-time enforcement eliminates this class of bugs entirely.
- **Architecture Allocation**: See DESIGN.md § Principles — exhaustive match on `CanonicalError` enum

#### Error Construction Overhead

- [ ] `p2` - **ID**: `cpt-cf-nfr-error-construction-overhead`

Constructing a canonical error **MUST** not require heap allocation for category selection. Context types that use only fixed fields (all types except `ErrorInfo`) **MUST** be
stack-allocated.

- **Threshold**: Error construction < 100ns at p99 for stack-only context types
- **Rationale**: Error paths are latency-sensitive. Heap allocation on every error would degrade p99 latency.
- **Architecture Allocation**: See DESIGN.md § NFR Allocation

## 7. Public Library Interfaces

### 7.1 Public API Surface

#### CanonicalError Enum

- [ ] `p1` - **ID**: `cpt-cf-interface-canonical-error`

- **Type**: Rust enum (`libs/modkit-errors`)
- **Stability**: stable
- **Description**: The 16-variant enum representing all canonical error categories. Each variant carries its category-specific context type and a message.
- **Breaking Change Policy**: Removing/renaming a variant or changing its context type requires a major version bump of `cf-modkit-errors`. Adding a new variant is a minor version
  change.

#### Context Type Structs

- [ ] `p1` - **ID**: `cpt-cf-interface-context-types`

- **Type**: Rust structs and enums (`libs/modkit-errors`)
- **Stability**: stable
- **Description**: Fixed-structure context types: `Validation` (enum), `ResourceInfo`, `ErrorInfo`, `QuotaFailure`, `PreconditionFailure`, `DebugInfo`, `RetryInfo`,
  and their sub-types: `FieldViolation`, `QuotaViolation`, `PreconditionViolation`.
- **Breaking Change Policy**: Removing/renaming a field or changing a field type requires a major version bump. Adding a new optional field is a minor version change.

### 7.2 External Integration Contracts

#### Error Response Contract (REST)

- [ ] `p1` - **ID**: `cpt-cf-contract-error-response-rest`

- **Direction**: provided by library to API consumers
- **Protocol/Format**: HTTP REST, `application/problem+json` (RFC 9457)
- **Compatibility**: Error response shapes are versioned via GTS identifiers. Breaking changes require a new GTS version suffix. Snapshot tests enforce response shape stability.

## 8. Use Cases

### API Consumer Handles Not Found Error

- [ ] `p1` - **ID**: `cpt-cf-usecase-consumer-handles-not-found`

**Actor**: `cpt-cf-actor-api-consumer`

**Preconditions**:

- Consumer calls a CyberFabric API endpoint requesting a resource by ID

**Main Flow**:

1. API endpoint receives request and queries for the resource
2. Resource is not found in the data store
3. Module code constructs `UserResourceError::not_found(id)` using a macro-declared resource type
4. Error middleware maps the canonical error to RFC 9457 Problem response
5. Consumer receives `404` with `type`, `title`, `detail`, and `context.type` (GTS identifier), `context.resource_name`
6. Consumer matches on the GTS type and `resource_name` to display an appropriate message

**Postconditions**:

- Consumer successfully parsed the error using the documented contract
- No retry attempted (not a transient error)

### CI Detects Accidental Error Contract Change

- [ ] `p1` - **ID**: `cpt-cf-usecase-ci-detects-breaking-change`

**Actor**: `cpt-cf-actor-ci-pipeline`

**Preconditions**:

- A developer or LLM agent submits a PR that modifies a context type field name

**Main Flow**:

1. CI runs `cargo build` — compile-time checks pass (field rename is structurally valid)
2. CI runs snapshot tests — a snapshot test fails because the JSON shape of the error response changed
3. CI runs semver check on `cf-modkit-errors` — reports a breaking change in a public type
4. PR is blocked with clear indication of which error contract changed and why it is breaking

**Postconditions**:

- The accidental breaking change is caught before merge
- Developer must either revert the change or explicitly update snapshots and bump the major version

**Alternative Flows**:

- **Intentional change**: Developer updates snapshots, bumps version, and adds a changelog entry. CI passes on re-run.

### LLM Agent Constructs Error Safely

- [ ] `p2` - **ID**: `cpt-cf-usecase-llm-constructs-error`

**Actor**: `cpt-cf-actor-llm-agent`

**Preconditions**:

- LLM agent is generating handler code for a new endpoint

**Main Flow**:

1. LLM generates code that returns `UserResourceError::not_found(id)` using a macro-declared resource type
2. LLM attempts to use a non-existent category — compile error stops it
3. LLM attempts to pass wrong context type to a category — compile error stops it
4. LLM attempts to construct `ResourceInfo` with a raw GTS string — compile error stops it (`pub(crate)` constructor)
5. Code compiles, snapshot tests pass, PR is green

**Postconditions**:

- LLM-generated code follows the canonical error contract
- No accidental error shape changes introduced

## 9. Acceptance Criteria

**Simplicity for LLMs and Humans**:

- [ ] A developer or LLM agent can construct any canonical error in a single line of code, without consulting HTTP/gRPC documentation
- [ ] Common library errors propagate through `?` without manual mapping at every call site
- [ ] The error vocabulary is finite and discoverable — code completion surfaces all available categories

**Extensibility**:

- [ ] New canonical categories can be added without breaking existing consumer error-handling code
- [ ] New resource types can be declared via macro without modifying the error framework
  **Accidental Changes Prevention**:

- [ ] Changing a category, context field, or GTS identifier is detected automatically before merge
- [ ] No error reaches API consumers outside the canonical vocabulary — there is no alternative path
- [ ] Production error responses for `internal`/`unknown` contain no stack traces, query text, or file paths

## 10. Dependencies

| Dependency              | Description                                                               | Criticality |
|-------------------------|---------------------------------------------------------------------------|-------------|
| `libs/modkit-errors`    | Crate where `CanonicalError`, context types, and `From` impls are defined | p1          |
| GTS Type System         | Provides globally unique identifiers for error categories                 | p1          |
| `cargo-semver-checks`   | CI tool for detecting breaking changes in public Rust APIs                | p1          |
| `insta` (or equivalent) | Snapshot testing library for error response shapes                        | p1          |
| RFC 9457 Problem        | Wire format for REST error responses                                      | p1          |

## 11. Assumptions

- API consumers parse error responses programmatically (not just display the `detail` string)
- The 16 canonical categories cover all error scenarios across all current and planned modules
- `cargo-semver-checks` (or equivalent) can detect field-level changes in public structs/enums
- Snapshot testing is feasible for all 16 categories with representative context payloads

## 12. Risks

| Risk                                 | Impact                             | Mitigation                                               |
|--------------------------------------|------------------------------------|----------------------------------------------------------|
| Snapshots become maintenance burden  | Devs skip updates, reducing trust  | One per category; auto-generate from enum                |
| 16 categories insufficient long-term | Ad-hoc types outside canonical set | Additive categories (minor); macro resource types        |
| LLM agents bypass compile checks     | Contract violated despite CI gates | Clippy lint on `Problem` construction; review guidelines |

## 13. Open Questions

- What is the exact snapshot format — full JSON response body or just the `context` payload?
- Should `cargo-semver-checks` be a hard gate (block merge) or a soft warning initially?
- Should the `#[resource_error]` macro live in `cf-modkit-errors` or in a separate `cf-modkit-errors-macro` crate?

## 14. Traceability

- **Design**: [DESIGN.md](./DESIGN.md)
- **ADRs**: [ADR/](./ADR/)
