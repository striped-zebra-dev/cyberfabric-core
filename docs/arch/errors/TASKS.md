# Migration Plan — Universal Error Architecture

Tracks the migration from the legacy error system (`Problem::new()` / `ErrDef` / `declare_errors!` / `ErrorCode`) to the canonical error architecture defined
in [DESIGN.md](./DESIGN.md). The PoC at `~/projects/canonical-errors/` validates all core pieces; this plan ports them into production.

## Strategy

Five phases, each independently committable. Phase 1 is purely additive — if the migration stalls, existing code still works.

| Phase                          | What                                   | Breaking?                                               | Commit boundary     |
|--------------------------------|----------------------------------------|---------------------------------------------------------|---------------------|
| **1: Foundation**              | Port PoC into `libs/modkit-errors`     | No                                                      | Merge to main       |
| **2: Example migration**       | Migrate `examples/modkit/users-info/`  | Wire format for users-info only                         | Merge to main       |
| **3: System module migration** | Migrate all system modules             | Wire format per module                                  | One or more merges  |
| **4: Legacy removal**          | Remove all legacy error infrastructure | Yes — compile errors for remaining direct Problem usage | Final cleanup merge |
| **5: Documentation**           | Update all docs to reflect canonical errors | No (docs only)                                      | Parallel with 2–4   |

---

## Phase 1 — Foundation (non-breaking, additive only)

Port PoC into `libs/modkit-errors`. All existing code compiles unchanged after this phase.

### 1.1 Context type structs

> Traces to: `cpt-cf-component-context-types`, `cpt-cf-interface-context-schemas`

- [ ] 1.1.1 Port `ResourceInfoV1` struct with `#[struct_to_gts_schema]`, GTS ID `gts.cf.core.errors.resource_info.v1~`, fields: `resource_type`, `resource_name`, `description`. Add
  `pub type ResourceInfo = ResourceInfoV1;`
- [ ] 1.1.2 Port `ErrorInfoV1` struct with GTS ID `gts.cf.core.errors.error_info.v1~`, fields: `reason`, `domain`, `metadata: HashMap<String, String>`. Add `with_metadata(k, v)`
  builder
- [ ] 1.1.3 Port `DebugInfoV1` struct with GTS ID `gts.cf.core.errors.debug_info.v1~`, fields: `detail`, `stack_entries: Vec<String>`. Add `with_stack(entries)` builder
- [ ] 1.1.4 Port `RetryInfoV1` struct with GTS ID `gts.cf.core.errors.retry_info.v1~`, field: `retry_after_seconds: u64`
- [ ] 1.1.5 ~~Port `RequestInfoV1`~~ — Removed. `cancelled` and `deadline_exceeded` use a plain message string (`trace_id` on Problem is sufficient for correlation)
- [ ] 1.1.6 Port `FieldViolationV1` struct with GTS ID `gts.cf.core.errors.field_violation.v1~`, fields: `field`, `description`, `reason`
- [ ] 1.1.7 Port `QuotaViolationV1` and `QuotaFailureV1` structs with GTS IDs
- [ ] 1.1.8 Port `PreconditionViolationV1` and `PreconditionFailureV1` structs with GTS IDs
- [ ] 1.1.9 Port `Validation` enum (`FieldViolations`, `Format`, `Constraint`) with manual `GtsSchema` impl and GTS ID `gts.cf.core.errors.validation.v1~`
- [ ] 1.1.10 Add unit tests for all context types: construction, serialization (verify `gts_type` is skipped), GTS schema generation

### 1.2 CanonicalError enum

> Traces to: `cpt-cf-component-canonical-error`, `cpt-cf-interface-canonical-categories`, `cpt-cf-constraint-gts-code-format`

