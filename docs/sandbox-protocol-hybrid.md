# Hybrid / Token-Exchange Sandbox Negotiation Protocol

Research findings for ailsd sandbox sharing between human (CLI) and LangGraph agent.

## Context

A sandbox is a remote container providing filesystem + shell (WebSocket) + HTTP API. The LangGraph agent routes its filesystem tools through a `BackendProtocol` backed by the sandbox. The human uses `ailsd` to sync files and open a terminal. Both need authenticated access to the same sandbox.

---

## 1. Sub-Pattern Analysis

### A) Client Suggests, Server Confirms

**Flow:**
1. Client sends `POST /sandbox/negotiate` with `{ preferred_sandbox_id?, thread_id, capabilities: ["fs:rw", "shell"] }`
2. Server validates: does the sandbox exist? Is the user authorized? Is the thread allowed?
3. Server returns `{ sandbox_id, token, expires_at, endpoints: { http, ws } }`

**Pros:** Client can resume a previous sandbox (pass existing ID). Server retains authority. Simple request/response.

**Cons:** Client must know sandbox IDs to suggest. Extra round-trip if suggestion is rejected.

**Verdict:** Best for the common case. Client passes thread_id, server maps to sandbox. Optional sandbox_id hint for reconnection.

### B) Server Assigns, Client Can Reassign

**Flow:**
1. Server creates sandbox on thread creation or first tool call.
2. Client discovers sandbox via `GET /threads/{id}/sandbox`.
3. Client can `POST /threads/{id}/sandbox/migrate` to move to a different sandbox.

**Pros:** Zero client-side complexity for the default path. Migration supports advanced workflows.

**Cons:** Migration is operationally expensive (state transfer). Two separate APIs to maintain.

**Verdict:** Migration adds complexity with little benefit for our use case. Skip migration; keep server-assigns as the default within pattern A.

### C) OAuth2-Style Token Exchange (RFC 8693)

**Flow:**
1. Client authenticates to LangGraph server with its API key (the `subject_token`).
2. Client requests `POST /oauth/token` with `grant_type=urn:ietf:params:oauth:grant-type:token-exchange`, `resource=sandbox:{id}`, `scope=fs:rw shell`.
3. Server validates, creates a scoped short-lived token, returns `{ access_token, token_type: "Bearer", expires_in, scope }`.
4. Client uses this token directly against the sandbox HTTP/WS APIs.

**Pros:** Standards-based. Well-understood security properties. Scoping and audience restriction built in. Existing libraries.

**Cons:** Heavier machinery than needed for a single-purpose protocol. Requires the server to act as a full token issuer. Overkill if sandbox auth is always mediated through the LangGraph server.

**Verdict:** Good model to borrow concepts from (scoped tokens, expiry, refresh) without fully implementing the RFC.

### D) Capability-Based (Signed URLs)

**Flow:**
1. Client requests sandbox access from server.
2. Server returns capability URLs: `https://sandbox-{id}.host/fs?token=<signed>`, `wss://sandbox-{id}.host/shell?token=<signed>`.
3. Token is HMAC-signed with expiry, scope, and sandbox ID embedded.

**Pros:** No token management on the sandbox side -- it just validates the signature. Extremely simple sandbox implementation. Works naturally with WebSocket (token in URL). No state on the sandbox.

**Cons:** Token in URL appears in logs, browser history (less relevant for CLI). Cannot revoke individual tokens without rotating the signing key. Longer URLs.

**Verdict:** Strong candidate for the sandbox side. The sandbox container can be stateless (just validate signature). Revocation concern is mitigated by short TTLs (5-15 min).

---

## 2. Recommended Protocol: A + D Hybrid

Combine **Client Suggests, Server Confirms** for negotiation with **Capability URLs** for sandbox access.

### Negotiation Phase (Client <-> LangGraph Server)

