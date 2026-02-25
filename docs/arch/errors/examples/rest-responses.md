# REST Response Examples — All 16 Canonical Categories

Every REST error response uses `Content-Type: application/problem+json` (RFC 9457). This document shows the complete cycle for each canonical category: resource error declaration,
user code, and wire output.

## How to Read Each Example

Each category follows three steps:

1. **Declaration** — the `#[resource_error]` macro, declared once per resource type in the module
2. **User Code** — how domain code constructs the error (one-liner at the call site)
3. **Output** — the RFC 9457 Problem JSON produced by `From<CanonicalError> for Problem`

Categories that are typically not scoped to a specific resource (e.g., `unauthenticated`, `service_unavailable`) use direct `CanonicalError::` constructors instead of a resource
error
type.

---

## 1. invalid_argument

**HTTP 400** | **Context**: `Validation` | **GTS**: `gts.cf.core.errors.err.v1~cf.core.errors.invalid_argument.v1~`

Input is wrong regardless of system state — malformed fields, bad format, or violated constraints.

### Declaration

```rust
#[resource_error("gts.cf.core.users.user.v1")]
struct UserResourceError;
```

### User Code — Field Violations

```rust
UserResourceError::invalid_argument(Validation::fields([
FieldViolation::new("email", "must be a valid email address", "INVALID_FORMAT"),
FieldViolation::new("name", "must be between 2 and 100 characters", "OUT_OF_RANGE"),
]))
```

### Output

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
        "description": "must be a valid email address",
        "reason": "INVALID_FORMAT"
      },
      {
        "field": "name",
        "description": "must be between 2 and 100 characters",
        "reason": "OUT_OF_RANGE"
      }
    ],
    "resource_type": "gts.cf.core.users.user.v1"
  }
}
```

> **Validation variants**: `Validation` is an enum with three variants. `Validation::fields([...])` for field-level violations (shown above). `Validation::format("...")` for
> malformed request bodies (e.g., invalid JSON). `Validation::constraint("...")` for general constraint violations that aren't field-specific. Each variant produces a different
`context` shape — consumers use the `type` field to determine the schema.

<details>
<summary>Variant: Format (malformed request body, no resource scope)</summary>

```rust
CanonicalError::invalid_argument(Validation::format("request body is not valid JSON"))
```

```json
{
  "type": "gts.cf.core.errors.err.v1~cf.core.errors.invalid_argument.v1~",
  "title": "Invalid Argument",
  "status": 400,
  "detail": "request body is not valid JSON",
  "instance": "/v1/users",
  "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
  "context": {
    "format": "request body is not valid JSON"
  }
}
```

</details>

<details>
<summary>Variant: Constraint (general constraint, no resource scope)</summary>

```rust
CanonicalError::invalid_argument(Validation::constraint("bulk import cannot exceed 1000 items"))
```

```json
{
  "type": "gts.cf.core.errors.err.v1~cf.core.errors.invalid_argument.v1~",
  "title": "Invalid Argument",
  "status": 400,
  "detail": "bulk import cannot exceed 1000 items",
  "instance": "/v1/users:import",
  "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
  "context": {
    "constraint": "bulk import cannot exceed 1000 items"
  }
}
```

</details>

---

## 2. not_found

**HTTP 404** | **Context**: `ResourceInfo` | **GTS**: `gts.cf.core.errors.err.v1~cf.core.errors.not_found.v1~`

A specific requested resource does not exist.

### Declaration

```rust
#[resource_error("gts.cf.oagw.upstreams.upstream.v1")]
struct UpstreamResourceError;
```

### User Code

```rust
UpstreamResourceError::not_found(upstream_id)
```

### Output

```http
HTTP/1.1 404 Not Found
Content-Type: application/problem+json

