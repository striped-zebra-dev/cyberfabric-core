Created:  2026-03-06 by Constructor Tech
Updated:  2026-03-06 by Constructor Tech
# Feature: Message Reactions & Feedback


<!-- toc -->

- [1. Feature Context](#1-feature-context)
  - [1.1 Overview](#11-overview)
  - [1.2 Purpose](#12-purpose)
  - [1.3 Actors](#13-actors)
  - [1.4 References](#14-references)
- [2. Actor Flows (CDSL)](#2-actor-flows-cdsl)
  - [Add/Change Reaction](#addchange-reaction)
  - [Remove Reaction](#remove-reaction)
  - [Get Reactions](#get-reactions)
- [3. Processes / Business Logic (CDSL)](#3-processes--business-logic-cdsl)
  - [Validate Reaction Access](#validate-reaction-access)
  - [UPSERT Reaction](#upsert-reaction)
  - [Notify Plugin](#notify-plugin)
- [4. States (CDSL)](#4-states-cdsl)
  - [Reaction Lifecycle State](#reaction-lifecycle-state)
- [5. Definitions of Done](#5-definitions-of-done)
  - [Reaction UPSERT](#reaction-upsert)
  - [Reaction Retrieval](#reaction-retrieval)
  - [Plugin Notification](#plugin-notification)
- [6. Acceptance Criteria](#6-acceptance-criteria)
- [7. Non-Functional Considerations](#7-non-functional-considerations)

<!-- /toc -->

- [ ] `p2` - **ID**: `cpt-cf-chat-engine-featstatus-message-reactions`

## 1. Feature Context

- [ ] `p2` - `cpt-cf-chat-engine-feature-message-reactions`

**Traces to**: `cpt-cf-chat-engine-fr-message-feedback` (FR-018), `cpt-cf-chat-engine-nfr-authentication` (NFR-006), `cpt-cf-chat-engine-nfr-lifecycle-performance` (NFR-014)

### 1.1 Overview

Per-user per-message like/dislike reactions with UPSERT semantics, stored independently from message content in a dedicated `message_reactions` table. Reactions can be added, changed, or removed (via `reaction_type: "none"`). A fire-and-forget `message.reaction` event is sent to the backend plugin for analytics after each reaction change. ADR-0020 rejected rich emoji reactions in favour of simple like/dislike.

### 1.2 Purpose

Enable end-users to provide lightweight feedback on individual messages without modifying message content or breaking the immutable tree principle. Backend plugins receive reaction events asynchronously for analytics and quality tracking.

Success criteria: Reactions are stored and retrieved within lifecycle-performance bounds; UPSERT semantics enforce one reaction per user per message at the database level; plugin notification failures never block the client response.

### 1.3 Actors

| Actor | Role in Feature |
|-------|-----------------|
| `cpt-cf-chat-engine-actor-client` | Submits, changes, or removes reactions on messages |
| `cpt-cf-chat-engine-actor-backend-plugin` | Receives fire-and-forget `message.reaction` events for analytics |

### 1.4 References

- **PRD**: [PRD.md](../PRD.md)
- **Design**: [DESIGN.md](../DESIGN.md)
- **ADR**: [ADR-0020 Message Reactions](../ADR/0020-message-reactions.md)
- **Dependencies**: `cpt-cf-chat-engine-feature-message-processing`
- **Sub-features**: [Message Delete](message-delete.md) (cascade deletion of messages and their reactions)

## 2. Actor Flows (CDSL)

### Add/Change Reaction

- [ ] `p2` - **ID**: `cpt-cf-chat-engine-flow-message-reactions-add-reaction`

**Actor**: `cpt-cf-chat-engine-actor-client`

**Success Scenarios**:
- Client submits like or dislike on a message; reaction is stored (or updated if one already exists); confirmation returned immediately

**Error Scenarios**:
- Session not found or not owned by the requesting user (404/403)
- Message not found or does not belong to the session (404)
- Invalid reaction_type value (400)
- Unauthenticated request (401)

**Steps**:
1. [ ] - `p2` - Algorithm: authenticate request using `cpt-cf-chat-engine-algo-session-lifecycle-authenticate` - `inst-add-auth`
2. [ ] - `p2` - API: POST /sessions/{session_id}/messages/{message_id}/reaction (body: reaction_type: "like" | "dislike") - `inst-add-api`
3. [ ] - `p2` - Algorithm: validate reaction access using `cpt-cf-chat-engine-algo-message-reactions-validate-access` - `inst-add-validate`
4. [ ] - `p2` - **IF** reaction_type NOT IN ("like", "dislike") **RETURN** 400 Bad Request (invalid reaction_type) - `inst-add-check-type`
5. [ ] - `p2` - Algorithm: UPSERT reaction using `cpt-cf-chat-engine-algo-message-reactions-upsert` - `inst-add-upsert`
6. [ ] - `p2` - **RETURN** 200 `{message_id, reaction_type, applied: true}` - `inst-add-return`
7. [ ] - `p2` - Algorithm: notify plugin using `cpt-cf-chat-engine-algo-message-reactions-notify-plugin` (fire-and-forget, after client response) - `inst-add-notify`

### Remove Reaction

- [ ] `p2` - **ID**: `cpt-cf-chat-engine-flow-message-reactions-remove-reaction`

**Actor**: `cpt-cf-chat-engine-actor-client`

**Success Scenarios**:
- Client sends reaction_type "none"; existing reaction is deleted; confirmation returned

**Error Scenarios**:
- Session not found or not owned by the requesting user (404/403)
- Message not found or does not belong to the session (404)
- No existing reaction to remove (idempotent: returns applied: false)
- Unauthenticated request (401)

**Steps**:
1. [ ] - `p2` - Algorithm: authenticate request using `cpt-cf-chat-engine-algo-session-lifecycle-authenticate` - `inst-rm-auth`
2. [ ] - `p2` - API: POST /sessions/{session_id}/messages/{message_id}/reaction (body: reaction_type: "none") - `inst-rm-api`
3. [ ] - `p2` - Algorithm: validate reaction access using `cpt-cf-chat-engine-algo-message-reactions-validate-access` - `inst-rm-validate`
4. [ ] - `p2` - DB: Delete the reaction record for the given message_id and user_id - `inst-rm-delete`
5. [ ] - `p2` - **IF** no row deleted: **RETURN** 200 `{message_id, reaction_type: "none", applied: false}` (idempotent) - `inst-rm-noop`
6. [ ] - `p2` - **RETURN** 200 `{message_id, reaction_type: "none", applied: true}` - `inst-rm-return`
7. [ ] - `p2` - Algorithm: notify plugin using `cpt-cf-chat-engine-algo-message-reactions-notify-plugin` (fire-and-forget, after client response) - `inst-rm-notify`

### Get Reactions

- [ ] `p2` - **ID**: `cpt-cf-chat-engine-flow-message-reactions-get-reactions`

**Actor**: `cpt-cf-chat-engine-actor-client`

**Success Scenarios**:
- Client retrieves all reactions for a message, scoped to their session ownership

**Error Scenarios**:
- Session not found or not owned by the requesting user (404/403)
- Message not found or does not belong to the session (404)
- Unauthenticated request (401)

**Steps**:
1. [ ] - `p2` - Algorithm: authenticate request using `cpt-cf-chat-engine-algo-session-lifecycle-authenticate` - `inst-get-auth`
2. [ ] - `p2` - API: GET /sessions/{session_id}/messages/{message_id}/reactions - `inst-get-api`
3. [ ] - `p2` - Algorithm: validate reaction access using `cpt-cf-chat-engine-algo-message-reactions-validate-access` - `inst-get-validate`
4. [ ] - `p2` - DB: Retrieve all reaction records (message_id, user_id, reaction_type, created_at, updated_at) for the given message_id - `inst-get-select`
5. [ ] - `p2` - **RETURN** 200 `{message_id, reactions: [{user_id, reaction_type, created_at, updated_at}]}` - `inst-get-return`

## 3. Processes / Business Logic (CDSL)

### Validate Reaction Access

- [ ] `p2` - **ID**: `cpt-cf-chat-engine-algo-message-reactions-validate-access`

**Input**: session_id, message_id, request identity (tenant_id, user_id)
**Output**: Validated session and message records or 401/403/404 error

**Steps**:
1. [ ] - `p2` - Algorithm: validate session ownership using `cpt-cf-chat-engine-algo-session-lifecycle-validate-ownership` - `inst-va-ownership`
2. [ ] - `p2` - DB: Retrieve message record (message_id, session_id) by message_id and session_id - `inst-va-get-message`
3. [ ] - `p2` - **IF** no message row returned **RETURN** 404 Not Found - `inst-va-message-not-found`
4. [ ] - `p2` - **RETURN** validated context (session, message, identity) - `inst-va-return`

### UPSERT Reaction

- [ ] `p2` - **ID**: `cpt-cf-chat-engine-algo-message-reactions-upsert`

**Input**: message_id, user_id, reaction_type ("like" | "dislike")
**Output**: Stored reaction record with previous_reaction_type for plugin notification

**Steps**:
1. [ ] - `p2` - DB: Look up existing reaction_type for the given message_id and user_id - `inst-up-select-existing`
2. [ ] - `p2` - **IF** no existing row: set previous_reaction_type=null - `inst-up-prev-null`
3. [ ] - `p2` - **IF** existing row: set previous_reaction_type=existing.reaction_type - `inst-up-prev-existing`
4. [ ] - `p2` - DB: Upsert reaction record for (message_id, user_id) — insert new or update existing reaction_type and updated_at timestamp - `inst-up-upsert`
5. [ ] - `p2` - **RETURN** {message_id, user_id, reaction_type, previous_reaction_type} - `inst-up-return`

### Notify Plugin

- [ ] `p2` - **ID**: `cpt-cf-chat-engine-algo-message-reactions-notify-plugin`

**Input**: session_id, message_id, user_id, reaction_type, previous_reaction_type
**Output**: void (fire-and-forget)

**Steps**:
1. [ ] - `p2` - DB: Retrieve session_type_id from sessions by session_id - `inst-np-get-session-type`
2. [ ] - `p2` - DB: Retrieve plugin_instance_id from session_types by session_type_id - `inst-np-get-plugin`
3. [ ] - `p2` - Resolve plugin: `hub.get_scoped::<dyn ChatEngineBackendPlugin>(ClientScope::gts_id(&plugin_instance_id))` - `inst-np-resolve`
4. [ ] - `p2` - Build MessageReactionEvent: {event: "message.reaction", session_id, message_id, user_id, reaction_type, previous_reaction_type, timestamp} - `inst-np-build-event`
5. [ ] - `p2` - **TRY** - `inst-np-try`
   1. [ ] - `p2` - Fire plugin invocation with MessageReactionEvent (fire-and-forget, non-blocking) - `inst-np-fire`
6. [ ] - `p2` - **CATCH** plugin error - `inst-np-catch`
   1. [ ] - `p2` - Log warning with trace_id and continue (notification failure must not affect client) - `inst-np-log`
7. [ ] - `p2` - **RETURN** void - `inst-np-return`

## 4. States (CDSL)

### Reaction Lifecycle State

- [ ] `p2` - **ID**: `cpt-cf-chat-engine-state-reactions-lifecycle`

This feature does not define explicit state machines. Reactions are stateless from a lifecycle perspective: they are created, updated, or deleted in a single synchronous operation. No intermediate states are tracked.

## 5. Definitions of Done

### Reaction UPSERT

- [ ] `p2` - **ID**: `cpt-cf-chat-engine-dod-message-reactions-upsert`

The system **MUST** accept POST /sessions/{session_id}/messages/{message_id}/reaction with `reaction_type` of "like", "dislike", or "none", enforce one reaction per user per message via composite PK (message_id, user_id) with UPSERT semantics, return immediate confirmation to the client, and send a fire-and-forget `message.reaction` event to the backend plugin after the client response.

**Implements**:
- `cpt-cf-chat-engine-flow-message-reactions-add-reaction`
- `cpt-cf-chat-engine-flow-message-reactions-remove-reaction`
- `cpt-cf-chat-engine-algo-message-reactions-validate-access`
- `cpt-cf-chat-engine-algo-message-reactions-upsert`
- `cpt-cf-chat-engine-algo-message-reactions-notify-plugin`
- `cpt-cf-chat-engine-algo-session-lifecycle-authenticate`
- `cpt-cf-chat-engine-algo-session-lifecycle-validate-ownership`

**Touches**:
- API: `POST /sessions/{session_id}/messages/{message_id}/reaction`
- DB: `message_reactions` (UPSERT / DELETE)
- Entities: `MessageReaction`, `MessageReactionEvent`

### Reaction Retrieval

- [ ] `p2` - **ID**: `cpt-cf-chat-engine-dod-message-reactions-retrieval`

The system **MUST** return all reactions for a given message scoped to the requesting user's session ownership, validated via JWT claims (tenant_id + user_id).

**Implements**:
- `cpt-cf-chat-engine-flow-message-reactions-get-reactions`
- `cpt-cf-chat-engine-algo-message-reactions-validate-access`
- `cpt-cf-chat-engine-algo-session-lifecycle-authenticate`

**Touches**:
- API: `GET /sessions/{session_id}/messages/{message_id}/reactions`
- DB: `message_reactions` (SELECT)
- Entities: `MessageReaction`

### Plugin Notification

- [ ] `p2` - **ID**: `cpt-cf-chat-engine-dod-message-reactions-notification`

The system **MUST** send a fire-and-forget `message.reaction` event to the backend plugin after storing or removing a reaction. Plugin notification failures must be logged but must never block or fail the client response.

**Implements**:
- `cpt-cf-chat-engine-algo-message-reactions-notify-plugin`

**Touches**:
- Plugin: `ChatEngineBackendPlugin` resolved by `plugin_instance_id`
- DB: `sessions`, `session_types` (plugin resolution)
- Entities: `MessageReactionEvent`

## 6. Acceptance Criteria

- [ ] POST /sessions/{session_id}/messages/{message_id}/reaction requires a valid JWT bearer token; missing or invalid token returns 401
- [ ] Session not found or belonging to a different user returns 404; cross-tenant access returns 403
- [ ] Message not found or not belonging to the specified session returns 404
- [ ] Submitting reaction_type "like" or "dislike" creates or updates the reaction via UPSERT; response is 200 `{message_id, reaction_type, applied: true}`
- [ ] Submitting reaction_type "none" deletes the existing reaction; response is 200 `{message_id, reaction_type: "none", applied: true}` or `applied: false` if no reaction existed
- [ ] Composite PK (message_id, user_id) enforces exactly one reaction per user per message at the database level
- [ ] Repeated identical requests produce the same result (idempotent UPSERT)
- [ ] Plugin receives a `message.reaction` event with previous_reaction_type after each reaction change; failures are logged and do not affect the client response
- [ ] tenant_id and user_id are always sourced from JWT claims, never from the request body or path parameters
- [ ] Reactions are cascade-deleted when the parent message is deleted (FK with CASCADE DELETE)

## 7. Non-Functional Considerations

- **Performance**: Reaction UPSERT targets < 50ms p95 latency. Composite PK index on `(message_id, user_id)` enables O(1) lookups. Reaction retrieval per message uses index on `message_id`.
- **Security**: 404 returned for both "not found" and "not owned" cases to prevent message ID enumeration. 403 only for cross-tenant access. Reaction data does not contain sensitive content.
- **Reliability**: Plugin notification is fire-and-forget; notification failures are logged but never block or retry. Database UPSERT is atomic and idempotent.
- **Data**: Composite PK `(message_id, user_id)` on `message_reactions`. FK on `message_id` with CASCADE DELETE. Index on `message_id` for per-message reaction queries.
- **Observability**: Structured log events for reaction add/change/remove with `trace_id`, `session_id`, `message_id`, `reaction_type`, `duration_ms`. Warning-level log on plugin notification failure.
- **Compliance / UX / Business**: Not applicable -- see session-lifecycle section 7.