```
POST /threads/{thread_id}/sandbox
Authorization: Bearer <langgraph-api-key>
{
  "sandbox_id": "sbx_abc123",     // optional, for reconnection
  "scopes": ["fs:rw", "shell"],   // requested capabilities
  "ttl": 900                       // requested TTL in seconds
}

200 OK
{
  "sandbox_id": "sbx_abc123",
  "endpoints": {
    "http": "https://sbx-abc123.sandboxes.example.com",
    "ws":   "wss://sbx-abc123.sandboxes.example.com/ws"
  },
  "token": "<signed-capability-token>",
  "expires_at": "2026-02-26T12:15:00Z",
  "scopes": ["fs:rw", "shell"],
  "refresh_before": "2026-02-26T12:10:00Z"
}
```

### Access Phase (Client <-> Sandbox)

```
# HTTP file operations
GET https://sbx-abc123.sandboxes.example.com/files/path/to/file
Authorization: Bearer <signed-capability-token>

# WebSocket shell -- token in first message (not URL)
ws = connect("wss://sbx-abc123.sandboxes.example.com/ws")
ws.send(JSON({ "type": "auth", "token": "<signed-capability-token>" }))
ws.recv() -> { "type": "auth_ok", "session_id": "..." }
```

### Token Refresh

```
POST /threads/{thread_id}/sandbox/refresh
Authorization: Bearer <langgraph-api-key>
{
  "sandbox_id": "sbx_abc123",
  "current_token": "<old-token>"   // proves possession
}

200 OK
{
  "token": "<new-signed-capability-token>",
  "expires_at": "2026-02-26T12:30:00Z",
  "refresh_before": "2026-02-26T12:25:00Z"
}
```

---

## 3. Auth Patterns

### Token Format: Signed JWT (compact)

```
Header: { "alg": "HS256", "typ": "JWT" }
Payload: {
  "sub": "user_xyz",            // who
  "aud": "sbx_abc123",          // which sandbox
  "scope": "fs:rw shell",       // what
  "thread_id": "thr_456",       // context
  "exp": 1740571500,            // when it expires
  "iat": 1740570600,
  "jti": "tok_unique_id"        // for revocation/audit
}
Signature: HMAC-SHA256(header.payload, sandbox_signing_key)
```

**Why JWT over opaque:** The sandbox can validate locally without calling back to the server. For a container that may have limited connectivity, this is important.

**Why HMAC over RSA:** Single issuer (the LangGraph server), so asymmetric crypto is unnecessary overhead. HMAC is faster and produces smaller tokens.

**Key distribution:** Server generates a per-sandbox HMAC key at sandbox creation time and shares it with the sandbox container via environment variable or secrets mount.

### Token Lifecycle

| Event | Action |
|---|---|
| Thread created | No sandbox yet (lazy) |
| First sandbox request | Server creates sandbox + issues token |
| Token approaching expiry | Client calls `/refresh` (proactive, before `refresh_before`) |
| WebSocket disconnect | Client reconnects + sends new/existing token in auth message |
| Thread archived | Server destroys sandbox, signing key rotated implicitly |
| Token leak suspected | Server rotates sandbox signing key; all existing tokens invalidated |

### Alternative: Opaque Tokens + Server-Side Validation

If sandbox containers can reliably reach the server, opaque tokens with server-side validation are simpler:
- Sandbox calls `GET /internal/tokens/{token}` to validate.
- Enables instant revocation.
- Adds latency to every sandbox request.

**Recommendation:** Start with JWT for the common path, add an optional opaque-token mode for environments where instant revocation is required.

---

## 4. Ownership and Lifecycle

### Creation

- **Server creates** the sandbox on first negotiation request for a thread.
- Server records `(thread_id -> sandbox_id)` mapping.
- Sandbox has a **lease TTL** (e.g. 1 hour of inactivity). Heartbeats from either client or agent extend the lease.

### Shared Responsibility Model