{
  "type": "gts.cf.core.errors.err.v1~cf.core.errors.not_found.v1~",
  "title": "Not Found",
  "status": 404,
  "detail": "Resource not found",
  "instance": "/v1/upstreams/01JUPS-MISSING",
  "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
  "context": {
    "resource_type": "gts.cf.oagw.upstreams.upstream.v1",
    "resource_name": "01JUPS-MISSING",
    "description": "Resource not found"
  }
}
```

---

## 3. already_exists

**HTTP 409** | **Context**: `ResourceInfo` | **GTS**: `gts.cf.core.errors.err.v1~cf.core.errors.already_exists.v1~`

Client tried to create a resource that already exists.

### Declaration

```rust
#[resource_error("gts.cf.core.users.user.v1")]
struct UserResourceError;
```

### User Code

```rust
UserResourceError::already_exists("alice@example.com")
```

> The macro hardcodes `description` to `"Resource already exists"`. To customize the `detail` field in the Problem output, chain `.with_message("custom detail text")`.

### Output

```http
HTTP/1.1 409 Conflict
Content-Type: application/problem+json

{
  "type": "gts.cf.core.errors.err.v1~cf.core.errors.already_exists.v1~",
  "title": "Already Exists",
  "status": 409,
  "detail": "Resource already exists",
  "instance": "/v1/users",
  "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
  "context": {
    "resource_type": "gts.cf.core.users.user.v1",
    "resource_name": "alice@example.com",
    "description": "Resource already exists"
  }
}
```

---

## 4. permission_denied

**HTTP 403** | **Context**: `ErrorInfo` | **GTS**: `gts.cf.core.errors.err.v1~cf.core.errors.permission_denied.v1~`

Caller is authenticated but lacks permission for this operation.

### Declaration

```rust
#[resource_error("gts.cf.core.tenants.tenant.v1")]
struct TenantResourceError;
```

### User Code

```rust
TenantResourceError::permission_denied(
ErrorInfo::new("CROSS_TENANT_ACCESS", "auth.cyberfabric.io")
)
```

### Output

```http
HTTP/1.1 403 Forbidden
Content-Type: application/problem+json

{
  "type": "gts.cf.core.errors.err.v1~cf.core.errors.permission_denied.v1~",
  "title": "Permission Denied",
  "status": 403,
  "detail": "You do not have permission to perform this operation",
  "instance": "/v1/tenants/01JTNT-OTHER/users",
  "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
  "context": {
    "reason": "CROSS_TENANT_ACCESS",
    "domain": "auth.cyberfabric.io",
    "metadata": {},
    "resource_type": "gts.cf.core.tenants.tenant.v1"
  }
}
```

---

## 5. unauthenticated

**HTTP 401** | **Context**: `ErrorInfo` | **GTS**: `gts.cf.core.errors.err.v1~cf.core.errors.unauthenticated.v1~`

Request has no valid authentication credentials. Not scoped to a resource — uses `CanonicalError` directly.

### User Code

```rust
CanonicalError::unauthenticated(
ErrorInfo::new("TOKEN_EXPIRED", "auth.cyberfabric.io")
.with_metadata("expires_at", "2026-02-25T10:00:00Z")
)
```

### Output

```http
HTTP/1.1 401 Unauthorized
Content-Type: application/problem+json

{
  "type": "gts.cf.core.errors.err.v1~cf.core.errors.unauthenticated.v1~",
  "title": "Unauthenticated",
  "status": 401,
  "detail": "Authentication required",
  "instance": "/v1/users/me",
  "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
  "context": {
    "reason": "TOKEN_EXPIRED",
    "domain": "auth.cyberfabric.io",
    "metadata": {
      "expires_at": "2026-02-25T10:00:00Z"
    }
  }
}
```

---

## 6. resource_exhausted

**HTTP 429** | **Context**: `QuotaFailure` | **GTS**: `gts.cf.core.errors.err.v1~cf.core.errors.resource_exhausted.v1~`

Quota or rate limit exceeded. Typically system-level — uses `CanonicalError` directly.

### User Code

```rust
CanonicalError::resource_exhausted(QuotaFailure::new([
QuotaViolation::new(
"requests_per_minute",
"Limit of 100 requests per minute exceeded",
),
]))
```

### Output

```http
HTTP/1.1 429 Too Many Requests
Content-Type: application/problem+json

