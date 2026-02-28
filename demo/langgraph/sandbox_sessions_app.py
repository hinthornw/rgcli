"""SSAP MVP custom app scaffold using Starlette.

Security model for LangSmith sandbox:
- Server keeps LANGSMITH_API_KEY (never returned to client).
- Session endpoints issue short-lived JWT access tokens (default 60m).
- Relay endpoints validate JWT/session/principal before proxying to sandbox dataplane.
"""

from __future__ import annotations

import asyncio
import contextlib
import hashlib
import os
import secrets
from datetime import UTC, datetime, timedelta
from enum import Enum
from typing import Any, Mapping, TypedDict
from uuid import uuid4

import httpx
import jwt
import websockets
from langgraph_api.cache import cache_get, cache_set
from starlette.applications import Starlette
from starlette.exceptions import HTTPException
from starlette.requests import Request
from starlette.responses import JSONResponse, Response, StreamingResponse
from starlette.routing import Route, WebSocketRoute
from starlette.websockets import WebSocket, WebSocketDisconnect

try:
    from lsandbox_py import SandboxClient as RustSandboxClient
except Exception:  # pragma: no cover - import-time dependency issue
    RustSandboxClient = None


def _truthy(value: str | None) -> bool:
    if value is None:
        return False
    return value.strip().lower() in {"1", "true", "yes", "on"}


def _enabled() -> bool:
    # Enabled by default for this demo app.
    # Set SSAP_ENABLED=false to disable explicitly.
    raw = os.getenv("SSAP_ENABLED")
    if raw is None:
        return True
    return _truthy(raw)


def _now() -> datetime:
    return datetime.now(UTC)


def _iso(dt: datetime) -> str:
    return dt.isoformat().replace("+00:00", "Z")


def _session_id() -> str:
    return f"ssn_{uuid4().hex[:12]}"


def _ttl_minutes() -> int:
    raw = os.getenv("SSAP_TOKEN_TTL_MINUTES", "60")
    try:
        parsed = int(raw)
    except ValueError:
        return 60
    return max(1, parsed)


def _session_max_hours() -> int:
    raw = os.getenv("SSAP_SESSION_MAX_HOURS", "8")
    try:
        parsed = int(raw)
    except ValueError:
        return 8
    return max(1, parsed)


def _jwt_secret() -> str:
    return os.getenv("SSAP_JWT_SECRET", "dev-only-ssap-secret-change-me")


def _jwt_issuer() -> str:
    return os.getenv("SSAP_JWT_ISSUER", "ssap-demo")


def _provider() -> str:
    return os.getenv("SSAP_PROVIDER", "langsmith")


def _cache_prefix() -> str:
    return os.getenv("SSAP_CACHE_PREFIX", "ssap:mvp")


def _caps() -> list[str]:
    raw = os.getenv("SSAP_CAPS", "execute,upload,download")
    items = [item.strip() for item in raw.split(",")]
    return [item for item in items if item]


def _langsmith_api_key() -> str:
    for env_name in ("LANGSMITH_API_KEY", "LANGGRAPH_API_KEY", "LANGCHAIN_API_KEY"):
        key = os.getenv(env_name, "").strip()
        if key:
            return key
    raise _error(
        503,
        "BACKEND_UNAVAILABLE",
        "One of LANGSMITH_API_KEY, LANGGRAPH_API_KEY, or LANGCHAIN_API_KEY is required for relay mode",
    )


def _langsmith_control_base() -> str:
    return os.getenv(
        "LANGSMITH_SANDBOX_CONTROL_BASE",
        "https://api.smith.langchain.com/v2/sandboxes",
    ).rstrip("/")


def _langsmith_endpoint() -> str:
    endpoint = os.getenv("LANGSMITH_ENDPOINT", "").strip()
    if endpoint:
        return endpoint.rstrip("/")
    base = _langsmith_control_base()
    suffix = "/v2/sandboxes"
    if base.endswith(suffix):
        return base[: -len(suffix)]
    return "https://api.smith.langchain.com"


