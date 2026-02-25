---
status: accepted
date: 2026-02-25
decision-makers: Cyber Fabric Architects Committee
---

# ADR-0001: Canonical Error Enum with Category-Typed Constructors

**ID**: `cpt-cf-adr-canonical-error-impl`

## Context and Problem Statement

The DESIGN document (`docs/arch/errors/DESIGN.md`) defines 16 canonical error categories with fixed context types as the universal error vocabulary. We need to decide how to
represent these categories in Rust code, addressing four interrelated concerns: (1) the error type structure itself, (2) how module code constructs errors, (3) how errors map to
RFC 9457 Problem at the REST boundary, and (4) how errors from underlying libraries (sqlx, sea-orm, serde, reqwest, SDK errors) convert into canonical errors.

Without a canonical error type, each module would define its own ad-hoc error enum with module-specific variants, manual `From` impls for library errors, and a per-module mapping to
wire formats. This produces inconsistent error shapes across modules, duplicated mapping boilerplate, and no compile-time guarantee that all errors reach the API layer in a canonical form.

## Decision Drivers

* **Single vocabulary**: every module must express errors through the same 16 categories; no module-specific error shapes should leak to API consumers
* **Ergonomic construction**: creating a canonical error in domain code should be one line with no HTTP details
* **Zero transport leakage**: domain code must never reference HTTP status codes, gRPC codes, or Problem fields
* **Library error absorption**: the `?` operator must work to convert common library errors (anyhow, sqlx, serde_json, SDK errors) into canonical errors without manual match arms
  in every call site
* **Compile-time safety**: the Problem mapping must be exhaustive (adding a category forces handling the new variant) and the context type per category must be enforced by the type
  system
* **Minimal boilerplate**: a struct-per-error approach requiring 15-20 lines per error type is unacceptable

## Considered Options

* Option A: Single `CanonicalError` enum with category-typed variants
* Option B: Struct-per-error with error trait
* Option C: Trait object (`Box<dyn CanonicalError>`) with category method

## Decision Outcome

Chosen option: "Option A — Single `CanonicalError` enum with category-typed variants", because it is the only option that provides exhaustive match safety, zero-boilerplate
construction, a single `From<CanonicalError> for Problem` implementation, and natural `From` impls for library errors, while keeping the total error surface to one enum and a
handful of context structs.

### Consequences

**Advantages**:

* One `From<CanonicalError> for Problem` impl covers all modules (no per-module mapping needed)
* Adding a new category is a compiler error everywhere that matches on `CanonicalError` (exhaustive match)
* `UserResourceError::not_found(id)` is one line, no HTTP details
* Library errors convert via blanket `From` impls (e.g., `From<anyhow::Error>` maps to `Internal`)

**Disadvantages**:

* All 16 variants live in one enum, which is larger than a single struct (but enum size is bounded by the largest variant, and context types are small)
* `ErrorInfo.metadata` (HashMap) requires heap allocation, unlike the other context types which are stack-only

### Confirmation

* Code review: every module handler returns `ApiResult<T>` where `ApiResult = Result<T, CanonicalError>`, not `Result<T, Problem>`
* Compile-time: the `From<CanonicalError> for Problem` match is exhaustive; CI will fail if a category is added without a mapping
* Lint: a custom clippy lint (or `#[deny(unreachable_patterns)]`) verifies that no module constructs a `Problem` directly, bypassing the canonical path

## Advantages and Disadvantages of the Options

### Option A: Single `CanonicalError` enum with category-typed variants

A single Rust enum with 16 variants, each carrying its category-specific context struct. Construction via associated functions (`CanonicalError::not_found(...)`,
`CanonicalError::invalid_argument(...)`). A single `From<CanonicalError> for Problem` impl in `modkit-errors` handles the complete REST mapping. Library error absorption via
`From<T> for CanonicalError` impls.

**Error type structure**:

```rust
enum CanonicalError {
    NotFound { ctx: ResourceInfo, message: String, resource_type: Option<String> },
    InvalidArgument { ctx: Validation, message: String, resource_type: Option<String> },
    Internal { ctx: DebugInfo, message: String, resource_type: Option<String> },
    // ... 13 more variants (including ServiceUnavailable, not Unavailable)
}
```

**Error creation in module code**:

```rust
// Resource-scoped errors (via macro-declared types):
// Declared via attribute macro — generates typed constructors:
#[resource_error("gts.cf.core.upstreams.upstream.v1")]
struct UpstreamResourceError;

UpstreamResourceError::not_found(id)
UpstreamResourceError::already_exists(alias)
UpstreamResourceError::invalid_argument(Validation::field("alias", "must not be empty"))

// Non-resource errors (direct construction):
CanonicalError::invalid_argument(Validation::format("request body is not valid JSON"))
CanonicalError::internal(e)  // blanket From<impl Display>
```

**Problem mapping** (one impl for all modules):

```rust
impl From<CanonicalError> for Problem {
    fn from(err: CanonicalError) -> Self {
        let (status, title) = match &err {
            CanonicalError::NotFound { .. } => (404, "Not Found"),
            CanonicalError::InvalidArgument { .. } => (400, "Invalid Argument"),
            // ... exhaustive match
        };
        Problem::new(status, title, err.message())
            .with_type(err.gts_type())
            .with_context(err.context_json())
    }
}
```

**Library error conversion** (blanket impls in `modkit-errors`):

```rust
impl From<anyhow::Error> for CanonicalError { ... => Internal }
impl From<serde_json::Error> for CanonicalError { ... => InvalidArgument }
impl From<sea_orm::DbErr> for CanonicalError { ... => Internal }
```

**Advantages**:

* Exhaustive match in `From<CanonicalError> for Problem` catches missing mappings at compile time
* One-line construction: `UpstreamResourceError::not_found(id)`
* A single `From` impl covers all modules (no per-module mapping needed)
* Blanket `From` impls for common library errors eliminate repetitive `map_err` calls
* `message` is a common field across all variants, not duplicated
* The enum is `Send + Sync + 'static` with no trait objects

**Trade-offs**:

* The enum has 16 variants, which is large but bounded and well-documented

**Disadvantages**:

* `ErrorInfo.metadata` (HashMap) forces a heap allocation even for simple permission errors
* Adding a 17th category (unlikely but possible) touches the enum, all match arms, and the Problem mapping

### Option B: Struct-per-error with error trait

Each error is a standalone Rust struct implementing an `ErrorSchema` trait with `STATUS` and `TITLE` constants. The error-to-Problem conversion is per-struct via `into_problem()`.

**Error type structure**:

```rust
struct UserNotFound {
    r#type: String,
    resource_name: String,
}

impl ErrorSchema for UserNotFound {
    const STATUS: u16 = 404;
    const TITLE: &'static str = "Not Found";
    const SCHEMA_ID: &'static str = "gts.cf.core.errors.err.v1~cf.core.errors.not_found.v1~";
}
```

**Error creation**:

```rust
return Err(UserNotFound { r#type: "gts.cf.core.users.user.v1".into(), resource_name: id.to_string() }.into());
```

**Problem mapping**: each struct implements `ErrorSchema::into_problem(&self) -> Problem`.

**Library error conversion**: no blanket impls; each module manually wraps library errors into specific struct instances.

**Advantages**:

* Each error is self-contained with its own schema
* Adding a new error requires no changes to existing code

**Disadvantages**:

* 15-20 lines of boilerplate per error type (struct + trait impl)
* No exhaustive match — impossible to verify all errors are handled
* No blanket `From` impls for library errors; every module manually maps each library error
* `STATUS: u16` embeds HTTP details in domain code (violates transport agnosticism)
* The `Problem` mapping is scattered across dozens of `impl ErrorSchema` blocks instead of one central location

### Option C: Trait object (`Box<dyn CanonicalError>`) with category method

