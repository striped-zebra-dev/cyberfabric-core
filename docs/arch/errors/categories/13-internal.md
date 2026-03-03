# 13 Internal

**Category**: `internal`
**GTS ID**: `gts.cf.core.errors.err.v1~cf.core.err.internal.v1~`
**HTTP Status**: 500
**Title**: "Internal"
**Context Type**: `DebugInfo`
**Use When**: A known infrastructure failure occurred (database error, serialization bug, etc.). The detail in production is generic; diagnostics are in logs via `trace_id`.
**Similar Categories**: `unknown` — truly unknown error vs known infrastructure failure
**Default Message**: "An internal error occurred. Please retry later."

## Context Schema

GTS schema ID: `gts.cf.core.errors.debug_info.v1~`

| Field | Type | Description |
|-------|------|-------------|
| `detail` | `String` | Human-readable debug message (generic in production) |
| `stack_entries` | `Vec<String>` | Stack trace entries (empty in production) |
| `details` | `Option<Object>` | Reserved for derived GTS type extensions (p3+); absent in p1 |

> Note: `resource_type` is not part of the `DebugInfo` GTS type (`gts.cf.core.errors.debug_info.v1~`). It is an optional envelope field on `CanonicalError::Internal` and is injected alongside `detail` and `stack_entries` into the wire `context` object during mapping to `Problem` via `Problem::from_error`.

## Rust Definitions and Constructor Example

```rust
CanonicalError::Internal {
    ctx: DebugInfo,
    message: String,
    resource_type: Option<String>,
    debug_info: Option<DebugInfo>,
}

use cf_modkit_errors::{CanonicalError, DebugInfo};

// From a database error via ? operator:
let user = db.find_user(&id).await?;  // DbErr auto-converts to CanonicalError::Internal

// Or explicit construction:
let err = CanonicalError::internal(
    DebugInfo::new("Database connection pool exhausted")
);
```

## JSON Wire — JSON Schema

```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "$id": "gts://gts.cf.core.errors.err.v1~cf.core.err.internal.v1~",
  "type": "object",
  "allOf": [
    { "$ref": "gts://gts.cf.core.errors.err.v1~" },
    {
      "properties": {
        "type": {
          "const": "gts.cf.core.errors.err.v1~cf.core.err.internal.v1~"
        },
        "title": { "const": "Internal" },
        "status": { "const": 500 },
        "context": {
          "type": "object",
          "required": ["detail", "stack_entries"],
          "properties": {
            "resource_type": {
              "type": "string",
              "description": "GTS type identifier of the associated resource (injected when resource_type is set)"
            },
            "detail": {
              "type": "string",
              "description": "Human-readable debug message (generic in production)"
            },
            "stack_entries": {
              "type": "array",
              "items": { "type": "string" },
              "description": "Stack trace entries (empty in production)"
            },
            "details": {
              "type": ["object", "null"],
              "description": "Reserved for derived GTS type extensions (p3+); absent in p1"
            }
          },
          "additionalProperties": false
        }
      }
    }
  ]
}
```

## JSON Wire — JSON Example

```json
{
  "type": "gts.cf.core.errors.err.v1~cf.core.err.internal.v1~",
  "title": "Internal",
  "status": 500,
  "detail": "An internal error occurred. Please retry later.",
  "context": {
    "resource_type": "gts.cf.core.tenants.tenant.v1~",
    "detail": "An internal error occurred. Please retry later.",
    "stack_entries": []
  }
}
```
