# Contract vs Variable Parts of an Error Response

Every error response has two distinct parts:

- **Contract** (C) — fixed structure that consumers depend on. Changing any contract field is a **breaking change**. Determined by the **category**, not by the occurrence.
- **Variable** (V) — values filled in by module code per occurrence. These change with every request and are **not part of the contract**.

---

## Field Classification

Which Problem fields are contract, which are variable, and which depend on scope:

| Field            | Part         | Resource-scoped        | Non-resource           | Who provides it                        |
|------------------|--------------|------------------------|------------------------|----------------------------------------|
| `type`           | **Contract** | GTS error category URI | GTS error category URI | Framework (from category)              |
| `title`          | **Contract** | Fixed per category     | Fixed per category     | Framework (from category)              |
| `status`         | **Contract** | Fixed per category     | Fixed per category     | Framework (from category)              |
| `context` schema | **Contract** | Fixed per category     | Fixed per category     | Framework (from category)              |
| `context` values | Variable     | Per-occurrence         | Per-occurrence         | Module code                            |
| `detail`         | Variable     | Per-occurrence         | Per-occurrence         | Module code (error message)            |
| `instance`       | Variable     | Per-occurrence         | Per-occurrence         | Framework (request URI)                |
| `trace_id`       | Variable     | Per-occurrence         | Per-occurrence         | Middleware (from span)                 |
| `debug`          | Variable     | Debug mode only        | Debug mode only        | Module code (via `.with_debug_info()`) |

> The `debug` field is present only when the application runs in debug mode **and** the error has debug info attached. Consumers MUST NOT depend on its presence, absence, or
> contents.

> For resource-scoped errors (constructed via `XxxResourceError::category(...)`), `resource_type` is injected into the `context` object rather than appearing as a top-level field.

---

## Complete Response Shape by Category

One row per category. **C** = Contract (breaking if changed), **V** = Variable (per-occurrence). Read any row to know the full response shape.

| #  | Category              | `type` (C)                                                         | `title` (C)         | `status` (C) | Context Type (C)      | `resource_type` (V)                 |
|----|-----------------------|--------------------------------------------------------------------|---------------------|--------------|-----------------------|-------------------------------------|
| 1  | `invalid_argument`    | `gts.cf.core.errors.err.v1~cf.core.errors.invalid_argument.v1~`    | Invalid Argument    | 400          | `Validation`          | `gts.cf.core.users.user.v1`         |
| 2  | `not_found`           | `gts.cf.core.errors.err.v1~cf.core.errors.not_found.v1~`           | Not Found           | 404          | `ResourceInfo`        | `gts.cf.oagw.upstreams.upstream.v1` |
| 3  | `already_exists`      | `gts.cf.core.errors.err.v1~cf.core.errors.already_exists.v1~`      | Already Exists      | 409          | `ResourceInfo`        | `gts.cf.core.users.user.v1`         |
| 4  | `permission_denied`   | `gts.cf.core.errors.err.v1~cf.core.errors.permission_denied.v1~`   | Permission Denied   | 403          | `ErrorInfo`           | `gts.cf.core.tenants.tenant.v1`     |
| 5  | `unauthenticated`     | `gts.cf.core.errors.err.v1~cf.core.errors.unauthenticated.v1~`     | Unauthenticated     | 401          | `ErrorInfo`           | —                                   |
| 6  | `resource_exhausted`  | `gts.cf.core.errors.err.v1~cf.core.errors.resource_exhausted.v1~`  | Resource Exhausted  | 429          | `QuotaFailure`        | —                                   |
| 7  | `failed_precondition` | `gts.cf.core.errors.err.v1~cf.core.errors.failed_precondition.v1~` | Failed Precondition | 400          | `PreconditionFailure` | `gts.cf.core.tenants.tenant.v1`     |
| 8  | `aborted`             | `gts.cf.core.errors.err.v1~cf.core.errors.aborted.v1~`             | Aborted             | 409          | `ErrorInfo`           | `gts.cf.oagw.upstreams.upstream.v1` |
| 9  | `out_of_range`        | `gts.cf.core.errors.err.v1~cf.core.errors.out_of_range.v1~`        | Out of Range        | 400          | `Validation`          | `gts.cf.core.users.user.v1`         |
| 10 | `unimplemented`       | `gts.cf.core.errors.err.v1~cf.core.errors.unimplemented.v1~`       | Unimplemented       | 501          | `ErrorInfo`           | `gts.cf.oagw.routes.route.v1`       |
| 11 | `internal`            | `gts.cf.core.errors.err.v1~cf.core.errors.internal.v1~`            | Internal            | 500          | `DebugInfo`           | `gts.cf.core.users.user.v1`         |
| 12 | `unknown`             | `gts.cf.core.errors.err.v1~cf.core.errors.unknown.v1~`             | Unknown             | 500          | `DebugInfo`           | —                                   |
| 13 | `service_unavailable` | `gts.cf.core.errors.err.v1~cf.core.errors.service_unavailable.v1~` | Unavailable         | 503          | `RetryInfo`           | —                                   |
| 14 | `deadline_exceeded`   | `gts.cf.core.errors.err.v1~cf.core.errors.deadline_exceeded.v1~`   | Deadline Exceeded   | 504          | `String` (message)    | `gts.cf.core.reports.report.v1`     |
| 15 | `cancelled`           | `gts.cf.core.errors.err.v1~cf.core.errors.cancelled.v1~`           | Cancelled           | 499          | `String` (message)    | `gts.cf.core.reports.report.v1`     |
| 16 | `data_loss`           | `gts.cf.core.errors.err.v1~cf.core.errors.data_loss.v1~`           | Data Loss           | 500          | `ResourceInfo`        | `gts.cf.core.files.file.v1`         |