A trait `CanonicalError` with `fn category(&self) -> Category` and `fn context(&self) -> ContextValue`. Errors are `Box<dyn CanonicalError>`. Modules implement the trait for their
own error types.

**Error type structure**:

```rust
trait CanonicalError: Error + Send + Sync {
    fn category(&self) -> Category;
    fn context(&self) -> ContextValue;
    fn message(&self) -> &str;
}
```

**Error creation** (raw, without helpers):

```rust
return Err(Box::new(MyNotFoundError { r#type: "gts.cf.core.users.user.v1", id }) as Box<dyn CanonicalError>);
```

With `From` impls for each concrete error type, the `?` operator and `.into()` shorten this:

```rust
// Requires: impl From<MyNotFoundError> for Box<dyn CanonicalError>
return Err(MyNotFoundError { r#type: "gts.cf.core.users.user.v1", id }.into());
```

A helper constructor on the trait object could reduce it further:

```rust
// Hypothetical convenience function:
fn not_found(r#type: &str, id: impl Display) -> Box<dyn CanonicalError> { ... }

return Err(not_found("gts.cf.core.users.user.v1", id));
```

However, each of these ergonomic layers requires additional boilerplate elsewhere: a `From` impl per concrete error type, or free-standing constructor functions that duplicate what the enum constructors in Option A provide natively.

**Problem mapping**: one `From<Box<dyn CanonicalError>> for Problem` impl that dispatches on `category()`.

**Library error conversion**: wrapper structs that implement `CanonicalError` for each library error type.

**Advantages**:

* Modules can define their own error structs while conforming to the category contract
* Extensible without modifying the trait

**Disadvantages**:

* `Box<dyn>` heap-allocates every error (even simple ones like `NotFound`)
* No compile-time exhaustiveness — a module can return any category from any type
* `category()` returns a runtime value, not enforced by the type system
* `?` operator requires `From` impls for `Box<dyn CanonicalError>` which are awkward
* The `CanonicalError` trait would collide with `std::error::Error` trait in scope

## More Information

**Library error blanket impls**:

The following `From` impls ship with `modkit-errors` out of the box:

| Library Type        | Maps To           | Rationale                                                                                                                                       |
|---------------------|-------------------|-------------------------------------------------------------------------------------------------------------------------------------------------|
| `anyhow::Error`     | `Internal`        | Untyped errors are internal by definition                                                                                                       |
| `sea_orm::DbErr`    | `Internal`        | Database errors are infrastructure failures                                                                                                     |
| `sqlx::Error`       | `Internal`        | Database driver errors are infrastructure failures                                                                                              |
| `serde_json::Error` | `InvalidArgument` | Serialization failures indicate malformed input (at API boundary) or internal bugs (in domain) — callers can override via explicit construction |
| `std::io::Error`    | `Internal`        | IO failures are infrastructure errors                                                                                                           |

Modules that need finer-grained mapping (e.g., `sqlx::Error::RowNotFound` → `NotFound`) implement their own `From` instead of relying on the blanket.

## Traceability

- **DESIGN**: [DESIGN.md](../DESIGN.md)

This decision directly addresses the following design elements:

* `cpt-cf-principle-transport-agnosticism` — Option A keeps HTTP/gRPC details out of the enum; transport mapping is in one `From` impl at the boundary
* `cpt-cf-principle-single-error-gateway` — The `CanonicalError` enum is the single type accepted by API layers
* `cpt-cf-principle-fixed-context-structures` — Each enum variant carries exactly one context type, enforced by the type system
* `cpt-cf-principle-fail-safe-fallback` — Blanket `From` impls for library errors ensure unhandled errors map to `Internal`
* `cpt-cf-constraint-gts-code-format` — GTS identifiers are compile-time constants derived from the variant name
* `cpt-cf-constraint-macro-gts-construction` — Resource types declared via attribute macros with compile-time GTS format validation
* `cpt-cf-component-canonical-error` — This ADR defines the implementation approach for this component
* `cpt-cf-component-rest-mapping` — One exhaustive `From<CanonicalError> for Problem` impl covers all modules