{
  "type": "gts.cf.core.errors.err.v1~cf.core.errors.resource_exhausted.v1~",
  "title": "Resource Exhausted",
  "status": 429,
  "detail": "Quota exceeded",
  "instance": "/v1/users",
  "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
  "context": {
    "violations": [
      {
        "subject": "requests_per_minute",
        "description": "Limit of 100 requests per minute exceeded"
      }
    ]
  }
}
```

---

## 7. failed_precondition

**HTTP 400** | **Context**: `PreconditionFailure` | **GTS**: `gts.cf.core.errors.err.v1~cf.core.errors.failed_precondition.v1~`

System is not in the required state — the input is valid but the operation cannot proceed. Differs from `invalid_argument` (input is wrong regardless of state) and from `aborted` (
transient concurrency issue).

### Declaration

```rust
#[resource_error("gts.cf.core.tenants.tenant.v1")]
struct TenantResourceError;
```

### User Code

```rust
TenantResourceError::failed_precondition(PreconditionFailure::new([
PreconditionViolation::new(
"STATE",
"tenant.users",
"Tenant must have zero active users before deletion",
),
]))
```

### Output

```http
HTTP/1.1 400 Bad Request
Content-Type: application/problem+json

{
  "type": "gts.cf.core.errors.err.v1~cf.core.errors.failed_precondition.v1~",
  "title": "Failed Precondition",
  "status": 400,
  "detail": "Operation precondition not met",
  "instance": "/v1/tenants/01JTNT-ABC",
  "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
  "context": {
    "violations": [
      {
        "type": "STATE",
        "subject": "tenant.users",
        "description": "Tenant must have zero active users before deletion"
      }
    ],
    "resource_type": "gts.cf.core.tenants.tenant.v1"
  }
}
```

---

## 8. aborted

**HTTP 409** | **Context**: `ErrorInfo` | **GTS**: `gts.cf.core.errors.err.v1~cf.core.errors.aborted.v1~`

Operation aborted due to a transient concurrency conflict. Client should re-read and retry.

### Declaration

```rust
#[resource_error("gts.cf.oagw.upstreams.upstream.v1")]
struct UpstreamResourceError;
```

### User Code

```rust
UpstreamResourceError::aborted(
ErrorInfo::new("OPTIMISTIC_LOCK_FAILURE", "cf.oagw")
.with_metadata("expected_version", "3")
.with_metadata("actual_version", "5")
)
```

### Output

```http
HTTP/1.1 409 Conflict
Content-Type: application/problem+json

{
  "type": "gts.cf.core.errors.err.v1~cf.core.errors.aborted.v1~",
  "title": "Aborted",
  "status": 409,
  "detail": "Operation aborted due to concurrency conflict",
  "instance": "/v1/upstreams/01JUPS-ABC",
  "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
  "context": {
    "reason": "OPTIMISTIC_LOCK_FAILURE",
    "domain": "cf.oagw",
    "metadata": {
      "expected_version": "3",
      "actual_version": "5"
    },
    "resource_type": "gts.cf.oagw.upstreams.upstream.v1"
  }
}
```

---

## 9. out_of_range

**HTTP 400** | **Context**: `Validation` | **GTS**: `gts.cf.core.errors.err.v1~cf.core.errors.out_of_range.v1~`

Value is syntactically valid but outside the accepted range. Differs from `invalid_argument` (structurally wrong).

### Declaration

```rust
#[resource_error("gts.cf.core.users.user.v1")]
struct UserResourceError;
```

### User Code

```rust
UserResourceError::out_of_range(
Validation::constraint("Page 50 is beyond the last page (12)")
)
```

### Output

```http
HTTP/1.1 400 Bad Request
Content-Type: application/problem+json

{
  "type": "gts.cf.core.errors.err.v1~cf.core.errors.out_of_range.v1~",
  "title": "Out of Range",
  "status": 400,
  "detail": "Page 50 is beyond the last page (12)",
  "instance": "/v1/users?page=50",
  "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
  "context": {
    "constraint": "Page 50 is beyond the last page (12)",
    "resource_type": "gts.cf.core.users.user.v1"
  }
}
```

---

## 10. unimplemented

**HTTP 501** | **Context**: `ErrorInfo` | **GTS**: `gts.cf.core.errors.err.v1~cf.core.errors.unimplemented.v1~`

Operation is not supported or not yet implemented.

### Declaration

```rust
#[resource_error("gts.cf.oagw.routes.route.v1")]
struct RouteResourceError;
```

### User Code

```rust
RouteResourceError::unimplemented(
ErrorInfo::new("GRPC_ROUTING", "cf.oagw")
)
```

### Output

```http
HTTP/1.1 501 Not Implemented
Content-Type: application/problem+json

