# SSAP Demo (LangGraph)

## Setup

```bash
cd demo/langgraph
uv sync
```

`uv sync` builds and installs the local Rust PyO3 extension `lsandbox-py` from `../../crates/langsmith-sandbox-py`.

## Run

```bash
uv run langgraph dev --config ./langgraph-ssap-mvp.json --no-browser
```

## Notes

- This is a development-only SSAP MVP scaffold.
- `LANGSMITH_API_KEY` must be set for session creation and relay mode endpoints.
- `LANGSMITH_SANDBOX_TEMPLATE` is optional. If unset, the app auto-selects the first available template from `GET /v2/sandboxes/templates`.
- Sandbox control-plane operations (template list/create/get) are executed through the Rust client via `lsandbox_py`.
- Set `SSAP_ENABLED=false` to disable SSAP routes.

## Quick Check

```bash
curl -s -X POST http://127.0.0.1:2024/v1/sandbox/sessions \
  -H "content-type: application/json" \
  -d '{"thread_id":"thr_demo","mode":"ensure"}'
```