def _langsmith_template_name() -> str:
    raw = os.getenv("LANGSMITH_SANDBOX_TEMPLATE")
    if raw is None:
        return "ssap-default"
    value = raw.strip()
    if not value:
        return "ssap-default"
    return value


def _auto_create_template_enabled() -> bool:
    raw = os.getenv("SSAP_AUTO_CREATE_TEMPLATE")
    if raw is None:
        return True
    return _truthy(raw)


def _template_create_body(template_name: str) -> dict[str, Any]:
    image = os.getenv("SSAP_TEMPLATE_IMAGE", "python:3.12-slim").strip()

    body: dict[str, Any] = {
        "name": template_name,
        "image": image,
    }
    cpu = os.getenv("SSAP_TEMPLATE_CPU", "").strip()
    memory = os.getenv("SSAP_TEMPLATE_MEMORY", "").strip()
    storage = os.getenv("SSAP_TEMPLATE_STORAGE", "").strip()
    if cpu:
        body["cpu"] = cpu
    if memory:
        body["memory"] = memory
    if storage:
        body["storage"] = storage
    return body


async def _langsmith_list_template_names() -> list[str]:
    if RustSandboxClient is None:
        raise _error(
            503,
            "BACKEND_UNAVAILABLE",
            "lsandbox_py is not installed; run `uv sync` in demo/langgraph",
        )
    key = _langsmith_api_key()
    endpoint = _langsmith_endpoint()

    client = RustSandboxClient(api_key=key, endpoint=endpoint)
    try:
        names = await client.list_template_names()
        return list(names)
    except HTTPException:
        raise
    except Exception as exc:
        raise _error(503, "BACKEND_UNAVAILABLE", f"Failed to list templates: {exc}") from exc


async def _langsmith_create_template(template_name: str) -> None:
    if RustSandboxClient is None:
        raise RuntimeError("lsandbox_py is required to auto-create templates")
    key = _langsmith_api_key()
    endpoint = _langsmith_endpoint()
    body = _template_create_body(template_name)
    client = RustSandboxClient(api_key=key, endpoint=endpoint)
    try:
        await client.create_template(
            name=str(body["name"]),
            image=str(body["image"]),
            cpu=body.get("cpu"),
            memory=body.get("memory"),
            storage=body.get("storage"),
        )
        return
    except Exception as exc:
        message = str(exc).lower()
        if "409" in message or "already exists" in message or "conflict" in message:
            return
        raise RuntimeError(f"create template failed: {exc}") from exc


async def _ensure_template_on_startup() -> None:
    if not _auto_create_template_enabled():
        return
    template_name = _langsmith_template_name()
    names = await _langsmith_list_template_names()
    if template_name in names:
        return
    await _langsmith_create_template(template_name)


def _error(status: int, code: str, message: str) -> HTTPException:
    return HTTPException(
        status_code=status,
        detail={
            "error": {
                "code": code,
                "message": message,
                "retryable": status in {423, 429, 503},
            }
        },
    )


def _relay_http_base_url(request: Request, session_id: str) -> str:
    base = str(request.base_url).rstrip("/")
    return f"{base}/v1/sandbox/relay/{session_id}"


def _relay_ws_base_url(request: Request, session_id: str) -> str:
    base = str(request.base_url).rstrip("/")
    return f"{base.replace('http://', 'ws://').replace('https://', 'wss://')}/v1/sandbox/relay/{session_id}"


class SandboxSessionMode(str, Enum):
    get = "get"
    ensure = "ensure"


class SessionRecord(TypedDict):
    session_id: str
    thread_id: str
    principal_id: str
    sandbox_name: str
    provider: str
    dataplane_url: str
    created_at: datetime
    session_expires_at: datetime


