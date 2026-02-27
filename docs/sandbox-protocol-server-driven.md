# Server-Driven Sandbox Negotiation Protocol

## Overview

In the server-driven approach, the LangGraph deployment (server) is the sole authority for sandbox lifecycle management. The client (ailsd) requests access; the server creates, assigns, and tears down sandboxes. Credentials are vended by the server and scoped to specific resources.

## 1. Auth Flow

### Token Exchange Sequence

```
Client                          Server (LangGraph)              Sandbox
  |                                 |                              |
  |-- POST /sandbox/acquire ------->|                              |
  |   (thread_id, assistant_id,     |                              |
  |    client auth header)          |                              |
  |                                 |-- provision/lookup --------->|
  |                                 |<-- sandbox ready ------------|
  |                                 |                              |
  |<-- 200 SandboxGrant ------------|                              |
  |   { sandbox_id, token,          |                              |
  |     ws_url, http_url,           |                              |
  |     expires_at }                |                              |
  |                                 |                              |
  |-- WS connect (token) -------------------------------->|
  |-- HTTP file ops (Bearer token) ---------------------->|
```

### Credential Structure

```json
{
  "sandbox_id": "sb-abc123",
  "token": "sbx_eyJ...",
  "ws_url": "wss://sandbox-host/ws/sb-abc123",
  "http_url": "https://sandbox-host/api/sb-abc123",
  "expires_at": "2025-06-01T12:00:00Z",
  "scopes": ["fs:read", "fs:write", "shell:exec"],
  "refresh_url": "/sandbox/refresh"
}
```

The token is a short-lived JWT or opaque token. The server signs it with a key shared with (or known to) the sandbox infrastructure. The client never needs to know the sandbox provider's native credentials.

### Token Refresh

Tokens are short-lived (5-15 min). The client calls `POST /sandbox/refresh` with the current token before expiry. The server validates the client's session is still active, then issues a new token. If the underlying thread/run has ended, refresh is denied.

## 2. Ownership and Lifecycle

### Server Owns Everything

- **Creation**: Server provisions on first `/sandbox/acquire` for a given scope (thread, assistant, etc.).
- **Reuse**: Subsequent requests for the same scope return the existing sandbox with a fresh token.
- **Cleanup**: Server tears down sandboxes when the associated resource is deleted, after an idle timeout, or on explicit `POST /sandbox/release`.
- **Orphan prevention**: Server runs a periodic reaper. Sandboxes with no token refresh in N minutes are marked for cleanup. A grace period allows reconnection.

### Lifecycle States

```
Provisioning --> Active --> Idle --> Terminating --> Gone
                   ^         |
                   |_________|  (refresh/reconnect)
```

The server tracks sandbox state in its own database. Clients never manage state transitions directly.

## 3. Scoping Strategies

| Scope | Description | Pros | Cons |
|-------|-------------|------|------|
| **Per-thread** | Each thread gets its own sandbox | Strong isolation between conversations; cleanup maps to thread deletion | Many sandboxes if user has many threads |
| **Per-assistant** | All threads for an assistant share one sandbox | Fewer sandboxes; persistent workspace across conversations | Cross-thread data leakage; harder cleanup |
| **Per-user** | One sandbox per authenticated user | Simplest; user always sees same workspace | No isolation between projects; huge blast radius |
| **Per-run** | Ephemeral sandbox per agent run | Maximum isolation; no state leaks | No persistence; file sync overhead every run |

**Recommendation**: Per-thread with optional per-assistant promotion. Default to per-thread for isolation. Allow server config to scope per-assistant when the use case demands persistent workspaces. The server decides; the client just requests access for a given `(thread_id, assistant_id)` pair.

## 4. Performance Considerations

### Round-Trip Overhead

- **Acquire**: 1 extra HTTP round-trip before first sandbox use. If the sandbox is already warm, this is ~50-100ms. Cold provision can be 2-10s.
- **Refresh**: 1 round-trip every 5-15 min. Negligible for interactive sessions.
- **Mitigation**: Client caches the `SandboxGrant` and skips acquire if `expires_at` is in the future. Server can pre-provision sandboxes for active threads.

### Warm Pool

The server can maintain a pool of pre-provisioned sandboxes. On acquire, it assigns one from the pool instead of provisioning on demand. This reduces cold-start latency to near zero at the cost of idle resource usage.

### Client-Side Caching

```rust
struct SandboxCache {
    grants: HashMap<(ThreadId, AssistantId), SandboxGrant>,
}

impl SandboxCache {
    fn get(&self, key: &(ThreadId, AssistantId)) -> Option<&SandboxGrant> {
        self.grants.get(key).filter(|g| g.expires_at > Utc::now())
    }
}
```

## 5. Security Analysis

### Strengths

- **Minimal client trust**: Client never sees sandbox provider credentials. Server acts as a credential broker.
- **Scoped tokens**: Tokens are bound to a specific sandbox and have limited lifetime. Leaking a token gives access only to that sandbox until expiry.
- **Centralized revocation**: Server can revoke all tokens for a sandbox instantly by rotating the signing key or invalidating the sandbox record.
- **Audit trail**: All sandbox access flows through the server, enabling logging of who accessed what and when.

### Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Token leakage | Attacker accesses sandbox until expiry | Short TTL (5 min); token bound to client IP or fingerprint |
| Server compromise | All sandbox credentials exposed | Sandbox tokens are derived, not stored raw; HSM for signing keys |
| Token replay | Reuse of intercepted token | Bind token to TLS session or include nonce; ws connection ID in claims |
| Denial of service | Exhausting sandbox pool | Per-user rate limits on acquire; max concurrent sandboxes per user |
| Stale sessions | Client uses expired sandbox | Graceful 401 from sandbox triggers re-acquire flow |

### Token Format (JWT Example)

```json
{
  "sub": "user-123",
  "sandbox_id": "sb-abc123",
  "scopes": ["fs:read", "fs:write", "shell:exec"],
  "thread_id": "th-xyz",
  "iat": 1717200000,
  "exp": 1717200900
}
```

Signed by a key the sandbox infrastructure trusts. The sandbox validates the signature and claims on every request.

## 6. Prior Art

### GitHub Codespaces

Server-driven. User calls `POST /user/codespaces` with repo/branch. GitHub provisions a VM + Docker container, returns connection URLs and a scoped `GITHUB_TOKEN`. Token has read/write access to the source repo only. Lifecycle tied to the codespace object; auto-stop after idle timeout; user or org can delete. Auth uses OAuth scopes (`codespace` scope required). ([REST API docs](https://docs.github.com/en/rest/codespaces/codespaces))

### Gitpod

Server-driven with OIDC. User opens a workspace URL; Gitpod provisions a container. For programmatic access, bearer tokens via `gitpod.io/tokens`. Workspaces get OIDC identity tokens (`gp idp token`) for authenticating to external services -- this is the reverse direction (sandbox-to-external) but demonstrates the token-exchange pattern. ([Access Tokens docs](https://www.gitpod.io/docs/enterprise/configure/user-settings/access-tokens), [OIDC docs](https://www.gitpod.io/docs/configure/workspaces/oidc))

### AWS Cloud9

Server-driven. AWS provisions an EC2 instance or connects to existing compute. Auth via IAM credentials. Environment lifecycle managed through AWS API. Scoping is per-environment, tied to IAM user/role permissions.

### Daytona

Open-source CDE platform. Server provisions "workspaces" on configured providers. API-driven with bearer token auth. Supports MCP integration for AI agent access to sandboxes. ([daytona.io](https://www.daytona.io/))

### Common Patterns Across All

1. Server creates the environment on user request
2. Server vends scoped credentials (bearer token, SSH key, or URL with embedded auth)
3. Server manages lifecycle (idle timeout, explicit delete, org policies)
4. Client is stateless -- it gets connection info and connects

## 7. Implementation Complexity

### Server Side (LangGraph Deployment)

**New components needed:**
- `POST /sandbox/acquire` endpoint -- validates client auth, determines scope, provisions or reuses sandbox, mints token
- `POST /sandbox/refresh` endpoint -- validates existing token, issues new one
- `POST /sandbox/release` endpoint -- explicit teardown
- Sandbox state table (sandbox_id, scope, status, created_at, last_active)
- Token signing/verification (JWT library or opaque token store)
- Reaper job for idle sandbox cleanup
- Integration with sandbox provider (e.g., E2B, Fly Machines, Docker)

**Estimated effort**: Medium-high. The token minting and lifecycle management is the bulk of the work. If the deployment already has a sandbox provider integration, adding the credential brokering layer is incremental.

### Client Side (ailsd)

**New components needed:**
- `SandboxGrant` type and cache
- `acquire_sandbox(thread_id, assistant_id)` method on `Client`
- `refresh_sandbox(grant)` background task
- WebSocket connection using vended `ws_url` + token
- HTTP file operations using vended `http_url` + token
- Retry/re-acquire on 401

**Estimated effort**: Low-medium. The client is simple -- it calls one endpoint, caches the result, and connects. The complexity is in the server.

### Protocol Versioning

Include a `protocol_version` field in the acquire response. The client checks compatibility. This allows the server to evolve the sandbox API without breaking older clients.

```json
{
  "protocol_version": "1.0",
  "sandbox_id": "sb-abc123",
  ...
}
```

## 8. Summary

| Aspect | Assessment |
|--------|-----------|
| **Auth complexity** | Medium -- one extra endpoint + token minting |
| **Client complexity** | Low -- acquire, cache, connect |
| **Server complexity** | Medium-high -- lifecycle management, token signing, reaper |
| **Security** | Strong -- centralized control, scoped short-lived tokens |
| **Flexibility** | High -- server can change scoping/provider without client changes |
| **Latency** | One extra round-trip on first connect; warm pool mitigates cold start |
| **Prior art alignment** | Strong -- matches Codespaces, Gitpod, Cloud9, Daytona patterns |

The server-driven approach is the industry standard for CDE provisioning. It concentrates complexity on the server (which already manages the deployment) and keeps the client thin. The main trade-off is the server must implement credential brokering and lifecycle management, but this is well-understood infrastructure.
