<!-- Created: 2026-02-04 by Constructor Tech -->
<!-- Updated: 2026-04-07 by Constructor Tech -->

# ADR-0016: Token-Based Session Sharing with Branching


<!-- toc -->

- [Context and Problem Statement](#context-and-problem-statement)
- [Decision Drivers](#decision-drivers)
- [Considered Options](#considered-options)
- [Decision Outcome](#decision-outcome)
  - [Consequences](#consequences)
  - [Confirmation](#confirmation)
- [Pros and Cons of the Options](#pros-and-cons-of-the-options)
  - [Option 1: Cryptographic share token with separate table](#option-1-cryptographic-share-token-with-separate-table)
  - [Option 2: Signed session_id JWT](#option-2-signed-session_id-jwt)
  - [Option 3: Publicly readable sessions](#option-3-publicly-readable-sessions)
- [Related Design Elements](#related-design-elements)

<!-- /toc -->

**Date**: 2026-02-04

**Status**: accepted

**Review**: Revisit if multi-token sharing per session is required

**ID**: `cpt-cf-chat-engine-adr-session-sharing`

## Context and Problem Statement

Users want to share conversations with others for collaboration, review, or assistance. Recipients should view the original conversation (read-only) and optionally create branches. How should Chat Engine enable secure session sharing without exposing session_id or requiring recipient authentication?

## Decision Drivers

* Secure sharing (no session_id exposure)
* Read-only access to original conversation
* Recipients can branch (not modify original)
* Cryptographically secure tokens (not guessable)
* Revocable sharing (owner can revoke access)
* Optional expiration (time-limited sharing)
* Track share token creator (audit trail)
* Multiple tokens per session (share with different groups)

## Considered Options

* **Option 1: Cryptographic share token as column on sessions table** - share_token column on sessions table maps token to session
* **Option 2: Signed session_id JWT** - Encode session_id in JWT, verify signature
* **Option 3: Publicly readable sessions** - Sessions publicly accessible by default

## Decision Outcome

Chosen option: "Cryptographic share token stored as a column on the sessions table", because it provides cryptographically secure tokens (min 32 chars random), enables revocation by clearing the column, supports optional expiration via application logic, and keeps session_id hidden from recipients. This approach avoids the overhead of a separate join table while leveraging the existing sessions table (`cpt-cf-chat-engine-dbtable-sessions`).

### Consequences

* Good, because share tokens cryptographically secure (not guessable)
* Good, because revocation instant (clear column value, no token re-issue)
* Good, because optional expiration (time-limited sharing via application logic)
* Good, because session_id hidden (token maps to session internally)
* Good, because recipients branch without owning session
* Good, because no separate table or join required (share_token is a column on sessions table)
* Bad, because single token per session (column approach limits to one active share token)
* Bad, because token generation requires crypto library
* Bad, because no token refresh mechanism (expired = generate new)

### Confirmation

Confirmed when a cryptographic share_token stored on the sessions table grants read-only access to the conversation and allows recipients to branch without exposing the session_id.

## Pros and Cons of the Options

### Option 1: Cryptographic share token as column on sessions table

* Good, because tokens are cryptographically secure and not guessable (min 32 chars random)
* Good, because revocation is instant by clearing the column value
* Good, because no separate table or join required (column on existing sessions table)
* Good, because UNIQUE constraint ensures token uniqueness across all sessions
* Bad, because single token per session (one column = one active share token)
* Bad, because no token refresh mechanism (expired tokens require generating new ones)

### Option 2: Signed session_id JWT

* Good, because stateless verification (no database lookup needed for validation)
* Good, because expiration is built into the JWT standard
* Bad, because session_id is embedded in the token payload (exposed if decoded)
* Bad, because revocation requires a blocklist (defeats stateless benefit)
* Bad, because multiple tokens per session with different permissions are awkward to manage

### Option 3: Publicly readable sessions

* Good, because no token generation or validation logic needed (simplest implementation)
* Good, because sharing is trivial (just share the session URL)
* Bad, because all sessions are exposed by default violating secure-by-default principle
* Bad, because no revocation or expiration possible (access is permanent and universal)
* Bad, because no audit trail for who accessed shared conversations

## Related Design Elements

**Actors**:
* `cpt-cf-chat-engine-actor-client` - Creates share token, shares URL with recipients
* `cpt-cf-chat-engine-actor-end-user` - Accesses shared session via token
* `cpt-cf-chat-engine-component-session-management` - Generates tokens, validates access

**Requirements**:
* `cpt-cf-chat-engine-fr-share-session` - Generate token, recipients view and branch
* `cpt-cf-chat-engine-usecase-share-session` - Full use case for sharing

**Design Elements**:
* `cpt-cf-chat-engine-design-entity-share-token` - Cryptographic token, session mapping, metadata
* `cpt-cf-chat-engine-dbtable-sessions` - Sessions table with share_token column (VARCHAR UNIQUE NULL)
* Sequence diagram S10 (Share Session)

**Related ADRs**:
* ADR-0014 (Conversation Branching from Any Historical Message) - Recipients branch from last message
* ADR-0015 (Session Type Switching with Capability Updates) - Branched sessions use original session type
