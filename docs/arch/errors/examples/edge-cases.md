# Edge Cases & Disambiguation Examples

This document covers scenarios where the correct canonical category is non-obvious, security-sensitive, or requires special handling.

> **Resource types**: Examples use macro-declared resource types (see `cpt-cf-constraint-macro-gts-construction`). These generate constructors for **all** canonical categories:
> ```rust
> #[resource_error("gts.cf.core.users.user.v1")]
> struct UserResourceError;
>
> #[resource_error("gts.cf.oagw.upstreams.upstream.v1")]
> struct UpstreamResourceError;
> ```

---

## Production vs Debug Mode

> **Note**: Debug/production mode switching is handled by error middleware (T10), which is not yet implemented in the PoC. The examples below show the target behavior. Without middleware, the PoC always produces the "Production Mode" output shape: `detail` is the safe generic message from the constructor, while `context.detail` contains the actual error text.

The same `internal` error produces different responses depending on the environment.

### Debug Mode

```http
HTTP/1.1 500 Internal Server Error
Content-Type: application/problem+json

{
  "type": "gts.cf.core.errors.err.v1~cf.core.errors.internal.v1~",
  "title": "Internal",
  "status": 500,
  "detail": "column \"email\" of relation \"users\" violates not-null constraint",
  "instance": "/v1/users",
  "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
  "context": {
    "detail": "column \"email\" of relation \"users\" violates not-null constraint",
    "stack_entries": [
      "cf_users::service::create_user (src/service.rs:42)",
      "cf_users::handler::post_user (src/handler.rs:18)"
    ]
  }
}
```

### Production Mode

```http
HTTP/1.1 500 Internal Server Error
Content-Type: application/problem+json

{
  "type": "gts.cf.core.errors.err.v1~cf.core.errors.internal.v1~",
  "title": "Internal",
  "status": 500,
  "detail": "An internal error occurred. Please retry later.",
  "instance": "/v1/users",
  "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
  "context": {
    "detail": "An internal error occurred. Please retry later.",
    "stack_entries": []
  }
}
```

> The `trace_id` is always present — consumers use it to correlate with support requests. The actual error details are available only in server logs.

### Debug Info on Non-Internal Errors

Any canonical error category can carry optional debug info via `.with_debug_info()`. This is useful for attaching diagnostic context (SQL queries, cache keys, upstream responses) to errors like `not_found` or `permission_denied` without changing their category or semantics.

```rust
UserResourceError::not_found("user-123")
    .with_debug_info(DebugInfo::new("SELECT * FROM users WHERE id = $1 returned 0 rows")
        .with_stack(vec!["cf_users::repo::find_by_id (src/repo.rs:42)".into()]))
```

In debug mode (`Problem::from_error_debug(err)`), the response includes a top-level `"debug"` key:

```json
{
  "type": "gts.cf.core.errors.err.v1~cf.core.errors.not_found.v1~",
  "title": "Not Found",
  "status": 404,
  "detail": "Resource not found",
  "context": {
    "resource_type": "gts.cf.core.users.user.v1",
    "resource_name": "user-123",
    "description": "Resource not found"
  },
  "debug": {
    "detail": "SELECT * FROM users WHERE id = $1 returned 0 rows",
    "stack_entries": [
      "cf_users::repo::find_by_id (src/repo.rs:42)"
    ]
  }
}
```

In production mode (`Problem::from(err)` or `Problem::from_error(err)`), the `"debug"` key is absent — the response is identical to a normal `not_found`.

---

## not_found vs permission_denied (Security)

When a resource exists but the caller lacks permission, the choice depends on whether revealing the resource's existence is a security risk.

### Default: Return permission_denied

Use when the resource type is not sensitive (caller knows resources exist in general):

```rust
// Caller is authenticated but not authorized for this tenant's users
UserResourceError::permission_denied(
    ErrorInfo::new("CROSS_TENANT_ACCESS", "auth.cyberfabric.io")
)
```

```http
HTTP/1.1 403 Forbidden
```

### Security-Sensitive: Return not_found

Use when revealing existence leaks information (e.g., user enumeration):

```rust
// Caller queries another user's private profile — don't confirm it exists
UserResourceError::not_found(user_id)
```

```http
HTTP/1.1 404 Not Found
```

> **Rule of thumb**: If the caller should not know whether the resource exists, use `not_found`. Document this decision in the module's API specification.

---

## invalid_argument vs failed_precondition

### invalid_argument — Input is wrong regardless of system state

```rust
// Email format is always invalid, no matter what state the system is in
CanonicalError::invalid_argument(
Validation::fields([
FieldViolation::new("email", "must be a valid email address", "INVALID_FORMAT")
])
)
```

### failed_precondition — Input is valid but system state prevents the operation

```rust
// The document ID and format are valid, but it's already published
CanonicalError::failed_precondition(
PreconditionFailure::new([
PreconditionViolation::new("STATE", "document.status", "Document is already published; unpublish before editing")
])
)
```

---

## aborted vs failed_precondition

Both signal "can't proceed," but differ in cause and retry strategy.

### aborted — Transient concurrency conflict (retry the same request)

```rust
// Two users updated the same upstream simultaneously; one gets aborted
UpstreamResourceError::aborted(
    ErrorInfo::new("OPTIMISTIC_LOCK_FAILURE", "cf.oagw")
        .with_metadata("expected_version", "3")
        .with_metadata("actual_version", "5")
)
```