def _parse_acquire_request(payload: Any) -> tuple[str, SandboxSessionMode, str | None]:
    if not isinstance(payload, dict):
        raise _error(400, "INVALID_REQUEST", "Request body must be a JSON object")

    thread_id = payload.get("thread_id")
    if not isinstance(thread_id, str) or not thread_id.strip():
        raise _error(400, "INVALID_REQUEST", "thread_id is required")

    mode_raw = payload.get("mode")
    if not isinstance(mode_raw, str):
        raise _error(400, "INVALID_REQUEST", "mode must be 'get' or 'ensure'")
    try:
        mode = SandboxSessionMode(mode_raw)
    except ValueError as exc:
        raise _error(400, "INVALID_REQUEST", "mode must be 'get' or 'ensure'") from exc

    sandbox_hint = payload.get("sandbox_hint")
    if sandbox_hint is not None and not isinstance(sandbox_hint, str):
        raise _error(400, "INVALID_REQUEST", "sandbox_hint must be a string when provided")

    return thread_id.strip(), mode, sandbox_hint


def _acquire_response(
    record: SessionRecord,
    request: Request,
    token: str,
    token_expires_at: datetime,
) -> dict[str, Any]:
    return {
        "session_id": record["session_id"],
        "thread_id": record["thread_id"],
        "sandbox": {
            "id": record["sandbox_name"],
            "provider": record["provider"],
            "http_base_url": _relay_http_base_url(request, record["session_id"]),
            "ws_base_url": _relay_ws_base_url(request, record["session_id"]),
        },
        "token": token,
        "expires_at": _iso(token_expires_at),
    }


def _refresh_response(token: str, token_expires_at: datetime) -> dict[str, str]:
    return {
        "token": token,
        "expires_at": _iso(token_expires_at),
    }


def _parse_iso(value: str) -> datetime:
    if value.endswith("Z"):
        value = f"{value[:-1]}+00:00"
    dt = datetime.fromisoformat(value)
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=UTC)
    return dt.astimezone(UTC)


def _ttl_until(expires_at: datetime) -> timedelta:
    seconds = int((expires_at - _now()).total_seconds())
    return timedelta(seconds=max(1, seconds))


def _binding_key(principal_id: str, thread_id: str) -> str:
    digest = hashlib.sha256(f"{principal_id}:{thread_id}".encode("utf-8")).hexdigest()
    return f"{_cache_prefix()}:binding:{digest}"


def _session_key(session_id: str) -> str:
    return f"{_cache_prefix()}:session:{session_id}"


def _record_to_payload(record: SessionRecord) -> dict[str, Any]:
    return {
        **record,
        "created_at": _iso(record["created_at"]),
        "session_expires_at": _iso(record["session_expires_at"]),
    }


def _record_from_payload(payload: Any) -> SessionRecord | None:
    if not isinstance(payload, dict):
        return None
    try:
        session_id = payload["session_id"]
        thread_id = payload["thread_id"]
        principal_id = payload["principal_id"]
        sandbox_name = payload["sandbox_name"]
        provider = payload["provider"]
        dataplane_url = payload["dataplane_url"]
        created_at = payload["created_at"]
        session_expires_at = payload["session_expires_at"]
    except KeyError:
        return None

    if not all(
        isinstance(item, str)
        for item in (
            session_id,
            thread_id,
            principal_id,
            sandbox_name,
            provider,
            dataplane_url,
            created_at,
            session_expires_at,
        )
    ):
        return None

    return {
        "session_id": session_id,
        "thread_id": thread_id,
        "principal_id": principal_id,
        "sandbox_name": sandbox_name,
        "provider": provider,
        "dataplane_url": dataplane_url,
        "created_at": _parse_iso(created_at),
        "session_expires_at": _parse_iso(session_expires_at),
    }


async def _load_session(session_id: str) -> SessionRecord | None:
    payload = await cache_get(_session_key(session_id))
    return _record_from_payload(payload)


async def _save_session(record: SessionRecord) -> None:
    await cache_set(
        _session_key(record["session_id"]),
        _record_to_payload(record),
        ttl=_ttl_until(record["session_expires_at"]),
    )


async def _load_bound_session_id(principal_id: str, thread_id: str) -> str | None:
    payload = await cache_get(_binding_key(principal_id, thread_id))
    if not isinstance(payload, dict):
        return None
    session_id = payload.get("session_id")
    if not isinstance(session_id, str) or not session_id:
        return None
    return session_id