| Concern | Owner |
|---|---|
| Sandbox creation | Server |
| Sandbox destruction | Server (lease expiry or explicit) |
| Token issuance | Server |
| Token refresh | Client (proactive) |
| Heartbeat / keep-alive | Client + Agent (either extends lease) |
| File conflict resolution | Application layer (not protocol) |

### Destruction

- Lease expires with no heartbeat -> server destroys sandbox.
- Thread deleted -> server destroys sandbox.
- Client calls `DELETE /threads/{id}/sandbox` -> server destroys.
- Admin force-kill via internal API.

---

## 5. Scoping

### Per-Thread Tokens (Recommended)

One token per `(user, thread, sandbox)` tuple. This is the natural granularity:
- A thread maps to one sandbox.
- A user has one active session per thread.
- Scopes are per-token: `fs:ro`, `fs:rw`, `shell`, `shell:ro` (observe only).

### Why Not Per-Session

Per-session tokens (e.g. one token per WebSocket connection) add complexity with minimal security benefit. If a token is scoped to a sandbox + thread with a short TTL, per-session granularity is unnecessary.

### Scope Definitions

| Scope | Grants |
|---|---|
| `fs:ro` | Read files via HTTP API |
| `fs:rw` | Read + write files |
| `shell` | Interactive shell via WebSocket |
| `shell:ro` | Observe shell output (no input) |
| `process` | Start/stop processes |

The agent typically gets `fs:rw` + `process`. The human gets `fs:rw` + `shell`. Scopes are requested at negotiation time and the server enforces policy.

---

## 6. Performance

### Token Exchange Overhead

- Negotiation: 1 round-trip to server (~50-200ms). Amortized over session lifetime.
- Token refresh: 1 round-trip, done proactively before expiry. No user-visible latency.
- JWT validation on sandbox: ~0.1ms (HMAC verify). Negligible.

### Caching

- Client caches `(thread_id -> sandbox_id, token, endpoints)` locally.
- On CLI restart, client sends cached `sandbox_id` in negotiation request to skip sandbox creation.

### WebSocket Reconnect

1. TCP drops or token expires mid-session.
2. Client detects disconnect (ping/pong timeout or close frame).
3. Client refreshes token via `/refresh` endpoint.
4. Client reconnects WebSocket, sends auth message with new token.
5. Shell session state is maintained server-side (tmux/screen in container); client resumes.

### Connection Pooling

- HTTP file operations: standard HTTP/2 connection pooling to sandbox endpoint.
- WebSocket: single long-lived connection per session. No pooling needed.

---

## 7. Security

### Principle of Least Privilege

- Tokens are scoped to specific capabilities.
- Server policy can downgrade requested scopes (e.g. agent requests `shell` but policy says `fs:rw` only).
- Short TTLs (5-15 min) limit blast radius of leaked tokens.

### WebSocket Auth Pattern

**Recommended: Token in first message (not URL, not header).**

| Method | Pros | Cons |
|---|---|---|
| Query param (`?token=`) | Simple | Logged in HTTP access logs, proxy logs |
| HTTP Upgrade header | Clean, standard | Some WebSocket libs don't support custom headers on upgrade |
| Cookie | Browser-compatible | Not relevant for CLI |
| **First message** | **No log exposure, universal library support** | **Requires server to hold unauthenticated connection briefly** |

Mitigation for first-message approach: sandbox drops connection if no valid auth message within 5 seconds.

### Token Leak Scenarios

| Scenario | Impact | Mitigation |
|---|---|---|
| Token logged accidentally | Attacker gets sandbox access until expiry | Short TTL (5 min). Token rotation on refresh. |
| Signing key compromised | Attacker can forge tokens for that sandbox | Per-sandbox keys. Key rotation destroys sandbox. |
| Man-in-the-middle | Token intercepted | TLS required for all connections. |

### Audit Trail

Every token includes `jti` (unique ID). Sandbox logs `jti` on each request. Server logs token issuance with `jti`, `sub`, `aud`, `scope`. Correlation across server + sandbox logs gives full audit trail.

---

## 8. Prior Art

