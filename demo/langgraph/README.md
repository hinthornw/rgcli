# SSAP Demo (LangGraph)

## Setup

```bash
cd demo/langgraph
uv sync
uv pip install --editable ../../crates/langsmith-sandbox-py
```

The second command installs the local Rust PyO3 extension `lsandbox-py`.

## Env

```bash
export LANGSMITH_API_KEY="..."
# Optional hardening: if set, only these bearer tokens can create/manage SSAP sessions
export SSAP_CLIENT_BEARER_TOKENS="dev-client"
```

## Run

```bash
cd ../..
uv run --project demo/langgraph langgraph dev --config ./langgraph.json --no-browser
```

## Notes

- This is a development-only SSAP MVP scaffold.
- `LANGSMITH_API_KEY` must be set for session creation and relay mode endpoints.
- `LANGSMITH_SANDBOX_TEMPLATE` is optional. If unset, the app auto-selects the first available template from `GET /v2/sandboxes/templates`.
- Sandbox control-plane operations (template list/create/get) are executed through the Rust client via async-native `lsandbox_py` methods.
- Server auth mode is `noop` in this config so relay WebSockets work with current LangGraph middleware behavior.
- `./auth.py` is kept in-repo for future re-enable once custom-auth + websocket scope handling is fixed upstream.
- Set `SSAP_ENABLED=false` to disable SSAP routes.

## Quick Check

```bash
curl -s -X POST http://127.0.0.1:2024/v1/sandbox/sessions \
  -H "authorization: Bearer dev-client" \
  -H "content-type: application/json" \
  -d '{"thread_id":"thr_demo","mode":"ensure"}'
```

## ailsd End-To-End

1. Configure ailsd to use this server and bearer token:
```yaml
# .ailsd.yaml
endpoint: "http://127.0.0.1:2024"
api_key: "dev-client"
assistant_id: "agent"
```

2. Create a real LangGraph thread id (UUID):
```bash
TID=$(ailsd threads create | awk '{print $3}')
```

3. Start chat on that same thread (chat auto-runs `session-ensure` and shows session id in header):
```bash
ailsd --thread-id "$TID"
```

4. In chat, run:
```text
/exec pwd
/terminal session
```
Note: in interactive `ailsd` chat, `/sandbox` is a local CLI command, so use
`ailsd sandbox session-get --thread "$TID"` if you want binding metadata.

5. In another terminal, user can run commands in the exact same sandbox session:
```bash
SID=$(ailsd sandbox session-get --thread "$TID" | jq -r '.session_id')
ailsd sandbox session-exec --session "$SID" "echo from-user > /workspace/shared.txt"
echo "/exec cat /workspace/shared.txt" | ailsd --thread-id "$TID"
```

6. Optional interactive relay shell for the same session:
```bash
ailsd sandbox session-connect --session "$SID"
```