- [ ] 1.2.1 Port `CanonicalError` enum with 16 struct-style variants, each with `ctx`, `message: String`, `resource_type: Option<String>`, `debug_info: Option<DebugInfo>`
- [ ] 1.2.2 Port 16 constructors with sensible default messages and `resource_type: None`, `debug_info: None`
- [ ] 1.2.3 Port `unknown()` convenience constructor that takes `impl Into<String>` and wraps in `DebugInfo`
- [ ] 1.2.4 Port builder methods: `with_message()`, `with_resource_type()`, `with_debug_info()`
- [ ] 1.2.5 Port accessors: `message()`, `resource_type()`, `debug_info()`, `gts_type()`, `status_code()`, `title()`
- [ ] 1.2.6 Port `Display` impl (`"{category}: {message}"`) and derive `Debug`, `Clone`
- [ ] 1.2.7 Port GTS type constants: 16 compound identifiers in format `gts.cf.core.errors.err.v1~cf.core.errors.{category}.v1~`
- [ ] 1.2.8 Add unit tests: constructors, builders, accessors, Display format, GTS type mapping, status code mapping for all 16 variants

### 1.3 Problem mapping (coexistence)

> Traces to: `cpt-cf-component-rest-mapping`, `cpt-cf-constraint-rfc9457`, `cpt-cf-interface-problem-wire-format`

- [ ] 1.3.1 Add `context: Option<serde_json::Value>` field to existing `Problem` struct with `#[serde(skip_serializing_if = "Option::is_none")]`
- [ ] 1.3.2 Add `debug: Option<serde_json::Value>` field to existing `Problem` struct with `#[serde(skip_serializing_if = "Option::is_none")]`
- [ ] 1.3.3 Change existing `code: String` field to use `#[serde(skip_serializing_if = "String::is_empty")]` (so new-path errors omit it)
- [ ] 1.3.4 Implement `Problem::from_error(err: CanonicalError) -> Self` — production mode, omits debug info, sets `code` to empty string, `errors` to `None`
- [ ] 1.3.5 Implement `Problem::from_error_debug(err: CanonicalError) -> Self` — debug mode, includes debug info if present
- [ ] 1.3.6 Implement `From<CanonicalError> for Problem` delegating to `from_error()`
- [ ] 1.3.7 Verify existing `Problem::new()`, `with_code()`, `with_errors()` still work (regression tests)
- [ ] 1.3.8 Add showcase tests: one per category via `from_error`, verifying full JSON shape (no `code`, no `errors`, has `context`)
- [ ] 1.3.9 Add debug showcase test: `from_error_debug` with `with_debug_info`, verifying `"debug"` key present

### 1.4 `#[resource_error]` attribute macro

> Traces to: `cpt-cf-constraint-macro-gts-construction`

- [ ] 1.4.1 Port `#[resource_error]` proc macro into `libs/modkit-errors-macro/` alongside existing `declare_errors!`
- [ ] 1.4.2 Port compile-time GTS format validation
- [ ] 1.4.3 Port 15 constructor generation (all except `service_unavailable`): ResourceInfo categories take `impl Into<String>`, others take context type directly
- [ ] 1.4.4 Port `.with_resource_type(gts_type)` injection on every generated constructor
- [ ] 1.4.5 Add compilation tests: valid GTS ID compiles, invalid GTS ID fails with clear error
- [ ] 1.4.6 Add functional tests: macro-generated constructors produce correct `CanonicalError` variants with `resource_type` set

### 1.5 Blanket `From` impls for library errors

> Traces to: `cpt-cf-component-canonical-error`

- [ ] 1.5.1 Implement `From<anyhow::Error> for CanonicalError` → `Internal` with source error detail in `ctx.detail`, safe default message
- [ ] 1.5.2 Implement `From<sea_orm::DbErr> for CanonicalError` → `Internal`
- [ ] 1.5.3 Implement `From<sqlx::Error> for CanonicalError` → `Internal`
- [ ] 1.5.4 Implement `From<serde_json::Error> for CanonicalError` → `InvalidArgument`
- [ ] 1.5.5 Implement `From<std::io::Error> for CanonicalError` → `Internal`
- [ ] 1.5.6 Add tests: each `From` impl preserves source error in context detail, uses safe default message, `?` operator works in `Result<T, CanonicalError>`

### 1.6 Snapshot tests

> Traces to: `cpt-cf-constraint-error-contract-stability`

- [ ] 1.6.1 Add `insta` snapshot test for each of the 16 categories (full JSON output via `Problem::from_error`)
- [ ] 1.6.2 Add snapshot test for resource-scoped variant (macro-generated, with `resource_type` in context)
- [ ] 1.6.3 Add snapshot test for debug mode (`Problem::from_error_debug` with `debug_info`)

