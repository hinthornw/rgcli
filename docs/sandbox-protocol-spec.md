# Sandbox Session Access Protocol

**Version:** 0.1.0-draft  
**Status:** Proposal

## Purpose

This document defines:

1. A minimal protocol that is small enough to ship now (MVP).
2. A full target protocol ("north star") that MVP evolves into without breaking clients.

The core model is server-authoritative sandbox brokering:

- Client asks the server for sandbox access for a thread.
- Server resolves or creates the sandbox session and issues a short-lived token.
- Client and agent connect directly to the sandbox dataplane.

## Terms

- **Thread**: unit of conversation/work in LangGraph.
- **Sandbox**: provider-hosted compute/filesystem environment.
- **Session**: server-managed association between a thread and a sandbox.
- **Provider**: backend hosting sandboxes (LangSmith, E2B, Daytona, Modal, etc.).

---

## MVP Protocol (Ship First)

### MVP Scope

MVP intentionally supports only:

1. Thread-scoped sandbox sessions.
2. Two acquisition modes: `get` and `ensure`.
3. Token refresh and explicit release.

MVP intentionally does not require:

1. Multi-scope association (`workspace`, `assistant`, `deployment`).
2. Heartbeat endpoint.
3. Actor-specific capability policies.
4. Normalized provider-agnostic dataplane RPC schema.

### Transport

| Concern | Transport | Format |
|---|---|---|
| Session negotiation | HTTPS | JSON |
| Session refresh | HTTPS | JSON |
| Session release | HTTPS | No body |
| Dataplane command/file/shell | Provider-specific (typically HTTP + WebSocket) | Provider-specific |

### Control Plane API (MVP)

All endpoints are served by the LangGraph API server.

#### 1) Create or Get Session

`POST /v1/sandbox/sessions`

Headers:

- `Authorization: Bearer <server-auth-token>`
- `Idempotency-Key: <uuid>` (recommended for `mode=ensure`)

Request:

```json
{
  "thread_id": "thr_123",
  "mode": "get"
}
```

Fields:

- `thread_id` (required): thread to resolve.
- `mode` (required):
  - `get`: return existing session only.
  - `ensure`: return existing or create one.

Success response (`200`):

```json
{
  "session_id": "ssn_123",
  "thread_id": "thr_123",
  "sandbox": {
    "id": "sb_123",
    "provider": "langsmith",
    "http_base_url": "https://dp.example.com/v1",
    "ws_base_url": "wss://dp.example.com/v1"
  },
  "token": "opaque_or_jwt_token",
  "expires_at": "2026-02-27T20:20:00Z"
}
```

Errors:

- `401 UNAUTHENTICATED`
- `403 FORBIDDEN`
- `404 SESSION_NOT_FOUND` (`mode=get` and no session exists)
- `409 SESSION_CONFLICT`
- `423 SANDBOX_STARTING`
- `503 PROVIDER_UNAVAILABLE`

#### 2) Refresh Session Token

`POST /v1/sandbox/sessions/{session_id}/refresh`

Request body:

```json
{}
```

Success response (`200`):

```json
{
  "token": "new_token",
  "expires_at": "2026-02-27T20:50:00Z"
}
```

Errors:

- `401 UNAUTHENTICATED`
- `403 FORBIDDEN`
- `404 SESSION_NOT_FOUND`
- `410 SESSION_EXPIRED`

#### 3) Release Session

`DELETE /v1/sandbox/sessions/{session_id}`

Success response:

- `204 No Content`

Errors:

- `401 UNAUTHENTICATED`
- `403 FORBIDDEN`
- `404 SESSION_NOT_FOUND`

### MVP Dataplane Auth

After session negotiation, clients authenticate directly to provider dataplane using:

- HTTP: `Authorization: Bearer <token>`
- WebSocket: provider-specific mechanism (header or auth frame)

### MVP Responsibilities

Server:

1. Authorize caller access to `thread_id`.
2. Resolve/create thread session atomically.
3. Mint short-lived sandbox token.
4. Return dataplane coordinates.
5. Log audit metadata (`request_id`, caller, thread_id, session_id, sandbox_id).

Client:

1. Call `mode=get` when read-only lookup is intended.
2. Call `mode=ensure` when creation is allowed.
3. Refresh token before expiry.
4. Release session when done (best effort).

Provider adapter:

1. Create/reuse sandbox per server instruction.
2. Provide dataplane URL(s).
3. Validate token or accept server-relayed traffic.

### MVP State Model

Minimal server state:

- `thread_id -> session_id -> sandbox_id`
- `session_id` remains distinct from `sandbox_id` for forward compatibility.

### MVP Security Baseline

1. Never expose provider control-plane credentials to clients.
2. Tokens are short-lived.
3. Token scope is limited to one session/sandbox.
4. Server remains authority for session-to-thread association.

### MVP Bootstrap Mode (Phase 0)

If server endpoints are unavailable, client may pass pre-negotiated sandbox info via run config:

```json
{
  "config": {
    "configurable": {
      "sandbox": {
        "session_id": "ssn_local_1",
        "thread_id": "thr_123",
        "sandbox": {
          "id": "sb_123",
          "provider": "langsmith",
          "http_base_url": "https://dp.example.com/v1",
          "ws_base_url": "wss://dp.example.com/v1"
        },
        "token": "provider_token",
        "expires_at": "2026-02-27T20:20:00Z"
      }
    }
  }
}
```

The shape intentionally mirrors the MVP `POST /sessions` response.

---

## Full Protocol (North Star)

This is the target model MVP grows into. Additions are designed to be additive.

### Full Model Additions

1. First-class generic scope:
   - `scope: { "type": "thread|workspace|assistant|deployment", "id": "..." }`
2. Actor identity:
   - `actor: { "type": "human|agent", "id": "..." }`
3. Capability negotiation:
   - request and grant capability sets per actor.
4. Lease management:
   - separate token refresh from lease heartbeat.

### Full Control Plane API

#### Negotiate Session

`POST /v1/sandbox/sessions`

Request:

```json
{
  "scope": { "type": "thread", "id": "thr_123" },
  "mode": "ensure",
  "actor": { "type": "human", "id": "usr_1" },
  "requested_capabilities": ["fs_read", "fs_write", "shell", "exec"],
  "sandbox_hint": "optional-existing-sandbox",
  "provider_preference": "auto"
}
```

`mode` evolves to:

- `ensure`: attach existing or create.
- `attach_only`: fail if no existing binding.
- `create_only`: fail if already exists.

Response:

```json
{
  "binding": {
    "id": "bind_789",
    "scope": { "type": "thread", "id": "thr_123" },
    "sandbox_id": "sb_123",
    "provider": "langsmith",
    "state": "active",
    "lease_expires_at": "2026-02-27T21:00:00Z"
  },
  "dataplane": {
    "http_base_url": "https://dp.example.com/v1",
    "ws_base_url": "wss://dp.example.com/v1"
  },
  "credentials": {
    "token_type": "Bearer",
    "access_token": "short_lived_token",
    "expires_at": "2026-02-27T20:20:00Z",
    "granted_capabilities": ["fs_read", "fs_write", "shell"]
  },
  "policy": {
    "idle_timeout_seconds": 900,
    "hard_ttl_seconds": 28800
  }
}
```

#### Get Binding

`GET /v1/sandbox/sessions/{binding_id}`

#### Refresh Token

`POST /v1/sandbox/sessions/{binding_id}/refresh-token`

#### Heartbeat Lease

`POST /v1/sandbox/sessions/{binding_id}/heartbeat`

#### Release Binding

`DELETE /v1/sandbox/sessions/{binding_id}`

### Full Lifecycle States

- `provisioning`
- `active`
- `idle`
- `suspended`
- `terminated`
- `failed`

### Full Dataplane Target (Normalized)

Optional normalized provider interface:

- `POST /v1/exec`
- `WS /v1/exec/ws`
- `WS /v1/shell/ws`
- `POST /v1/files/upload`
- `GET /v1/files/download`

WebSocket frame types:

- Client: `auth`, `start`, `stdin`, `resize`, `signal`, `ping`, `close`
- Server: `auth_ok`, `ready`, `stdout`, `stderr`, `exit`, `pong`, `error`

### Relay Fallback

If provider cannot validate broker-issued tokens, server may proxy dataplane traffic while keeping the same control-plane protocol.

### Full Error Envelope

```json
{
  "error": {
    "code": "CAPABILITY_DENIED",
    "message": "shell not granted",
    "retryable": false,
    "request_id": "req_123"
  }
}
```

Recommended HTTP/code mapping:

- `401 UNAUTHENTICATED`
- `403 CAPABILITY_DENIED`
- `404 BINDING_NOT_FOUND`
- `409 BINDING_CONFLICT`
- `423 SANDBOX_STARTING`
- `429 RATE_LIMITED`
- `503 BACKEND_UNAVAILABLE`

---

## Evolution Rules

MVP to full protocol must preserve:

1. Endpoint continuity for existing MVP routes.
2. Backward compatibility of MVP response fields.
3. `session_id`/`binding_id` identity stability.
4. Additive request fields (new fields optional by default).

Migration strategy:

1. Ship MVP endpoints and thread-only model.
2. Add optional full negotiate shape (`scope`, `actor`, capabilities).
3. Add heartbeat and richer lifecycle states.
4. Add normalized dataplane + relay fallback where needed.

---

## Compatibility Notes

### MCP Mapping

| Protocol | MCP |
|---|---|
| `POST /v1/sandbox/sessions` | `environment/acquire` |
| `POST /v1/sandbox/sessions/{id}/refresh` | `environment/refresh` |
| `POST /v1/sandbox/sessions/{id}/heartbeat` (full) | `environment/heartbeat` |
| `DELETE /v1/sandbox/sessions/{id}` | `environment/release` |

### A2A Mapping

| Protocol | A2A |
|---|---|
| Session response | Task environment metadata |
| `sandbox.id` | Shared context reference |
| Dataplane access | Artifact/resource URI references |