{
  "type": "gts.cf.core.errors.err.v1~cf.core.errors.unimplemented.v1~",
  "title": "Unimplemented",
  "status": 501,
  "detail": "This operation is not implemented",
  "instance": "/v1/routes/01JRTE-ABC/grpc-config",
  "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
  "context": {
    "reason": "GRPC_ROUTING",
    "domain": "cf.oagw",
    "metadata": {},
    "resource_type": "gts.cf.oagw.routes.route.v1"
  }
}
```

---

## 11. internal

**HTTP 500** | **Context**: `DebugInfo` | **GTS**: `gts.cf.core.errors.err.v1~cf.core.errors.internal.v1~`

Unexpected server error — bugs, invariant violations, or library errors absorbed via `?`. In production, details are stripped.
See [edge-cases.md](./edge-cases.md#production-vs-debug-mode) for the debug mode variant with stack traces.

### Declaration

```rust
#[resource_error("gts.cf.core.users.user.v1")]
struct UserResourceError;
```

### User Code

```rust
UserResourceError::internal(DebugInfo::new("An internal error occurred. Please retry later."))
```

> Library errors (sqlx, serde_json, etc.) also convert via the `?` operator using blanket `From` impls — see [edge-cases.md](./edge-cases.md#library-error-absorption). The `?` path
> produces `CanonicalError::Internal` without `resource_type`.

### Output

```http
HTTP/1.1 500 Internal Server Error
Content-Type: application/problem+json

{
  "type": "gts.cf.core.errors.err.v1~cf.core.errors.internal.v1~",
  "title": "Internal",
  "status": 500,
  "detail": "An internal error occurred. Please retry later.",
  "instance": "/v1/users/01JUSR-ABC",
  "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
  "context": {
    "detail": "An internal error occurred. Please retry later.",
    "stack_entries": [],
    "resource_type": "gts.cf.core.users.user.v1"
  }
}
```

---

## 12. unknown

**HTTP 500** | **Context**: `DebugInfo` | **GTS**: `gts.cf.core.errors.err.v1~cf.core.errors.unknown.v1~`

Error from an unknown source where the category cannot be determined. Differs from `internal` (server's own bugs) — `unknown` is for opaque external errors.

### User Code

```rust
CanonicalError::unknown("Unexpected response from payment provider")
```

### Output

```http
HTTP/1.1 500 Internal Server Error
Content-Type: application/problem+json

{
  "type": "gts.cf.core.errors.err.v1~cf.core.errors.unknown.v1~",
  "title": "Unknown",
  "status": 500,
  "detail": "Unexpected response from payment provider",
  "instance": "/v1/integrations/payments/sync",
  "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
  "context": {
    "detail": "Unexpected response from payment provider",
    "stack_entries": []
  }
}
```

---

## 13. service_unavailable

**HTTP 503** | **Context**: `RetryInfo` | **GTS**: `gts.cf.core.errors.err.v1~cf.core.errors.service_unavailable.v1~`

Service temporarily unavailable — client should retry. Not scoped to a resource.

### User Code

```rust
CanonicalError::service_unavailable(RetryInfo::after_seconds(30))
```

### Output

```http
HTTP/1.1 503 Service Unavailable
Content-Type: application/problem+json
Retry-After: 30

{
  "type": "gts.cf.core.errors.err.v1~cf.core.errors.service_unavailable.v1~",
  "title": "Unavailable",
  "status": 503,
  "detail": "Service temporarily unavailable",
  "instance": "/v1/users",
  "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
  "context": {
    "retry_after_seconds": 30
  }
}
```

---

## 14. deadline_exceeded

**HTTP 504** | **Context**: `String` (message) | **GTS**: `gts.cf.core.errors.err.v1~cf.core.errors.deadline_exceeded.v1~`

Operation did not complete within the allowed time.

### Declaration

```rust
#[resource_error("gts.cf.core.reports.report.v1")]
struct ReportResourceError;
```

### User Code

```rust
ReportResourceError::deadline_exceeded("Report generation exceeded 30s timeout")
```

### Output

```http
HTTP/1.1 504 Gateway Timeout
Content-Type: application/problem+json