### 1.7 Re-exports and public API

> Traces to: `cpt-cf-component-canonical-error`

- [ ] 1.7.1 Re-export `CanonicalError`, all context types, and `#[resource_error]` from `libs/modkit-errors/src/lib.rs`
- [ ] 1.7.2 Verify full `cargo build` of the workspace succeeds — no existing code broken
- [ ] 1.7.3 Verify full `cargo test` of the workspace succeeds — all existing tests pass

---

## Phase 2 — Example module migration (validate pattern)

Migrate `examples/modkit/users-info/` to prove the migration pattern. After this phase, `users-info` uses `CanonicalError` exclusively. All other modules unchanged.

> Traces to: `cpt-cf-principle-single-error-gateway`, `cpt-cf-constraint-macro-gts-construction`, `cpt-cf-seq-domain-error-to-rest`

- [ ] 2.1 Declare resource types with `#[resource_error("gts.cf.examples.users_info.user.v1")]` (and any other resource types in this module)
- [ ] 2.2 Replace `From<DomainError> for Problem` with `From<DomainError> for CanonicalError` — map each domain error to the appropriate canonical category with typed context
- [ ] 2.3 Replace all `Problem::new()` / `ErrorCode::xxx().with_context()` calls with `XxxResourceError::category()` or `CanonicalError::category()` calls
- [ ] 2.4 Update handler return types from `Result<T, Problem>` to `Result<T, CanonicalError>`
- [ ] 2.5 Remove `gts/errors.json` and `declare_errors!` invocation from `users-info`
- [ ] 2.6 Remove `ErrorCode` enum imports
- [ ] 2.7 Verify `cargo build` and `cargo test` pass for the workspace
- [ ] 2.8 Verify `users-info` error responses match expected canonical format (manual or integration test)

---

## Phase 3 — System module migration

Migrate each system module. Order: simplest first. Each module follows the same pattern as Phase 2. OAGW is one PR despite being the largest.

### 3.1 file-parser

> Traces to: `cpt-cf-principle-single-error-gateway`, `cpt-cf-seq-domain-error-to-rest`

- [ ] 3.1.1 Declare resource types, replace `From<DomainError> for Problem` with `From<DomainError> for CanonicalError`
- [ ] 3.1.2 Replace `Problem::new()` calls, update handler return types to `Result<T, CanonicalError>`
- [ ] 3.1.3 Remove legacy error infrastructure (`gts/errors.json`, `declare_errors!`, `ErrorCode`)
- [ ] 3.1.4 Verify build and tests pass

### 3.2 simple-user-settings

> Traces to: `cpt-cf-principle-single-error-gateway`, `cpt-cf-seq-domain-error-to-rest`

- [ ] 3.2.1 Declare resource types, replace `From<DomainError> for Problem` with `From<DomainError> for CanonicalError`
- [ ] 3.2.2 Replace `ErrorCode::xxx().with_context()` calls, update handler return types to `Result<T, CanonicalError>`
- [ ] 3.2.3 Remove `gts/errors.json`, `declare_errors!`, `ErrorCode`
- [ ] 3.2.4 Verify build and tests pass

### 3.3 nodes-registry

> Traces to: `cpt-cf-principle-single-error-gateway`, `cpt-cf-seq-domain-error-to-rest`

- [ ] 3.3.1 Declare resource types, replace error mappings
- [ ] 3.3.2 Replace `Problem::new()` / `ErrorCode` calls, update handler return types to `Result<T, CanonicalError>`
- [ ] 3.3.3 Remove legacy error infrastructure
- [ ] 3.3.4 Verify build and tests pass

### 3.4 types-registry

> Traces to: `cpt-cf-principle-single-error-gateway`, `cpt-cf-seq-domain-error-to-rest`

- [ ] 3.4.1 Declare resource types, replace error mappings
- [ ] 3.4.2 Replace `ErrorCode` calls, update handler return types to `Result<T, CanonicalError>`
- [ ] 3.4.3 Remove legacy error infrastructure
- [ ] 3.4.4 Verify build and tests pass