> **How to read**: columns 3-6 are **contract** — they never change for a given category. The `resource_type` column is **variable** — the values shown are examples
> from [rest-responses.md](./rest-responses.md). Categories with **—** are typically system-level errors that use `CanonicalError::` directly (no resource scope). Any category
> *can* be resource-scoped or not, depending on whether you use `XxxResourceError::` or `CanonicalError::`.

Every row also has these **variable** fields (same for all categories, not shown in the table):

| Field              | Source      | Example                                                         |
|--------------------|-------------|-----------------------------------------------------------------|
| `detail`           | Module code | `"Resource already exists"`                                     |
| `instance`         | Framework   | `"/v1/users"`                                                   |
| `trace_id`         | Middleware  | `"4bf92f3577b34da6a3ce929d0e0e4736"`                            |
| `context.*` values | Module code | Varies per context type                                         |
| `debug`            | Module code | `{ "detail": "...", "stack_entries": [...] }` (debug mode only) |

---

## Context Schema (Contract)

The Context Type column above determines the `context` JSON shape. Full field types:

| Context Type              | Schema                                          | Fields                                                                               | In Categories                                                      |
|---------------------------|-------------------------------------------------|--------------------------------------------------------------------------------------|--------------------------------------------------------------------|
| **`Validation`**          | `{ field_violations[] }`                        | `field` `String`, `description` `String`, `reason` `String`                          | `invalid_argument`, `out_of_range`                                 |
|                           | `{ format }`                                    | `format` `String`                                                                    |                                                                    |
|                           | `{ constraint }`                                | `constraint` `String`                                                                |                                                                    |
| **`ResourceInfo`**        | `{ resource_type, resource_name, description }` | `resource_type` `String (GTS URI)`, `resource_name` `String`, `description` `String` | `not_found`, `already_exists`, `data_loss`                         |
| **`ErrorInfo`**           | `{ reason, domain, metadata }`                  | `reason` `String`, `domain` `String`, `metadata` `Map<String, String>`               | `permission_denied`, `aborted`, `unimplemented`, `unauthenticated` |
| **`QuotaFailure`**        | `{ violations[] }`                              | `subject` `String`, `description` `String`                                           | `resource_exhausted`                                               |
| **`PreconditionFailure`** | `{ violations[] }`                              | `type` `String`, `subject` `String`, `description` `String`                          | `failed_precondition`                                              |
| **`DebugInfo`**           | `{ detail, stack_entries[] }`                   | `detail` `String`, `stack_entries` `[String]`                                        | `internal`, `unknown`                                              |
| *(message only)*          | `String`                                        | —                                                                                    | `cancelled`, `deadline_exceeded`                                   |
| **`RetryInfo`**           | `{ retry_after_seconds }`                       | `retry_after_seconds` `u64`                                                          | `service_unavailable`                                              |

---

## Annotated Example

Putting it together for `already_exists` constructed via `UserResourceError::already_exists(...)`:

```json
{
  "type": "gts.cf.core.errors.err.v1~cf.core.errors.already_exists.v1~", // CONTRACT — fixed for already_exists
  "title": "Already Exists", // CONTRACT — fixed for already_exists
  "status": 409, // CONTRACT — fixed for already_exists
  "detail": "Resource already exists", // variable — per occurrence
  "instance": "/v1/users", // variable — request URI
  "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736", // variable — from span
  "context": {
    // CONTRACT schema, variable values
    "resource_type": "gts.cf.core.users.user.v1", //   variable value
    "resource_name": "alice@example.com", //   variable value
    "description": "Resource already exists" // variable value
  }
}
```

---

## What This Means for Change Protection

| Change                                                    | Breaking? | Why                                              |
|-----------------------------------------------------------|-----------|--------------------------------------------------|
| Rename `context.resource_name` to `context.name`          | **Yes**   | Contract: consumers parse by field name          |
| Remove `context.description` from `ResourceInfo`          | **Yes**   | Contract: consumers expect this field            |
| Change `status` from `409` to `400` for `already_exists`  | **Yes**   | Contract: consumers branch on status code        |
| Change the GTS identifier of a category                   | **Yes**   | Contract: consumers route on `type`              |
| Change the context type of a category                     | **Yes**   | Contract: consumers expect a specific schema     |
| Add `context.owner` optional field to `ResourceInfo`      | No        | Additive: consumers ignore unknown fields        |
| Add a new canonical category                              | No        | Additive: consumers ignore unknown `type` values |
| Change the `detail` message text                          | No        | Variable: consumers should not parse `detail`    |
| Change `context.resource_name` value from UUID to slug    | No        | Variable: values are not contractual             |
| Change `context.resource_type` value for a resource error | No        | Variable: resource identity is per-occurrence    |

> **Rule**: Consumers may depend on **contract** fields (`type`, `title`, `status`, context field names and types). Consumers must **not** depend on **variable** fields (`detail`
> text, specific context values, `instance` path, `context.resource_type`). This is the boundary between what the error architecture protects and what module code is free to
> change.