async def _save_binding(principal_id: str, thread_id: str, session_id: str, expires_at: datetime) -> None:
    await cache_set(
        _binding_key(principal_id, thread_id),
        {"session_id": session_id},
        ttl=_ttl_until(expires_at),
    )


async def _clear_binding_and_session(record: SessionRecord) -> None:
    ttl = timedelta(seconds=1)
    await cache_set(_session_key(record["session_id"]), None, ttl=ttl)
    await cache_set(_binding_key(record["principal_id"], record["thread_id"]), None, ttl=ttl)


_lock = asyncio.Lock()


def _require_enabled() -> None:
    if not _enabled():
        raise _error(404, "NOT_FOUND", "SSAP routes are disabled")


def _principal_id_from_request(request: Request) -> str:
    user = request.scope.get("user")
    identity = None
    if user is not None:
        identity = getattr(user, "identity", None) or getattr(user, "id", None)
    if not isinstance(identity, str) or not identity.strip():
        # LangGraph noop auth mode does not provide a user identity.
        # Use a stable fallback principal so agent and client still bind
        # to the same shared session in local dev.
        return "client:anonymous"
    return identity.strip()


def _issue_access_token(record: SessionRecord) -> tuple[str, datetime]:
    now = _now()
    exp = now + timedelta(minutes=_ttl_minutes())
    payload = {
        "iss": _jwt_issuer(),
        "sub": record["principal_id"],
        "sid": record["session_id"],
        "thread_id": record["thread_id"],
        "sandbox_id": record["sandbox_name"],
        "caps": _caps(),
        "iat": int(now.timestamp()),
        "exp": int(exp.timestamp()),
        "jti": secrets.token_urlsafe(12),
    }
    token = jwt.encode(payload, _jwt_secret(), algorithm="HS256")
    return token, exp


def _decode_bearer_token(auth_header: str | None) -> str:
    if not auth_header:
        raise _error(401, "UNAUTHENTICATED", "Missing Authorization header")
    parts = auth_header.split(" ", 1)
    if len(parts) != 2 or parts[0].lower() != "bearer":
        raise _error(401, "UNAUTHENTICATED", "Expected Bearer token")
    return parts[1].strip()


def _decode_access_token(headers: Mapping[str, str]) -> str:
    auth_header = headers.get("authorization")
    if auth_header:
        return _decode_bearer_token(auth_header)
    api_key = headers.get("x-api-key")
    if isinstance(api_key, str) and api_key.strip():
        return api_key.strip()
    raise _error(401, "UNAUTHENTICATED", "Missing access token")


def _claims_from_token(token: str) -> dict:
    try:
        claims = jwt.decode(
            token,
            _jwt_secret(),
            algorithms=["HS256"],
            issuer=_jwt_issuer(),
            options={"require": ["exp", "iat", "sid", "sub"]},
        )
    except jwt.ExpiredSignatureError as exc:
        raise _error(401, "TOKEN_EXPIRED", "Token expired") from exc
    except jwt.InvalidTokenError as exc:
        raise _error(401, "UNAUTHENTICATED", "Invalid token") from exc
    return claims


def _require_capability(claims: dict, cap: str) -> None:
    caps = claims.get("caps", [])
    if cap not in caps:
        raise _error(403, "CAPABILITY_DENIED", f"Missing capability: {cap}")


async def _langsmith_get_box(name: str) -> dict:
    if RustSandboxClient is None:
        raise _error(
            503,
            "BACKEND_UNAVAILABLE",
            "lsandbox_py is not installed; run `uv sync` in demo/langgraph",
        )
    key = _langsmith_api_key()
    endpoint = _langsmith_endpoint()

    client = RustSandboxClient(api_key=key, endpoint=endpoint)
    try:
        payload = await client.get_sandbox(name)
        return dict(payload)
    except HTTPException:
        raise
    except Exception as exc:
        message = str(exc)
        if "not found" in message.lower():
            raise _error(404, "SESSION_NOT_FOUND", f"Sandbox '{name}' not found") from exc
        raise _error(503, "BACKEND_UNAVAILABLE", f"Failed to get sandbox: {message}") from exc


