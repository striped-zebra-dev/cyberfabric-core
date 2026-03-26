Created:  2026-03-06 by Constructor Tech
Updated:  2026-03-06 by Constructor Tech
# Feature: Message Delete


<!-- toc -->

- [1. Feature Context](#1-feature-context)
  - [1.1 Overview](#11-overview)
  - [1.2 Purpose](#12-purpose)
  - [1.3 Actors](#13-actors)
  - [1.4 References](#14-references)
- [2. Actor Flows (CDSL)](#2-actor-flows-cdsl)
  - [Delete Message](#delete-message)
- [3. Processes / Business Logic (CDSL)](#3-processes--business-logic-cdsl)
  - [Validate Message Ownership](#validate-message-ownership)
  - [Collect Subtree](#collect-subtree)
- [4. States (CDSL)](#4-states-cdsl)
  - [Message Deletion State](#message-deletion-state)
- [5. Definitions of Done](#5-definitions-of-done)
  - [Message Deletion](#message-deletion)
- [6. Acceptance Criteria](#6-acceptance-criteria)
- [7. Non-Functional Considerations](#7-non-functional-considerations)

<!-- /toc -->

- [ ] `p1` - **ID**: `cpt-cf-chat-engine-featstatus-message-delete`

## 1. Feature Context

- [ ] `p1` - `cpt-cf-chat-engine-feature-message-reactions`

**Traces to**: `cpt-cf-chat-engine-fr-delete-message` (FR-017), `cpt-cf-chat-engine-nfr-response-time` (NFR-001), `cpt-cf-chat-engine-nfr-authentication` (NFR-006), `cpt-cf-chat-engine-nfr-data-integrity` (NFR-007)

### 1.1 Overview

Targeted deletion of a message node and its full descendant subtree within a session. The operation validates session ownership via JWT identity, confirms the message belongs to that session, then performs a cascade delete of the message, all descendant messages in the subtree, and all associated reactions. Returns a structured deletion confirmation with timestamp.

### 1.2 Purpose

Allow clients to permanently remove a message and its full descendant subtree from a session. The endpoint is scoped under the session path to enforce ownership semantics at the routing layer: both the session and message IDs are validated against the requesting user's identity before any write occurs.

Success criteria: Message and all descendant messages are deleted with their reactions in a single atomic operation; unauthorized or non-existent resources return appropriate error codes without leaking ownership information.

### 1.3 Actors

| Actor | Role in Feature |
|-------|-----------------|
| `cpt-cf-chat-engine-actor-client` | Sends delete request scoped to a session; receives confirmation |

### 1.4 References

- **PRD**: [PRD.md](../PRD.md)
- **Design**: [DESIGN.md](../DESIGN.md)
- **Dependencies**: `cpt-cf-chat-engine-feature-session-lifecycle`, `cpt-cf-chat-engine-feature-message-processing`

## 2. Actor Flows (CDSL)

### Delete Message

- [ ] `p1` - **ID**: `cpt-cf-chat-engine-flow-message-delete-delete-message`

**Actor**: `cpt-cf-chat-engine-actor-client`

**Success Scenarios**:
- Client deletes an owned message; message, descendant subtree, and all reactions removed; confirmation returned

**Error Scenarios**:
- Session not found or not owned by the requesting user (404/403)
- Message not found or does not belong to the session (404)
- Unauthenticated request (401)

**Steps**:
1. [ ] - `p1` - Algorithm: authenticate request using `cpt-cf-chat-engine-algo-session-lifecycle-authenticate` - `inst-del-auth`
2. [ ] - `p1` - API: DELETE /sessions/{session_id}/messages/{message_id} - `inst-del-api`
3. [ ] - `p1` - Algorithm: validate message ownership using `cpt-cf-chat-engine-algo-message-delete-validate-message-ownership` - `inst-del-validate-ownership`
4. [ ] - `p1` - Algorithm: collect subtree IDs using `cpt-cf-chat-engine-algo-message-delete-collect-subtree` - `inst-del-collect-subtree`
5. [ ] - `p1` - DB: BEGIN TRANSACTION - `inst-del-tx-begin`
6. [ ] - `p1` - DB: Delete all reactions associated with messages in subtree_ids - `inst-del-db-delete-reactions`
7. [ ] - `p1` - DB: Delete all messages where message_id is in subtree_ids - `inst-del-db-delete-messages`
8. [ ] - `p1` - DB: COMMIT - `inst-del-tx-commit`
9. [ ] - `p1` - **RETURN** 200 `{message_id, deleted: true, deleted_count, deleted_at}` - `inst-del-return`

## 3. Processes / Business Logic (CDSL)

### Validate Message Ownership

- [ ] `p1` - **ID**: `cpt-cf-chat-engine-algo-message-delete-validate-message-ownership`

**Input**: session_id, message_id, request identity (tenant_id, user_id)
**Output**: Message record or 401/403/404 error

**Steps**:
1. [ ] - `p1` - DB: Retrieve session record (session_id, tenant_id, user_id) by session_id - `inst-own-db-get-session`
2. [ ] - `p1` - **IF** no session row returned **RETURN** 404 Not Found - `inst-own-session-not-found`
3. [ ] - `p1` - **IF** session.tenant_id != request.tenant_id **RETURN** 403 Forbidden - `inst-own-check-tenant`
4. [ ] - `p1` - **IF** session.user_id != request.user_id **RETURN** 403 Forbidden - `inst-own-check-user`
5. [ ] - `p1` - DB: Retrieve message record (message_id, session_id) by message_id and session_id - `inst-own-db-get-message`
6. [ ] - `p1` - **IF** no message row returned **RETURN** 404 Not Found - `inst-own-message-not-found`
7. [ ] - `p1` - **RETURN** message record for caller use - `inst-own-return`

### Collect Subtree

- [ ] `p1` - **ID**: `cpt-cf-chat-engine-algo-message-delete-collect-subtree`

**Input**: message_id, session_id
**Output**: List of all message IDs in the subtree (target message + all descendants)

**Steps**:
1. [ ] - `p1` - DB: Recursively collect all descendant message IDs starting from parent_message_id, traversing the parent-child hierarchy scoped to session_id - `inst-subtree-cte`
2. [ ] - `p1` - Prepend the target message_id to the result set - `inst-subtree-prepend-root`
3. [ ] - `p1` - **RETURN** subtree_ids (list of all message IDs to delete) - `inst-subtree-return`

## 4. States (CDSL)

### Message Deletion State

This feature does not define explicit state machines. Message deletion is a synchronous, single-step operation: the message either exists and is deleted, or the request is rejected. No intermediate states are tracked.

## 5. Definitions of Done

### Message Deletion

- [ ] `p1` - **ID**: `cpt-cf-chat-engine-dod-message-delete-delete-message`

The system **MUST** permanently delete a message, its full descendant subtree, and all associated reactions in a single atomic DB transaction, scoped to the requesting user's session ownership validated via JWT claims (tenant_id + user_id), returning a structured deletion confirmation with count and timestamp.

**Implements**:
- `cpt-cf-chat-engine-flow-message-delete-delete-message`
- `cpt-cf-chat-engine-algo-message-delete-validate-message-ownership`
- `cpt-cf-chat-engine-algo-message-delete-collect-subtree`
- `cpt-cf-chat-engine-algo-session-lifecycle-authenticate`

**Touches**:
- API: `DELETE /sessions/{session_id}/messages/{message_id}`
- DB: `messages`, `message_reactions` (cascade subtree delete)
- Entities: `Message`, `Reaction`

## 6. Acceptance Criteria

- [ ] DELETE /sessions/{session_id}/messages/{message_id} requires a valid JWT bearer token; missing or invalid token returns 401
- [ ] Session not found or belonging to a different user returns 404; cross-tenant access returns 403
- [ ] Message not found or not belonging to the specified session returns 404
- [ ] Successful deletion removes the message, all descendant messages, and all associated reactions atomically; response is 200 `{message_id, deleted: true, deleted_count, deleted_at}`
- [ ] tenant_id and user_id are always sourced from JWT claims, never from the request body or path parameters
- [ ] Cascade delete of subtree and reactions does not affect other messages or sessions
- [ ] Deleting a message with child messages removes the entire subtree (recursive)
- [ ] Response includes deleted_count reflecting total messages removed (target + descendants)

## 7. Non-Functional Considerations

- **Performance**: Recursive CTE for subtree collection targets < 100ms for trees up to 1000 nodes. Batch DELETE within single transaction avoids per-row round-trips.
- **Security**: 404 returned for both "not found" and "not owned" cases to prevent message ID enumeration. 403 only for cross-tenant access. Response does not leak information about other users' messages.
- **Reliability**: Entire subtree deletion is atomic (single transaction). On transaction failure, no partial deletions occur.
- **Data**: Recursive CTE uses index on `messages.parent_message_id`. Reactions deletion uses FK index on `message_reactions.message_id`.
- **Compliance / UX / Business**: Not applicable — see session-lifecycle §7.
