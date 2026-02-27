# Shared Sandbox Environment: Problem Statement

## Context

We have a **sandbox API** that provides remote container environments with:
- Filesystem access (read/write files)
- Command execution (one-shot and streaming via WebSocket)
- Templates, volumes, and pools for lifecycle management

We have a **deep agents framework** where agents use a `BackendProtocol` to interact with files. The protocol defines: `ls_info`, `read`, `write`, `edit`, `glob_info`, `grep_raw`. Backends are pluggable — today they use ephemeral in-memory state, but they could be backed by anything.

We have a **CLI/TUI tool (ailsd)** that humans use to interact with LangGraph deployments — chat with agents, manage threads, sync files, etc.

## The Problem

We want a human and an agent to **work in the same filesystem environment simultaneously**.

- The **agent** (running server-side in a LangGraph deployment) needs filesystem access for its tools — reading code, writing files, running commands.
- The **human** (running ailsd locally) needs to sync their local files to that same environment, open a terminal, and see what the agent is doing.

Today these are disconnected. The agent's filesystem is ephemeral and invisible to the human. The human's local filesystem is invisible to the agent.

**Sandboxes solve this** — they're persistent remote containers that both sides could connect to. But we need a way for both sides to agree on *which* sandbox to use and how to authenticate to it.

## What Needs to Be Figured Out

### 1. Association
How does a sandbox get associated with a session (thread, deployment, assistant, etc.)? Who creates it? When?

### 2. Discovery
How does each party find out which sandbox to use? Does the server tell the client? Does the client tell the server? Is it configured ahead of time?

### 3. Authentication
The sandbox API currently uses a LangSmith API key (`X-Api-Key` header) for both the control plane (CRUD) and the dataplane (execute, read, write, WebSocket). In a shared model:
- The server needs access to run agent filesystem operations
- The client needs access for terminal + file sync
- What credentials does each side use? Same key? Scoped tokens? Something else?

### 4. Lifecycle
Who creates the sandbox? Who tears it down? What happens when the session ends? What about long-running sandboxes that persist across sessions?

## Current Sandbox API

The sandbox system has two layers:

### Control Plane (LangSmith API — `api.smith.langchain.com/v2/sandboxes`)

| Operation | Endpoint | Description |
|-----------|----------|-------------|
| List sandboxes | `GET /boxes` | Returns `SandboxInfo[]` |
| Get sandbox | `GET /boxes/{name}` | Returns `SandboxInfo` |
| Create sandbox | `POST /boxes` | Body: `{ template_name, name? }` |
| Delete sandbox | `DELETE /boxes/{name}` | |
| List templates | `GET /templates` | Returns `SandboxTemplate[]` |
| Create template | `POST /templates` | Body: `{ name, image, cpu?, memory?, ... }` |
| Delete template | `DELETE /templates/{name}` | |
| List/create/delete volumes | `/volumes` | Persistent storage |
| List/create/delete pools | `/pools` | Pre-warmed instance pools |

### Dataplane (per-sandbox URL — `{sandbox.dataplane_url}`)

| Operation | Endpoint | Description |
|-----------|----------|-------------|
| Execute command | `POST /execute` | Body: `{ command, timeout, shell, env?, cwd? }` — returns `{ stdout, stderr, exit_code }` |
| Stream execute | `WS /execute/ws` | WebSocket streaming with stdout/stderr chunks, stdin input, kill, reconnect |
| Upload file | `POST /upload?path=...` | Multipart file upload |
| Download file | `GET /download?path=...` | Returns file bytes |

### Key Data Types

```
SandboxInfo { name, template_name, dataplane_url?, id?, created_at?, updated_at? }
SandboxTemplate { name, image, resources: { cpu, memory, storage? }, volume_mounts[] }
ExecutionResult { stdout, stderr, exit_code }
OutputChunk { stream: "stdout"|"stderr", data, offset }  // WebSocket streaming
```

### Auth
Both control plane and dataplane use `X-Api-Key` header with a LangSmith API key. Currently there is no concept of scoped or short-lived tokens.

## Agent-Side: BackendProtocol

The deep agents framework defines a `BackendProtocol` that agent filesystem tools call:

| Method | Description |
|--------|-------------|
| `ls_info(path)` | List directory contents |
| `read(file_path, offset?, limit?)` | Read file with line numbers |
| `write(file_path, content)` | Create file (create-only) |
| `edit(file_path, old_string, new_string, replace_all?)` | Edit file content |
| `glob_info(pattern, path?)` | Glob pattern matching |
| `grep_raw(pattern, path?, glob?)` | Regex search across files |

A sandbox-backed implementation of this protocol would translate these operations into sandbox dataplane calls. This is a server-side concern — it would live in the LangGraph deployment, not in ailsd.

## Human-Side: ailsd

ailsd currently supports:
- **CLI:** `ailsd sandbox list`, `create`, `exec`, `connect`, `sync`, `delete`
- **TUI:** `/sandbox list`, `/terminal <name>` to open a split terminal pane connected to a sandbox shell via WebSocket

File sync is local-to-sandbox via tar+upload. Terminal is a persistent bash shell over WebSocket.

## Open Questions

1. Should sandbox association be per-thread, per-assistant, per-deployment, or something else?
2. Should the sandbox outlive the thread/session?
3. How should credentials flow when both the server and the client need sandbox access?
4. Is there an existing pattern in LangSmith/LangGraph for this kind of shared resource negotiation?
5. What are the trust boundaries? Can the client be trusted to select the sandbox, or must the server be the authority?
6. How does this interact with pools (pre-warmed instances)?