### 3.5 api-gateway

> Traces to: `cpt-cf-principle-single-error-gateway`, `cpt-cf-component-error-middleware`

- [ ] 3.5.1 Replace `Problem::new()` in auth middleware with `CanonicalError::unauthenticated()` / `CanonicalError::permission_denied()`
- [ ] 3.5.2 Replace `Problem::new()` in license validation middleware
- [ ] 3.5.3 Replace `Problem::new()` in MIME validation middleware
- [ ] 3.5.4 Update middleware return types to use `CanonicalError`
- [ ] 3.5.5 Verify build and tests pass

### 3.6 oagw (single PR)

> Traces to: `cpt-cf-principle-single-error-gateway`, `cpt-cf-seq-domain-error-to-rest`

- [ ] 3.6.1 Declare resource types for all OAGW resources (upstreams, routes, consumers, etc.)
- [ ] 3.6.2 Replace `From<DomainError> for Problem` with `From<DomainError> for CanonicalError` — map all 15 domain error variants
- [ ] 3.6.3 Replace all `Problem::new().with_type().with_code()` chains in handler code
- [ ] 3.6.4 Update handler return types to `Result<T, CanonicalError>`
- [ ] 3.6.5 Remove error source header handling if it relied on `code` field (or adapt to use `gts_type()`)
- [ ] 3.6.6 Remove legacy error infrastructure
- [ ] 3.6.7 Verify build and tests pass

---

## Phase 4 — Legacy removal

All modules are now on `CanonicalError`. Remove legacy infrastructure and finalize the `Problem` struct.

### 4.1 Clean up Problem struct

> Traces to: `cpt-cf-interface-problem-wire-format`, `cpt-cf-constraint-error-contract-stability`

- [ ] 4.1.1 Remove `code: String` field from `Problem`
- [ ] 4.1.2 Remove `errors: Option<Vec<ValidationViolation>>` field from `Problem`
- [ ] 4.1.3 Change `context: Option<serde_json::Value>` to `context: serde_json::Value` (non-optional)
- [ ] 4.1.4 Remove `with_code()` builder method
- [ ] 4.1.5 Remove `with_errors()` builder method
- [ ] 4.1.6 Remove or deprecate `Problem::new()` — all construction goes through `CanonicalError`
- [ ] 4.1.7 Update snapshot tests to reflect final Problem shape

### 4.2 Remove legacy error infrastructure

> Traces to: `cpt-cf-constraint-error-contract-stability`

- [ ] 4.2.1 Remove `ErrDef` struct from `libs/modkit-errors/src/catalog.rs`
- [ ] 4.2.2 Remove `declare_errors!` macro from `libs/modkit-errors-macro/`
- [ ] 4.2.3 Remove `ValidationViolation` struct (replaced by `FieldViolation` in `Validation` context)
- [ ] 4.2.4 Remove `ValidationError` and `ValidationErrorResponse` structs
- [ ] 4.2.5 Remove convenience constructors from `libs/modkit/src/api/problem.rs` (`bad_request`, `not_found`, `conflict`, `internal_error`)
- [ ] 4.2.6 Remove all `gts/errors.json` files from modules and examples
- [ ] 4.2.7 Remove any remaining `ErrorCode` enum references

### 4.3 Final verification

> Traces to: `cpt-cf-constraint-error-contract-stability`, `cpt-cf-principle-single-error-gateway`

- [ ] 4.3.1 `cargo build` — workspace compiles with no legacy error references
- [ ] 4.3.2 `cargo test` — all tests pass
- [ ] 4.3.3 Grep verification: zero occurrences of `Problem::new`, `ErrDef`, `declare_errors!`, `ErrorCode`, `with_code()`, `with_errors()` in module code
- [ ] 4.3.4 Grep verification: zero `gts/errors.json` files in repository

---

## Phase 5 — Documentation updates

Update all documentation to reflect the canonical error architecture. Can run in parallel with Phases 2–4 once Phase 1 is merged.

### 5.1 ModKit Unified System docs

> Primary developer-facing documentation. These are the docs new contributors read first.