async def _langsmith_create_box(name_hint: str | None = None) -> dict:
    if RustSandboxClient is None:
        raise _error(
            503,
            "BACKEND_UNAVAILABLE",
            "lsandbox_py is not installed; run `uv sync` in demo/langgraph",
        )
    key = _langsmith_api_key()
    endpoint = _langsmith_endpoint()
    template_name = _langsmith_template_name()

    client = RustSandboxClient(api_key=key, endpoint=endpoint)
    try:
        payload = await client.create_sandbox(template_name=template_name, name=name_hint)
        return dict(payload)
    except HTTPException:
        raise
    except Exception as exc:
        raise _error(503, "BACKEND_UNAVAILABLE", f"Failed to create sandbox: {exc}") from exc


def _sandbox_name_from_payload(payload: dict) -> str:
    name = payload.get("name")
    if not isinstance(name, str) or not name:
        raise _error(503, "BACKEND_UNAVAILABLE", "Sandbox payload missing name")
    return name


def _sandbox_dataplane_from_payload(payload: dict) -> str:
    url = payload.get("dataplane_url")
    if not isinstance(url, str) or not url:
        raise _error(503, "BACKEND_UNAVAILABLE", "Sandbox missing dataplane_url")
    return url.rstrip("/")


async def _require_session_for_principal(
    request: Request,
    session_id: str,
    required_cap: str | None = None,
) -> SessionRecord:
    token = _decode_access_token(request.headers)
    return await _require_session_for_token(token, session_id, required_cap)


async def _require_session_for_token(
    token: str,
    session_id: str,
    required_cap: str | None = None,
) -> SessionRecord:
    claims = _claims_from_token(token)
    if claims.get("sid") != session_id:
        raise _error(403, "FORBIDDEN", "Token session mismatch")
    if required_cap is not None:
        _require_capability(claims, required_cap)

    record = await _load_session(session_id)
    if record is None:
        raise _error(404, "SESSION_NOT_FOUND", f"Session '{session_id}' does not exist")
    if claims.get("sub") != record["principal_id"]:
        raise _error(403, "FORBIDDEN", "Token principal mismatch")
    if _now() > record["session_expires_at"]:
        raise _error(410, "SESSION_EXPIRED", "Session exceeded max lifetime")
    return record


def _dataplane_ws_execute_url(dataplane_url: str) -> str:
    ws_base = dataplane_url.replace("https://", "wss://").replace("http://", "ws://")
    return f"{ws_base.rstrip('/')}/execute/ws"


async def ensure_session_record(
    principal_id: str,
    thread_id: str,
    mode: SandboxSessionMode,
    sandbox_hint: str | None = None,
) -> SessionRecord:
    async with _lock:
        existing_session_id = await _load_bound_session_id(principal_id, thread_id)
        if existing_session_id is not None:
            existing = await _load_session(existing_session_id)
            if existing is not None and _now() <= existing["session_expires_at"]:
                return existing
        if mode is SandboxSessionMode.get:
            raise _error(
                404,
                "SESSION_NOT_FOUND",
                f"No sandbox session exists for thread '{thread_id}'",
            )

    if sandbox_hint:
        sandbox = await _langsmith_get_box(sandbox_hint)
    else:
        sandbox = await _langsmith_create_box()

    record: SessionRecord = {
        "session_id": _session_id(),
        "thread_id": thread_id,
        "principal_id": principal_id,
        "sandbox_name": _sandbox_name_from_payload(sandbox),
        "provider": _provider(),
        "dataplane_url": _sandbox_dataplane_from_payload(sandbox),
        "created_at": _now(),
        "session_expires_at": _now() + timedelta(hours=_session_max_hours()),
    }

    async with _lock:
        await _save_session(record)
        await _save_binding(principal_id, thread_id, record["session_id"], record["session_expires_at"])
    return record