> **Client action**: Re-read the resource, apply changes to the new version, retry.

### failed_precondition — State violation (fix the state, then retry)

```rust
// Tenant has active users; can't delete until they're removed
CanonicalError::failed_precondition(
PreconditionFailure::new([
PreconditionViolation::new("STATE", "tenant.users", "Remove all active users before deleting the tenant")
])
)
```

> **Client action**: Perform the corrective action described in `description`, then retry.

---

## invalid_argument vs out_of_range

### invalid_argument — Structurally wrong value

```rust
// "abc" is not a number at all
CanonicalError::invalid_argument(
Validation::fields([
FieldViolation::new("page", "must be a positive integer", "INVALID_TYPE")
])
)
```

### out_of_range — Valid type, but outside accepted bounds

```rust
// 50 is a valid integer, but the dataset only has 12 pages
CanonicalError::out_of_range(
Validation::constraint("Page 50 is beyond the last page (12)")
)
```

---

## internal vs unknown

### internal — Server recognizes its own bug

```rust
// An invariant was violated in our code
CanonicalError::internal(DebugInfo::new("User record has no tenant_id; data invariant violated"))
```

### unknown — Error from external source, category undetermined

```rust
// External SDK returned an opaque error we can't classify
CanonicalError::unknown("Unexpected response from payment provider")
```

> Both produce HTTP 500. The distinction matters for observability: `internal` alerts are actionable bugs; `unknown` alerts need investigation to find the source.

---

## Library Error Absorption

> **Note**: Blanket `From` impls for library errors (T7) are not yet in the PoC. The examples below show the target behavior.

Common library errors convert automatically via the `?` operator. No manual mapping needed.

### sqlx / sea_orm → internal

```rust
async fn get_user(&self, id: Uuid) -> Result<User, CanonicalError> {
    // sqlx::Error automatically converts to CanonicalError::Internal
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
        .bind(id)
        .fetch_one(&self.pool)
        .await?;
    Ok(user)
}
```

### serde_json → invalid_argument

```rust
fn parse_config(raw: &str) -> Result<Config, CanonicalError> {
    // serde_json::Error automatically converts to CanonicalError::InvalidArgument
    let config: Config = serde_json::from_str(raw)?;
    Ok(config)
}
```

### Overriding the default mapping

When the blanket mapping is too coarse, construct explicitly:

```rust
async fn get_user(&self, id: Uuid) -> Result<User, CanonicalError> {
    match sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
        .bind(id)
        .fetch_one(&self.pool)
        .await
    {
        Ok(user) => Ok(user),
        Err(sqlx::Error::RowNotFound) => Err(UserResourceError::not_found(id)),
        Err(e) => Err(e.into()), // other sqlx errors → Internal
    }
}
```

---

## Validation with Multiple Violations (Accumulation Pattern)

Collect all validation errors before returning, so the client can fix everything at once.

```rust
fn validate_create_user(req: &CreateUserRequest) -> Result<(), CanonicalError> {
    let mut violations = Vec::new();

    if req.email.is_empty() {
        violations.push(FieldViolation::new("email", "is required", "REQUIRED"));
    } else if !is_valid_email(&req.email) {
        violations.push(FieldViolation::new("email", "must be a valid email address", "INVALID_FORMAT"));
    }

    if req.name.len() < 2 {
        violations.push(FieldViolation::new("name", "must be at least 2 characters", "TOO_SHORT"));
    }

    if req.name.len() > 100 {
        violations.push(FieldViolation::new("name", "must be at most 100 characters", "TOO_LONG"));
    }

    if violations.is_empty() {
        Ok(())
    } else {
        Err(UserResourceError::invalid_argument(Validation::fields(violations)))
    }
}
```

```http
HTTP/1.1 400 Bad Request
Content-Type: application/problem+json

{
  "type": "gts.cf.core.errors.err.v1~cf.core.errors.invalid_argument.v1~",
  "title": "Invalid Argument",
  "status": 400,
  "detail": "Request validation failed",
  "instance": "/v1/users",
  "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
  "context": {
    "field_violations": [
      {
        "field": "email",
        "description": "is required",
        "reason": "REQUIRED"
      },
      {
        "field": "name",
        "description": "must be at least 2 characters",
        "reason": "TOO_SHORT"
      }
    ],
    "resource_type": "gts.cf.core.users.user.v1"
  }
}
```

---

## Nested Field Paths

The `field` value in `FieldViolation` uses XPath-style dot-separated paths with bracket notation for array indices. This format is a **contract restriction** — all field references MUST use this syntax. Extensions to the path syntax (e.g., wildcards, predicates) require a design change.

```rust
CanonicalError::invalid_argument(
Validation::fields([
FieldViolation::new("address.zip_code", "must be 5 digits", "INVALID_FORMAT"),
FieldViolation::new("contacts[0].phone", "must include country code", "INVALID_FORMAT"),
])
)
```

```json
{
  "context": {
    "field_violations": [
      {
        "field": "address.zip_code",
        "description": "must be 5 digits",
        "reason": "INVALID_FORMAT"
      },
      {
        "field": "contacts[0].phone",
        "description": "must include country code",
        "reason": "INVALID_FORMAT"
      }
    ]
  }
}
```
