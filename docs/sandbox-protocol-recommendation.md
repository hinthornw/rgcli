# Sandbox Protocol: Comparison and Recommendation

## 1. Side-by-Side Comparison

| Dimension | Server-Driven | Client-Driven | Hybrid (Negotiate + Capability URLs) |
|---|---|---|---|
| **Auth complexity** | Medium -- server mints tokens, manages signing keys | Low -- shared API key works today; delegation API needed later | Medium -- server mints scoped JWTs, sandbox validates locally |
| **Security posture** | Strong -- client never sees provider creds, centralized revocation | Weaker -- client tells server which sandbox to use; server must validate | Strong -- scoped short-lived JWTs, per-sandbox signing keys, audit via `jti` |
| **Performance overhead** | 1 extra round-trip on first connect; warm pool mitigates cold start | No negotiation round-trip; client pre-warms sandbox | 1 negotiation round-trip (amortized); JWT validation ~0.1ms on sandbox |
| **Client implementation** | Low (~200 LOC) -- acquire, cache, connect | Low (~200-400 LOC) -- most infra exists in ailsd already | Low-Medium (~300 LOC) -- negotiate, cache, refresh timer, auth-first-message |
| **Server implementation** | Medium-High -- lifecycle mgmt, token signing, reaper, sandbox provider integration | Medium -- config parsing, BackendProtocol, sandbox validation | Medium -- negotiate endpoint, refresh endpoint, HMAC signing, lease mgmt |
| **Flexibility** | High -- server can change provider/scoping without client changes | Medium -- client is coupled to sandbox provider API | High -- protocol is provider-agnostic via capability URLs |
| **Developer UX** | Simple -- client calls one endpoint, gets connection info | Best for power users -- `--sandbox <name>`, reuse across threads | Good -- client suggests sandbox, server confirms; reconnection works naturally |
| **Operational complexity** | Server owns cleanup (reaper, idle timeout) | Client owns cleanup (TTL, heartbeat, manual `sandbox delete`) | Shared -- server owns lease/destruction, client owns refresh/heartbeat |

## 2. Tradeoff Analysis

### Server-Driven

**You gain:** Centralized control, strong security defaults, industry-standard pattern (Codespaces, Gitpod, Cloud9). Server can evolve sandbox infra without client updates. Clean audit trail.

**You lose:** Requires server-side changes before anything works. Cold-start latency without warm pools. Client has no ability to pre-warm or reuse sandboxes across threads. Blocked until server team ships the endpoints.

### Client-Driven

**You gain:** Works today with existing `SandboxClient` code and shared API key. No server changes needed for v1. Client can pre-warm sandboxes, reuse them, manage them independently. Fastest path to a working demo.

**You lose:** Security is weaker -- server trusts client-provided sandbox reference. Cleanup burden falls on the client (orphan sandboxes). No centralized audit. Harder to lock down in multi-tenant deployments.

### Hybrid

**You gain:** Best of both -- server retains authority (validates, issues tokens, manages lifecycle) while client can suggest/reconnect to existing sandboxes. Capability URLs make the sandbox stateless (validates JWT locally). Clean incremental path from simple to production-grade.

**You lose:** Most overall design complexity. Requires server-side work (negotiate + refresh endpoints). Neither purely simple nor purely powerful -- it's a compromise.

## 3. Incremental Adoption Path

### Phase 0: Client-Driven with Shared API Key (works today)

- Client creates sandbox via existing `SandboxClient`.
- Client passes `{ sandbox_id, credential: <api_key> }` in run config.
- Server extracts sandbox_id from config, connects using the shared API key.
- No new server endpoints. No new auth. Just config passthrough.
- **Ship this in a week.** It proves the end-to-end flow.

### Phase 1: Add Server-Side Negotiation (hybrid)

- Server adds `POST /threads/{id}/sandbox` (negotiate) and `/refresh`.
- Server creates sandboxes on demand, issues scoped JWT tokens.
- Client switches from "create sandbox + pass ID" to "negotiate with server."
- Existing `--sandbox <name>` flag becomes a hint to the negotiate endpoint.
- **Ship in 2-4 weeks** after server team is available.

### Phase 2: Production Hardening

- Per-sandbox HMAC signing keys.
- Scope enforcement on sandbox side (`fs:rw`, `shell`, `process`).
- Audit logging with `jti` correlation.
- Lease management with heartbeats, idle timeout, reaper.
- Token revocation for incident response.

### Phase 3: Advanced Features

- Multi-user sandbox sharing (pair programming, agent + human).
- Observable shell sessions.
- Sandbox snapshots and restore.
- Provider abstraction (E2B, Modal, custom) behind capability URL interface.

## 4. Recommendation

**Phase 0 (now): Client-driven with shared API key.**

**Phase 1 (when server team is ready): Hybrid negotiate + capability URLs.**

### Rationale

We are early stage. We do not control the LangGraph server release cycle. The client-driven approach lets us ship a working sandbox integration immediately with zero server-side dependencies. The key insight is:

1. **ailsd already has full sandbox lifecycle management** (`SandboxClient`, create/connect/exec/sync/delete). Using it is trivial.
2. **Shared API key is acceptable** when client and server are same-org, same-trust-level -- which they are for all current users.
3. **The config payload is forward-compatible.** By passing `{ sandbox_id, credential, dataplane_url }` in run config today, we define the wire format. When the server later grows a negotiate endpoint, the client switches from "I created this sandbox, use it" to "give me a sandbox for this thread" -- but the sandbox access path (HTTP + WS with bearer token) stays the same.

The hybrid approach is the right Phase 1 target because:

- **Server retains authority** -- critical for multi-tenant and security audits.
- **Client can suggest sandboxes** -- supports reconnection and the `--sandbox <name>` UX.
- **Capability URLs make sandboxes stateless** -- sandbox containers just validate JWTs, no callback to server needed.
- **It borrows from proven patterns** -- AWS STS, GitHub App tokens, K8s projected tokens.

Do **not** go pure server-driven. It blocks all progress on server team availability and offers no UX advantage for a single-user CLI tool. The server-driven model is right for a hosted platform (like Codespaces), not for an early-stage CLI where the human operator is the primary user.

### Concrete Next Steps

1. **This week:** Add `sandbox` field to run request config in `src/api/types.rs`. Pass `sandbox_id` + API key when `--sandbox` flag is set. Verify agent tools route through sandbox.
2. **Next sprint:** Add auto-create sandbox on chat start (`--sandbox auto`). Add sandbox pane events from SSE stream.
3. **When server is ready:** Implement negotiate client in ailsd. Switch from direct sandbox creation to server-mediated. Keep `--sandbox <name>` as a hint parameter.
