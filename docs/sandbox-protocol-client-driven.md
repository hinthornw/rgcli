# Client-Driven Sandbox Negotiation Protocol

Research document for the sandbox sharing protocol where the **client (ailsd)** creates/selects the sandbox and tells the **server (LangGraph deployment)** which sandbox to use.

## Overview

In this model, ailsd is the sandbox owner. It creates or selects an existing sandbox via the LangSmith sandbox API (`lsandbox::SandboxClient`), then passes sandbox connection details to the LangGraph agent so the agent's filesystem/shell tools route through that sandbox.

The client already has full sandbox lifecycle management (create, connect, exec, sync, delete) as seen in `src/commands/sandbox.rs`. The question is how to let the server's agent tools also target the same sandbox.

---

## 1. Auth Flow: How the Server Gets Access

### Option A: Scoped Token Delegation

The client mints a short-lived, scoped token that grants the server access to a specific sandbox. The flow:

1. Client creates sandbox, gets full access via its `LANGSMITH_API_KEY`.
2. Client calls a `/sandbox/{id}/delegate` endpoint requesting a scoped token with:
   - Read/write filesystem access
   - Shell exec (optionally restricted to certain commands)
   - Expiry (e.g., 1 hour, renewable)
   - No delete/lifecycle permissions
3. Client passes the scoped token to the server as part of the run config.
4. Server's `BackendProtocol` uses the scoped token for sandbox operations.

**Pros**: Least privilege. Server cannot destroy or reassign the sandbox. Token can expire independently of the client session.

**Cons**: Requires a new delegation API on the sandbox service. Token refresh adds complexity.

### Option B: Shared API Key (Current Model)