async def get_owned_session_record(principal_id: str, session_id: str) -> SessionRecord:
    record = await _load_session(session_id)
    if record is None:
        raise _error(404, "SESSION_NOT_FOUND", f"Session '{session_id}' does not exist")
    if record["principal_id"] != principal_id:
        raise _error(403, "FORBIDDEN", "Session principal mismatch")
    if _now() > record["session_expires_at"]:
        raise _error(410, "SESSION_EXPIRED", "Session exceeded max lifetime")
    return record


async def acquire_sandbox_session(req: Request) -> JSONResponse:
    _require_enabled()
    thread_id, mode, sandbox_hint = _parse_acquire_request(await req.json())
    principal_id = _principal_id_from_request(req)
    record = await ensure_session_record(principal_id, thread_id, mode, sandbox_hint)
    token, token_exp = _issue_access_token(record)
    return JSONResponse(_acquire_response(record, req, token, token_exp))


async def get_sandbox_session(req: Request) -> JSONResponse:
    _require_enabled()
    session_id = req.path_params["session_id"]
    principal_id = _principal_id_from_request(req)
    record = await get_owned_session_record(principal_id, session_id)
    token, token_exp = _issue_access_token(record)
    return JSONResponse(_acquire_response(record, req, token, token_exp))


async def refresh_sandbox_session(req: Request) -> JSONResponse:
    _require_enabled()
    session_id = req.path_params["session_id"]
    principal_id = _principal_id_from_request(req)
    record = await get_owned_session_record(principal_id, session_id)
    token, token_exp = _issue_access_token(record)
    await _save_session(record)
    await _save_binding(principal_id, record["thread_id"], session_id, record["session_expires_at"])
    return JSONResponse(_refresh_response(token, token_exp))


async def release_sandbox_session(req: Request) -> Response:
    _require_enabled()
    session_id = req.path_params["session_id"]
    principal_id = _principal_id_from_request(req)
    record = await get_owned_session_record(principal_id, session_id)
    await _clear_binding_and_session(record)
    return Response(status_code=204)


async def relay_execute(req: Request) -> Response:
    _require_enabled()
    session_id = req.path_params["session_id"]
    record = await _require_session_for_principal(req, session_id, required_cap="execute")
    body = await req.body()
    key = _langsmith_api_key()
    url = f"{record['dataplane_url']}/execute"
    async with httpx.AsyncClient(timeout=120) as client:
        resp = await client.post(
            url,
            headers={"X-Api-Key": key, "Content-Type": "application/json"},
            content=body,
        )
    return Response(
        content=resp.content,
        status_code=resp.status_code,
        media_type=resp.headers.get("content-type", "application/json"),
    )


async def relay_upload(req: Request) -> Response:
    _require_enabled()
    session_id = req.path_params["session_id"]
    path = req.query_params.get("path")
    if not path:
        raise _error(400, "INVALID_REQUEST", "Query parameter 'path' is required")
    record = await _require_session_for_principal(req, session_id, required_cap="upload")
    body = await req.body()
    key = _langsmith_api_key()
    url = f"{record['dataplane_url']}/upload"
    content_type = req.headers.get("content-type", "application/octet-stream")
    async with httpx.AsyncClient(timeout=120) as client:
        resp = await client.post(
            url,
            params={"path": path},
            headers={"X-Api-Key": key, "Content-Type": content_type},
            content=body,
        )
    return Response(
        content=resp.content,
        status_code=resp.status_code,
        media_type=resp.headers.get("content-type", "application/json"),
    )


async def relay_download(req: Request) -> StreamingResponse:
    _require_enabled()
    session_id = req.path_params["session_id"]
    path = req.query_params.get("path")
    if not path:
        raise _error(400, "INVALID_REQUEST", "Query parameter 'path' is required")
    record = await _require_session_for_principal(req, session_id, required_cap="download")
    key = _langsmith_api_key()
    url = f"{record['dataplane_url']}/download"
    async with httpx.AsyncClient(timeout=120) as client:
        resp = await client.get(url, params={"path": path}, headers={"X-Api-Key": key})
    return StreamingResponse(
        iter([resp.content]),
        status_code=resp.status_code,
        media_type=resp.headers.get("content-type", "application/octet-stream"),
    )


