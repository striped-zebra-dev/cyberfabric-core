# Batch Composition — Canonical Errors in Batch Responses

Canonical errors compose naturally with the batch pattern defined in [guidelines/DNA/REST/BATCH.md](../../../../guidelines/DNA/REST/BATCH.md). Each failed item in a batch response
carries a full RFC 9457 Problem produced from a `CanonicalError`.

> **Key point**: The canonical error model defines what a single error looks like. The batch envelope is a transport-layer concern. No changes to `CanonicalError` are needed for
> batch support.

---

## Partial Success — Mixed Categories

**Scenario**: Batch-creating three users. One succeeds, one has a duplicate email, one has invalid fields.

```http
POST /v1/users:batch HTTP/1.1
Content-Type: application/json
```

```json
{
  "items": [
    {
      "idempotency_key": "req-1",
      "data": { "email": "alice@example.com", "name": "Alice" }
    },
    {
      "idempotency_key": "req-2",
      "data": { "email": "bob@example.com", "name": "Bob" }
    },
    {
      "idempotency_key": "req-3",
      "data": { "email": "not-an-email", "name": "" }
    }
  ]
}
```

```http
HTTP/1.1 207 Multi-Status
Content-Type: application/json
```

```json
{
  "items": [
    {
      "index": 0,
      "idempotency_key": "req-1",
      "status": 201,
      "location": "/v1/users/01JUSR-ALICE",
      "etag": "W/\"a1b2c3\"",
      "data": {
        "id": "01JUSR-ALICE",
        "email": "alice@example.com",
        "name": "Alice"
      }
    },
    {
      "index": 1,
      "idempotency_key": "req-2",
      "status": 409,
      "error": {
        "type": "gts.cf.core.errors.err.v1~cf.core.errors.already_exists.v1~",
        "title": "Already Exists",
        "status": 409,
        "detail": "Resource already exists",
        "instance": "/v1/users:batch#item-1",
        "trace_id": "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6",
        "context": {
          "resource_type": "gts.cf.core.users.user.v1",
          "resource_name": "bob@example.com",
          "description": "Resource already exists"
        }
      }
    },
    {
      "index": 2,
      "idempotency_key": "req-3",
      "status": 400,
      "error": {
        "type": "gts.cf.core.errors.err.v1~cf.core.errors.invalid_argument.v1~",
        "title": "Invalid Argument",
        "status": 400,
        "detail": "Request validation failed",
        "instance": "/v1/users:batch#item-2",
        "trace_id": "b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7",
        "context": {
          "field_violations": [
            {
              "field": "email",
              "description": "must be a valid email address",
              "reason": "INVALID_FORMAT"
            },
            {
              "field": "name",
              "description": "is required",
              "reason": "REQUIRED"
            }
          ],
          "resource_type": "gts.cf.core.users.user.v1"
        }
      }
    }
  ]
}
```

**What is happening**:

- **Item 0**: Succeeded → `201 Created` with resource data
- **Item 1**: `CanonicalError::AlreadyExists(ResourceInfo)` → per-item `409` with full Problem
- **Item 2**: `CanonicalError::InvalidArgument(Validation::FieldViolations)` → per-item `400` with field violations
- **Top-level**: `207 Multi-Status` because outcomes are mixed

---

## All Failed — Same Category

When every item fails with the same HTTP status, the top-level status matches.

**Scenario**: Batch-updating two upstreams, both have version conflicts.

```http
HTTP/1.1 409 Conflict
Content-Type: application/json
```

```json
{
  "items": [
    {
      "index": 0,
      "status": 409,
      "error": {
        "type": "gts.cf.core.errors.err.v1~cf.core.errors.aborted.v1~",
        "title": "Aborted",
        "status": 409,
        "detail": "Operation aborted due to concurrency conflict",
        "instance": "/v1/upstreams:batch#item-0",
        "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
        "context": {
          "resource_type": "gts.cf.oagw.upstreams.upstream.v1",
          "reason": "OPTIMISTIC_LOCK_FAILURE",
          "domain": "cf.oagw",
          "metadata": {
            "expected_version": "2",
            "actual_version": "4"
          }
        }
      }
    },
    {
      "index": 1,
      "status": 409,
      "error": {
        "type": "gts.cf.core.errors.err.v1~cf.core.errors.aborted.v1~",
        "title": "Aborted",
        "status": 409,
        "detail": "Operation aborted due to concurrency conflict",
        "instance": "/v1/upstreams:batch#item-1",
        "trace_id": "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6",
        "context": {
          "resource_type": "gts.cf.oagw.upstreams.upstream.v1",
          "reason": "OPTIMISTIC_LOCK_FAILURE",
          "domain": "cf.oagw",
          "metadata": {
            "expected_version": "1",
            "actual_version": "3"
          }
        }
      }
    }
  ]
}
```

---

## All Failed — Mixed Categories

When all items fail but with different HTTP statuses, the top-level is still `207`.

**Scenario**: Batch-deleting resources — one not found, one permission denied.

```http
HTTP/1.1 207 Multi-Status
Content-Type: application/json
```

```json
{
  "items": [
    {
      "index": 0,
      "status": 404,
      "error": {
        "type": "gts.cf.core.errors.err.v1~cf.core.errors.not_found.v1~",
        "title": "Not Found",
        "status": 404,
        "detail": "Resource not found",
        "instance": "/v1/users:batch#item-0",
        "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
        "context": {
          "resource_type": "gts.cf.core.users.user.v1",
          "resource_name": "01JUSR-MISSING",
          "description": "Resource not found"
        }
      }
    },
    {
      "index": 1,
      "status": 403,
      "error": {
        "type": "gts.cf.core.errors.err.v1~cf.core.errors.permission_denied.v1~",
        "title": "Permission Denied",
        "status": 403,
        "detail": "You do not have permission to perform this operation",
        "instance": "/v1/users:batch#item-1",
        "trace_id": "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6",
        "context": {
          "resource_type": "gts.cf.core.users.user.v1",
          "reason": "CROSS_TENANT_ACCESS",
          "domain": "auth.cyberfabric.io",
          "metadata": { }
        }
      }
    }
  ]
}
```

---

## Status Code Summary

Reference from [guidelines/DNA/REST/BATCH.md](../../../../guidelines/DNA/REST/BATCH.md):

| Batch Outcome            | Top-Level HTTP Status       |
|--------------------------|-----------------------------|
| All succeeded            | `200 OK` (or `201 Created`) |
| Partial success/failure  | `207 Multi-Status`          |
| All failed (same status) | Matching `4xx`              |
| All failed (mixed)       | `207 Multi-Status`          |