Both client and server use the same `LANGSMITH_API_KEY`. The server already has this key (it's a LangSmith deployment). The client just passes the sandbox name/ID.

**Pros**: Zero new auth infrastructure. Works today with existing `lsandbox::SandboxClient`.

**Cons**: Server has full lifecycle control (could delete the sandbox). No granular permissions. Acceptable if client and server are same-org, same-trust-level.

### Option C: Sandbox-Level Access Token

Each sandbox gets a dataplane-level bearer token at creation time. The client passes this token (not its API key) to the server. The dataplane token only grants read/write/exec on that specific sandbox.

**Pros**: Narrow scope without needing a delegation API. Token is sandbox-specific.

**Cons**: Requires the sandbox service to support per-sandbox tokens (may already exist via `dataplane_url` auth).

### Recommendation

Start with **Option B** (shared API key) for v1 since client and server are both LangSmith-authenticated. Design the config payload to carry an opaque `sandbox_credential` field so we can swap to Option A or C later without protocol changes.

---

## 2. Ownership and Lifecycle

### Creation
Client creates the sandbox before starting or resuming a thread:

```
ailsd sandbox create <template> --name <name>
```

Or automatically when entering chat mode with `--sandbox` flag.

### Lifecycle Management
The client owns the lifecycle:

| Event | Action |
|-------|--------|
| Client starts chat with `--sandbox` | Create or reuse sandbox |
| Client disconnects gracefully | Keep sandbox alive (configurable TTL) |
| Client runs `ailsd sandbox delete` | Explicit teardown |
| Sandbox TTL expires | Platform garbage-collects |
| Thread is deleted | Optional: delete associated sandbox |

### Orphaned Sandbox Mitigation

- **TTL on sandbox**: Set at creation time. If no client heartbeat within TTL, sandbox is stopped (not deleted, so state is preserved).
- **Heartbeat**: Client periodically pings sandbox while connected. Implemented via the existing WebSocket connection or a lightweight HTTP keepalive.
- **`ailsd sandbox list`**: Already exists. Users can see and clean up orphans.
- **Metadata tags**: Tag sandboxes with `thread_id`, `created_by: ailsd`, `session_id`. Enables batch cleanup.

### Reconnection
If client crashes and restarts, it can:
1. List sandboxes tagged with its session/thread.
2. Resume the sandbox (if within TTL).
3. Re-inject sandbox details into the next agent run.

---

## 3. Scoping: Sandbox-to-Thread Mapping

### Option A: 1:1 (One Sandbox Per Thread)
- Simple mental model. Thread state and filesystem state are coupled.
- Wasteful if many threads, easy cleanup when thread is done.

### Option B: 1:N (One Sandbox Across Threads)
- Client reuses a sandbox for multiple threads/conversations.
- Useful for long-lived dev environments.
- Risk: agent actions in one thread affect another thread's filesystem.

### Option C: Named Environments (User-Managed)
- User creates named sandboxes independently of threads.
- Attaches a sandbox to any thread via `--sandbox <name>`.
- Most flexible, most complexity.

### Recommendation
Default to **Option A** (auto-created, 1:1) with opt-in to **Option C** (named, reusable). The config payload includes a `sandbox_id` field; whether it was auto-created or user-provided is a client concern, transparent to the server.

---

## 4. Performance

Client-driven has a performance advantage: **no negotiation round-trip**.

The flow is:
1. Client creates sandbox (one-time, can be pre-warmed).
2. Client sends run request with `sandbox_id` in config.
3. Server immediately connects to sandbox using provided credentials.

Compare with server-driven, which requires:
1. Client sends run request.
2. Server creates sandbox.
3. Server returns sandbox details in response/event.
4. Client connects to sandbox.

Client-driven saves one round-trip and allows **pre-warming**: client can create the sandbox before the user even starts typing, so it's ready instantly.

### Latency Estimates
- Sandbox creation: ~2-5s (container spin-up).
- Pre-warm eliminates this from the critical path.
- Sandbox connection (WebSocket): ~100-300ms.
- File sync (tar upload): depends on size, typically <1s for small projects.

---

## 5. Security Analysis

### Trust Boundary

In client-driven mode, the client tells the server "use this sandbox." This means:

**Threat: Malicious client points server at attacker-controlled sandbox**
- The server's agent would execute code in an attacker-controlled environment.
- Mitigation: The server should verify the sandbox belongs to the same org/workspace. The `sandbox_id` must be validated against the LangSmith API using the server's own credentials, not blindly trusted.

**Threat: Client gains access to server's internal state via shared sandbox**
- If the agent writes secrets or internal state to the sandbox filesystem, the client can read them.
- Mitigation: Agent tools should never write server secrets to the sandbox. The sandbox is explicitly a shared workspace, like a shared filesystem. Treat it as untrusted from the server's perspective.

**Threat: Client manipulates sandbox filesystem to influence agent behavior**
- Client could place malicious files that the agent's tools read and act on.
- This is inherent and by design: the whole point is that both parties share a filesystem. The agent must treat sandbox contents as user-provided input.

**Threat: Sandbox escape**
- Standard container isolation concerns. Not specific to this protocol.
- Mitigated by the sandbox platform's isolation guarantees.

### Trust Model Summary

```
Client (ailsd)  <-- trusts -->  Sandbox Platform  <-- trusts -->  Server (LangGraph)
     |                                                                    |
     +-- Full access (owner) -----> Sandbox <----- Scoped access ---------+
```

The sandbox is a **shared untrusted workspace**. Both client and server treat its contents as potentially adversarial. The sandbox platform is the trusted intermediary.

---

## 6. Prior Art

### SSH Port Forwarding / VS Code Remote SSH
- Client initiates connection, sets up tunnel, server executes within client-provided environment.
- Relevant parallel: client "owns" the remote environment and grants the server (language server, extensions) access.
- Key difference: SSH has a well-established auth model (keys, certificates). We need to define our equivalent.

### GitHub Codespaces
- User creates a codespace (client-driven). GitHub's backend services interact with it.
- Uses per-codespace tokens for API access.
- Cleanup via inactivity timeout + manual delete.
- Very close to our model.

### Google Cloud Shell / AWS CloudShell
- Platform creates shell on behalf of user. More server-driven.
- Less relevant to client-driven, but shows TTL/cleanup patterns.

### Gitpod / Coder
- User-provisioned dev environments. Workspace is created by user, IDE connects.
- Agent/bot integrations use workspace-scoped API tokens.
- Strong parallel for the delegation model.

### Local-First Dev Tools (Cursor, Windsurf)
- User's local filesystem is the "sandbox."
- Agent acts on user's filesystem via tool calls.
- No delegation needed since it's all local. But the trust model is the same: agent treats filesystem as shared space.

---

## 7. Implementation Complexity

### Client Side (ailsd)

**Low complexity.** Most infrastructure already exists:

- `SandboxClient` for create/get/delete (in `src/commands/sandbox.rs`)
- `SandboxTerminal` for interactive shell (in `src/ui/chat/sandbox_pane.rs`)
- File sync via tar upload (in `sandbox::sync`)

New work needed:
1. **Config injection**: Add `sandbox_id` and `sandbox_credential` to the run request config sent to LangGraph. Modify `new_run_request()` in `src/api/types.rs`.
2. **Auto-create on chat start**: In `src/ui/app.rs`, optionally create a sandbox when chat begins with `--sandbox` flag.
3. **Heartbeat**: Periodic ping while chat is active (trivial with existing tokio runtime).
4. **Cleanup on exit**: Optionally delete sandbox when chat ends (or leave alive with TTL).

Estimated: ~200-400 lines of new code.

### Server Side (LangGraph)

**Medium complexity.** The server needs:

1. **Config parsing**: Extract `sandbox_id` and `sandbox_credential` from run config.
2. **BackendProtocol implementation**: A new backend that routes filesystem/shell tool calls to the sandbox API instead of the local filesystem.
3. **Sandbox validation**: Verify the provided `sandbox_id` is accessible and belongs to the right org.
4. **Tool registration**: Register sandbox-backed tools (read_file, write_file, exec_command) when sandbox config is present.

This is server-side work, but the protocol is straightforward: the server receives a sandbox reference in config and uses it.

---

## 8. Proposed Wire Protocol

### Run Request Config

```json
{
  "config": {
    "configurable": {
      "sandbox": {
        "sandbox_id": "sb_abc123",
        "credential": "<scoped-token-or-api-key>",
        "dataplane_url": "https://sandbox-dp.langsmith.com/sb_abc123"
      }
    }
  }
}
```

### Sandbox Lifecycle Events (SSE)

The server can optionally emit sandbox-related events so the client knows what's happening:

```
event: sandbox
data: {"type": "connected", "sandbox_id": "sb_abc123"}

event: sandbox
data: {"type": "file_written", "path": "/workspace/main.py"}

event: sandbox
data: {"type": "exec", "command": "python main.py", "exit_code": 0}
```

These are informational; the client can use them to update the TUI sandbox pane in real-time.

---

## 9. Summary: Strengths and Weaknesses

### Strengths
- No negotiation round-trip; client is in control
- Pre-warming possible (create sandbox before run starts)
- Client manages lifecycle; natural fit since the human "owns" the dev session
- Sandbox reuse across threads is trivial
- Most client infrastructure already exists in ailsd

### Weaknesses
- Server must trust client-provided sandbox reference (mitigated by validation)
- Client must handle cleanup / orphan prevention
- Auth delegation not yet built (but shared API key works for v1)
- Slightly more client complexity than server-driven (client does more work)