- [ ] 5.1.1 Rewrite `docs/modkit_unified_system/05_errors_rfc9457.md` — replace `Problem::new()` / `ProblemType` / `ErrorCode` patterns with `CanonicalError` categories, `#[resource_error]` macro, typed context structs, and `Result<T, CanonicalError>` handler return types
- [ ] 5.1.2 Update `docs/modkit_unified_system/01_overview.md` — update error handling summary to reference canonical errors instead of raw Problem construction
- [ ] 5.1.3 Update `docs/modkit_unified_system/03_clienthub_and_plugins.md` — update error handling section to show `CanonicalError` in SDK error boundaries
- [ ] 5.1.4 Update `docs/modkit_unified_system/04_rest_operation_builder.md` — update error registration examples to reflect canonical error categories
- [ ] 5.1.5 Update `docs/modkit_unified_system/06_authn_authz_secure_orm.md` — update error handling patterns for auth flows
- [ ] 5.1.6 Update `docs/modkit_unified_system/07_odata_pagination_select_filter.md` — update OData validation error handling to use `CanonicalError::invalid_argument` with `Validation` context
- [ ] 5.1.7 Update `docs/modkit_unified_system/10_checklists_and_templates.md` — update error handling checklist and test templates
- [ ] 5.1.8 Update `docs/modkit_unified_system/README.md` — update framework overview error references

### 5.2 Architecture-level docs

> High-level architecture and project governance docs.

- [ ] 5.2.1 Update `docs/ARCHITECTURE_MANIFEST.md` — update error handling section (RFC-9457 + canonical categories), update `modkit-errors` library description
- [ ] 5.2.2 Update `docs/REPO_PLAYBOOK.md` — update error handling standards references
- [ ] 5.2.3 Update `docs/MODULES.md` — update error mapping component descriptions to reference canonical error flow

### 5.3 Authorization docs

> Auth-related docs that define error types and handling.

- [ ] 5.3.1 Update `docs/arch/authorization/DESIGN.md` — map `AuthNResolverError` types to canonical categories
- [ ] 5.3.2 Update `docs/arch/authorization/AUTHN_JWT_OIDC_PLUGIN.md` — update error handling section to use `CanonicalError::unauthenticated` / `service_unavailable` / `internal`
- [ ] 5.3.3 Review `docs/arch/authorization/ADR/` — add notes about canonical error alignment where relevant

### 5.4 Checklists and process docs

> Review templates and coding standards.

- [ ] 5.4.1 Update `docs/checklists/DESIGN.md` — update error handling architecture checklist (REL-DESIGN-002)
- [ ] 5.4.2 Update `docs/checklists/FEATURE.md` — update security error handling (SEC-FDESIGN-006) and error handling completeness (REL-FDESIGN-001) checklists
- [ ] 5.4.3 Update `docs/checklists/CODING.md` — update explicit error handling standards (ERR-CODE-001)
- [ ] 5.4.4 Update `docs/checklists/PRD.md` — update error handling expectations (REL-PRD-003)

### 5.5 Module SDK READMEs

> Update after each module is migrated in Phase 3. Track per-module.

- [ ] 5.5.1 Update OAGW SDK README error examples after 3.6 migration
- [ ] 5.5.2 Update other module SDK READMEs as they migrate (credstore, tenant-resolver, authz-resolver, authn-resolver)

### 5.6 Observability docs

- [ ] 5.6.1 Update `docs/TRACING_SETUP.md` — document canonical error observability (internal vs unknown alerting, trace_id correlation)

---

## Separate workstreams (orthogonal to this migration)

The following items from the original task list are independent of the canonical error migration and will be tracked separately:

- **W3C trace ID extraction** — Replace `tracing::Span::current().id()` (u64 span ID) with a proper W3C trace ID from `opentelemetry` span context. Orthogonal; can be done before,
  during, or after migration.
- **Error middleware catch-all** — Catches unhandled errors (panics, framework errors), wraps as `CanonicalError::Internal`, strips `DebugInfo.stack_entries` in production,
  attaches `trace_id`. Depends on Phase 1 completion.
- **Semver CI gate** — Run `cargo-semver-checks` on `cf-modkit-errors` in CI. Should be added after Phase 1 snapshot tests are in place.
