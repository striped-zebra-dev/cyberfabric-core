# 09 Failed Precondition

**Category**: `failed_precondition`
**GTS ID**: `gts.cf.core.errors.err.v1~cf.core.err.failed_precondition.v1~`
**HTTP Status**: 400
**Title**: "Failed Precondition"
**Context Type**: `PreconditionFailure`
**Use When**: The request is valid but the system is not in the required state to perform it (e.g., deleting a non-empty directory, operating on a resource in the wrong lifecycle state).
**Similar Categories**: `invalid_argument` — request itself is bad vs system state prevents it
**Default Message**: "Operation precondition not met"

## Context Schema

GTS schema ID: `gts.cf.core.errors.precondition_failure.v1~`

| Field | Type | Description |
|-------|------|-------------|
| `violations` | `Vec<PreconditionViolation>` | List of precondition violations |
| `details` | `Option<Object>` | Reserved for derived GTS type extensions (p3+); absent in p1 |

Each `PreconditionViolation` (GTS schema ID: `gts.cf.core.errors.precondition_violation.v1~`):

| Field | Type | Description |
|-------|------|-------------|
| `type` | `String` | Precondition category (`STATE`, `TOS`, `VERSION`) |
| `subject` | `String` | What failed the check |
| `description` | `String` | How to resolve the failure |

## Rust Definitions and Constructor Example

```rust
CanonicalError::FailedPrecondition {
    ctx: PreconditionFailure,
    message: String,
    resource_type: Option<String>,
    debug_info: Option<DebugInfo>,
}

use cf_modkit_errors::{CanonicalError, PreconditionFailure, PreconditionViolation};

let err = CanonicalError::failed_precondition(
    PreconditionFailure {
        violations: vec![
            PreconditionViolation {
                type_: "STATE".to_string(),
                subject: "tenant.users".to_string(),
                description: "Tenant must have zero active users before deletion".to_string(),
            }
        ]
    }
);
```

## JSON Wire — JSON Schema

```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "$id": "gts://gts.cf.core.errors.err.v1~cf.core.err.failed_precondition.v1~",
  "type": "object",
  "allOf": [
    { "$ref": "gts://gts.cf.core.errors.err.v1~" },
    {
      "properties": {
        "type": {
          "const": "gts.cf.core.errors.err.v1~cf.core.err.failed_precondition.v1~"
        },
        "title": { "const": "Failed Precondition" },
        "status": { "const": 400 },
        "context": {
          "type": "object",
          "required": ["violations"],
          "properties": {
            "resource_type": {
              "type": "string",
              "description": "GTS type identifier of the associated resource (injected when resource_type is set)"
            },
            "violations": {
              "type": "array",
              "items": { "$ref": "#/$defs/PreconditionViolation" }
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
  ],
  "$defs": {
    "PreconditionViolation": {
      "$id": "gts://gts.cf.core.errors.precondition_violation.v1~",
      "type": "object",
      "required": ["type", "subject", "description"],
      "properties": {
        "type": { "type": "string", "description": "Precondition category (STATE, TOS, VERSION)" },
        "subject": { "type": "string", "description": "What failed the check" },
        "description": { "type": "string", "description": "How to resolve the failure" }
      },
      "additionalProperties": false
    }
  }
}
```

## JSON Wire — JSON Example

```json
{
  "type": "gts.cf.core.errors.err.v1~cf.core.err.failed_precondition.v1~",
  "title": "Failed Precondition",
  "status": 400,
  "detail": "Operation precondition not met",
  "context": {
    "resource_type": "gts.cf.core.tenants.tenant.v1~",
    "violations": [
      {
        "type": "STATE",
        "subject": "tenant.users",
        "description": "Tenant must have zero active users before deletion"
      }
    ]
  }
}
```