async def relay_execute_ws(websocket: WebSocket) -> None:
    _require_enabled()
    session_id = websocket.path_params["session_id"]
    await websocket.accept()

    try:
        token = _decode_access_token(websocket.headers)
        record = await _require_session_for_token(token, session_id, required_cap="execute")
    except HTTPException as exc:
        payload = exc.detail if isinstance(exc.detail, dict) else {"error": {"message": str(exc.detail)}}
        with contextlib.suppress(Exception):
            await websocket.send_json(payload)
            await websocket.close(code=4401)
        return

    upstream_url = _dataplane_ws_execute_url(record["dataplane_url"])
    upstream_headers = {"X-Api-Key": _langsmith_api_key()}

    async def _client_to_upstream(upstream: Any) -> None:
        try:
            while True:
                message = await websocket.receive()
                msg_type = message.get("type")
                if msg_type == "websocket.disconnect":
                    with contextlib.suppress(Exception):
                        await upstream.close()
                    return
                if msg_type != "websocket.receive":
                    continue
                text = message.get("text")
                data = message.get("bytes")
                if text is not None:
                    await upstream.send(text)
                elif data is not None:
                    await upstream.send(data)
        except WebSocketDisconnect:
            with contextlib.suppress(Exception):
                await upstream.close()

    async def _upstream_to_client(upstream: Any) -> None:
        async for message in upstream:
            if isinstance(message, str):
                await websocket.send_text(message)
            else:
                await websocket.send_bytes(message)

    try:
        try:
            upstream_ctx = websockets.connect(
                upstream_url,
                additional_headers=upstream_headers,
                max_size=None,
            )
        except TypeError:
            upstream_ctx = websockets.connect(
                upstream_url,
                extra_headers=upstream_headers,
                max_size=None,
            )

        async with upstream_ctx as upstream:
            tasks = [
                asyncio.create_task(_client_to_upstream(upstream)),
                asyncio.create_task(_upstream_to_client(upstream)),
            ]
            done, pending = await asyncio.wait(tasks, return_when=asyncio.FIRST_COMPLETED)
            for task in pending:
                task.cancel()
            await asyncio.gather(*done, *pending, return_exceptions=True)
    except Exception as exc:
        with contextlib.suppress(Exception):
            await websocket.send_json(
                {
                    "type": "error",
                    "error_type": "RelayError",
                    "error": f"relay websocket failed: {exc}",
                }
            )
    finally:
        with contextlib.suppress(Exception):
            await websocket.close()


routes = [
    Route("/v1/sandbox/sessions", acquire_sandbox_session, methods=["POST"]),
    Route("/v1/sandbox/sessions/{session_id}", get_sandbox_session, methods=["GET"]),
    Route("/v1/sandbox/sessions/{session_id}/refresh", refresh_sandbox_session, methods=["POST"]),
    Route("/v1/sandbox/sessions/{session_id}", release_sandbox_session, methods=["DELETE"]),
    Route("/v1/sandbox/relay/{session_id}/execute", relay_execute, methods=["POST"]),
    Route("/v1/sandbox/relay/{session_id}/upload", relay_upload, methods=["POST"]),
    Route("/v1/sandbox/relay/{session_id}/download", relay_download, methods=["GET"]),
    WebSocketRoute("/v1/sandbox/relay/{session_id}/execute/ws", relay_execute_ws),
]


async def _handle_http_exception(_: Request, exc: HTTPException) -> JSONResponse:
    if isinstance(exc.detail, dict):
        return JSONResponse(exc.detail, status_code=exc.status_code, headers=exc.headers)
    return JSONResponse(
        {"error": {"code": "HTTP_ERROR", "message": str(exc.detail), "retryable": False}},
        status_code=exc.status_code,
        headers=exc.headers,
    )


@contextlib.asynccontextmanager
async def _lifespan(_: Starlette):
    if _enabled():
        await _ensure_template_on_startup()
    yield


app = Starlette(
    routes=routes,
    lifespan=_lifespan,
    exception_handlers={
        HTTPException: _handle_http_exception,
    },
)