{
  "type": "gts.cf.core.errors.err.v1~cf.core.errors.deadline_exceeded.v1~",
  "title": "Deadline Exceeded",
  "status": 504,
  "detail": "Report generation exceeded 30s timeout",
  "instance": "/v1/reports/generate",
  "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
  "context": {
    "resource_type": "gts.cf.core.reports.report.v1"
  }
}
```

---

## 15. cancelled

**HTTP 499** | **Context**: `String` (message) | **GTS**: `gts.cf.core.errors.err.v1~cf.core.errors.cancelled.v1~`

Operation cancelled by the caller (client disconnected).

### Declaration

```rust
#[resource_error("gts.cf.core.reports.report.v1")]
struct ReportResourceError;
```

### User Code

```rust
ReportResourceError::cancelled("Client disconnected during report generation")
```

### Output

```http
HTTP/1.1 499 Client Closed Request
Content-Type: application/problem+json

{
  "type": "gts.cf.core.errors.err.v1~cf.core.errors.cancelled.v1~",
  "title": "Cancelled",
  "status": 499,
  "detail": "Client disconnected during report generation",
  "instance": "/v1/reports/generate",
  "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
  "context": {
    "resource_type": "gts.cf.core.reports.report.v1"
  }
}
```

---

## 16. data_loss

**HTTP 500** | **Context**: `ResourceInfo` | **GTS**: `gts.cf.core.errors.err.v1~cf.core.errors.data_loss.v1~`

Unrecoverable data loss or corruption detected.

### Declaration

```rust
#[resource_error("gts.cf.core.files.file.v1")]
struct FileResourceError;
```

### User Code

```rust
FileResourceError::data_loss("01JFILE-ABC")
```

> The macro hardcodes `description` to `"Data loss detected"`. To customize the `detail` field in the Problem output, chain `.with_message("custom detail text")`.

### Output

```http
HTTP/1.1 500 Internal Server Error
Content-Type: application/problem+json

{
  "type": "gts.cf.core.errors.err.v1~cf.core.errors.data_loss.v1~",
  "title": "Data Loss",
  "status": 500,
  "detail": "Data loss detected",
  "instance": "/v1/files/01JFILE-ABC",
  "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
  "context": {
    "resource_type": "gts.cf.core.files.file.v1",
    "resource_name": "01JFILE-ABC",
    "description": "Data loss detected"
  }
}
```

---

## Quick Reference

All resource error types declared in these examples:

```rust
#[resource_error("gts.cf.core.users.user.v1")]
struct UserResourceError;

#[resource_error("gts.cf.core.tenants.tenant.v1")]
struct TenantResourceError;

#[resource_error("gts.cf.core.files.file.v1")]
struct FileResourceError;

#[resource_error("gts.cf.core.reports.report.v1")]
struct ReportResourceError;

#[resource_error("gts.cf.oagw.upstreams.upstream.v1")]
struct UpstreamResourceError;

#[resource_error("gts.cf.oagw.routes.route.v1")]
struct RouteResourceError;
```

Summary of which categories use resource-scoped vs direct constructors:

| Pattern                           | Categories                                                                                                                                                                                                                                               | Why                                                                              |
|-----------------------------------|----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|----------------------------------------------------------------------------------|
| `XxxResourceError::category(...)` | `invalid_argument`, `not_found`, `already_exists`, `permission_denied`, `failed_precondition`, `aborted`, `out_of_range`, `unimplemented`, `internal`, `deadline_exceeded`, `cancelled`, `data_loss`, `unauthenticated`, `resource_exhausted`, `unknown` | Error may be about a specific resource                                           |
| `CanonicalError::category(...)`   | `service_unavailable`                                                                                                                                                                                                                                    | System-level only; any category can also be used directly without resource scope |

> Most categories can be used both ways: via the `XxxResourceError::category(...)` macro (which injects `resource_type` into the `context`) or via `CanonicalError::category(...)`
> directly (no `resource_type`). Only `service_unavailable` is exclusively direct — it has no resource-scoped variant. Consumers route on `type` (GTS error category) and can
> optionally use `resource_type` inside `context` to identify the affected resource.