### AWS STS AssumeRole
- Client exchanges long-lived credentials for short-lived scoped credentials.
- Directly analogous: ailsd exchanges API key for sandbox-scoped token.
- **Borrowed:** Scoped temporary credentials, expiry, session policies.

### GCP Service Account Impersonation
- Service A gets a short-lived token to act as Service B.
- **Borrowed:** The concept of the server issuing tokens on behalf of the user for a different service (sandbox).

### GitHub App Installation Tokens
- App authenticates with JWT, gets installation-scoped token.
- Token is short-lived (1 hour), scoped to specific repos/permissions.
- **Borrowed:** Two-phase auth (authenticate to issuer, get scoped token for resource). Permission scoping model.

### Kubernetes Service Account Token Projection
- Pods get projected tokens with audience, expiry, and automatic refresh.
- **Borrowed:** Audience-restricted tokens (our `aud` = sandbox ID). Automatic refresh before expiry.

### Cloudflare Access / Tailscale
- Capability URLs with embedded auth for service access.
- **Borrowed:** Concept of self-contained access tokens validated locally.

---

## 9. Implementation Complexity

### Phase 1: Minimal Viable (Low Complexity)

**Server side:**
- `POST /threads/{id}/sandbox` -- create/get sandbox, issue signed token.
- `POST /threads/{id}/sandbox/refresh` -- reissue token.
- HMAC signing with per-sandbox key.
- Sandbox container validates JWT on each request.

**Client side (ailsd):**
- Token cache in memory (lost on restart, re-negotiated on next connect).
- Proactive refresh timer.
- Auth-first-message for WebSocket.

**Estimate:** ~500 lines server, ~300 lines client.

### Phase 2: Production Hardening

- Token revocation list (short since TTLs are short).
- Scope enforcement on sandbox side.
- Audit logging.
- Sandbox lease management with heartbeats.

### Phase 3: Advanced

- Multi-user sandbox sharing (multiple users get separate tokens for same sandbox).
- Observable shell sessions (one user watches another's terminal).
- Token downscoping (user can create a further-restricted token from their token).

### Incremental Adoption Path

1. Start with server-assigned sandboxes and simple bearer tokens (opaque, server-validated).
2. Move to JWT when sandbox count grows and server-side validation becomes a bottleneck.
3. Add scoping when multi-role access is needed.
4. Add capability URLs if sandboxes become multi-tenant.

---

## 10. Recommended Wire Protocol Summary

```
# 1. Negotiate sandbox access
POST /threads/{thread_id}/sandbox
-> { sandbox_id, token, endpoints, expires_at, scopes }

# 2. Use sandbox (HTTP)
GET/PUT/DELETE {endpoints.http}/files/{path}
Authorization: Bearer {token}

# 3. Use sandbox (WebSocket)
CONNECT {endpoints.ws}
SEND: { "type": "auth", "token": "{token}" }
RECV: { "type": "auth_ok" }
SEND: { "type": "stdin", "data": "ls -la\n" }
RECV: { "type": "stdout", "data": "total 42\n..." }

# 4. Refresh token (before expiry)
POST /threads/{thread_id}/sandbox/refresh
-> { token, expires_at }

# 5. Release sandbox
DELETE /threads/{thread_id}/sandbox

# 6. Heartbeat (extend lease)
POST /threads/{thread_id}/sandbox/heartbeat
```

---

## 11. Open Questions

1. **Sandbox provider abstraction:** Should the protocol be agnostic to sandbox provider (E2B, Modal, custom)? If so, the capability URL pattern works best since it hides provider-specific auth.
2. **Agent token issuance:** Does the agent get its own token, or does it access the sandbox through an internal path (no token needed since it runs inside the deployment)?
3. **Concurrent access conflicts:** Two writers to the same file -- handle at protocol level (locking) or application level (last-write-wins)?
4. **Sandbox snapshots:** Should the protocol support snapshotting/restoring sandbox state? This affects token scoping (need a `snapshot` scope).
