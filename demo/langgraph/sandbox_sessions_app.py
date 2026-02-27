"""SSAP MVP custom app scaffold using Starlette.

Security model for LangSmith sandbox:
- Server keeps LANGSMITH_API_KEY (never returned to client).
- Session endpoints issue short-lived JWT access tokens (default 60m).
- Relay endpoints validate JWT/session/principal before proxying to sandbox dataplane.
"""

from __future__ import annotations

import asyncio
import hashlib
import os
import secrets
from datetime import UTC, datetime, timedelta
from enum import Enum
from typing import Any, TypedDict
from uuid import uuid4

import httpx
import jwt
from starlette.applications import Starlette
from starlette.exceptions import HTTPException
from starlette.requests import Request
from starlette.responses import JSONResponse, Response, StreamingResponse
from starlette.routing import Route

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


def _caps() -> list[str]:
    raw = os.getenv("SSAP_CAPS", "execute,upload,download")
    items = [item.strip() for item in raw.split(",")]
    return [item for item in items if item]


def _langsmith_api_key() -> str:
    key = os.getenv("LANGSMITH_API_KEY", "").strip()
    if not key:
        raise _error(
            503,
            "BACKEND_UNAVAILABLE",
            "LANGSMITH_API_KEY is required for relay mode",
        )
    return key


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


def _langsmith_template_name() -> str | None:
    raw = os.getenv("LANGSMITH_SANDBOX_TEMPLATE")
    if raw is None:
        return None
    value = raw.strip()
    return value or None


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


_lock = asyncio.Lock()
_thread_to_session: dict[str, str] = {}
_sessions: dict[str, SessionRecord] = {}


def _require_enabled() -> None:
    if not _enabled():
        raise _error(404, "NOT_FOUND", "SSAP routes are disabled")


def _principal_id_from_request(request: Request) -> str:
    user = request.scope.get("user")
    if user is not None:
        identity = getattr(user, "identity", None) or getattr(user, "id", None)
        if identity:
            return f"user:{identity}"

    auth = request.headers.get("authorization", "")
    if auth:
        digest = hashlib.sha256(auth.encode("utf-8")).hexdigest()[:16]
        return f"auth:{digest}"

    return "anon:dev"


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
    if template_name is None:
        template_names = await _langsmith_list_template_names()
        if not template_names:
            raise _error(503, "BACKEND_UNAVAILABLE", "No sandbox templates available")
        template_name = template_names[0]

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
    token = _decode_bearer_token(request.headers.get("authorization"))
    claims = _claims_from_token(token)
    if claims.get("sid") != session_id:
        raise _error(403, "FORBIDDEN", "Token session mismatch")
    if required_cap is not None:
        _require_capability(claims, required_cap)

    async with _lock:
        record = _sessions.get(session_id)
        if record is None:
            raise _error(404, "SESSION_NOT_FOUND", f"Session '{session_id}' does not exist")
        if claims.get("sub") != record["principal_id"]:
            raise _error(403, "FORBIDDEN", "Token principal mismatch")
        if _now() > record["session_expires_at"]:
            raise _error(410, "SESSION_EXPIRED", "Session exceeded max lifetime")
        return record


async def acquire_sandbox_session(req: Request) -> JSONResponse:
    _require_enabled()
    thread_id, mode, sandbox_hint = _parse_acquire_request(await req.json())
    principal_id = _principal_id_from_request(req)

    async with _lock:
        scope_key = f"{principal_id}:{thread_id}"
        existing_session_id = _thread_to_session.get(scope_key)
        if existing_session_id is not None:
            existing = _sessions.get(existing_session_id)
            if existing is not None:
                token, token_exp = _issue_access_token(existing)
                return JSONResponse(_acquire_response(existing, req, token, token_exp))
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
    token, token_exp = _issue_access_token(record)

    async with _lock:
        scope_key = f"{principal_id}:{record['thread_id']}"
        _sessions[record["session_id"]] = record
        _thread_to_session[scope_key] = record["session_id"]
    return JSONResponse(_acquire_response(record, req, token, token_exp))


async def get_sandbox_session(req: Request) -> JSONResponse:
    _require_enabled()
    session_id = req.path_params["session_id"]
    principal_id = _principal_id_from_request(req)
    async with _lock:
        record = _sessions.get(session_id)
        if record is None:
            raise _error(404, "SESSION_NOT_FOUND", f"Session '{session_id}' does not exist")
        if record["principal_id"] != principal_id:
            raise _error(403, "FORBIDDEN", "Session principal mismatch")
        if _now() > record["session_expires_at"]:
            raise _error(410, "SESSION_EXPIRED", "Session exceeded max lifetime")
    token, token_exp = _issue_access_token(record)
    return JSONResponse(_acquire_response(record, req, token, token_exp))


async def refresh_sandbox_session(req: Request) -> JSONResponse:
    _require_enabled()
    session_id = req.path_params["session_id"]
    principal_id = _principal_id_from_request(req)
    async with _lock:
        record = _sessions.get(session_id)
        if record is None:
            raise _error(404, "SESSION_NOT_FOUND", f"Session '{session_id}' does not exist")
        if record["principal_id"] != principal_id:
            raise _error(403, "FORBIDDEN", "Session principal mismatch")
        if _now() > record["session_expires_at"]:
            raise _error(410, "SESSION_EXPIRED", "Session exceeded max lifetime")
    token, token_exp = _issue_access_token(record)
    return JSONResponse(_refresh_response(token, token_exp))


async def release_sandbox_session(req: Request) -> Response:
    _require_enabled()
    session_id = req.path_params["session_id"]
    principal_id = _principal_id_from_request(req)
    async with _lock:
        record = _sessions.get(session_id)
        if record is None:
            raise _error(404, "SESSION_NOT_FOUND", f"Session '{session_id}' does not exist")
        if record["principal_id"] != principal_id:
            raise _error(403, "FORBIDDEN", "Session principal mismatch")
        del _sessions[session_id]
        scope_key = f"{principal_id}:{record['thread_id']}"
        if _thread_to_session.get(scope_key) == session_id:
            del _thread_to_session[scope_key]
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


routes = [
    Route("/v1/sandbox/sessions", acquire_sandbox_session, methods=["POST"]),
    Route("/v1/sandbox/sessions/{session_id}", get_sandbox_session, methods=["GET"]),
    Route("/v1/sandbox/sessions/{session_id}/refresh", refresh_sandbox_session, methods=["POST"]),
    Route("/v1/sandbox/sessions/{session_id}", release_sandbox_session, methods=["DELETE"]),
    Route("/v1/sandbox/relay/{session_id}/execute", relay_execute, methods=["POST"]),
    Route("/v1/sandbox/relay/{session_id}/upload", relay_upload, methods=["POST"]),
    Route("/v1/sandbox/relay/{session_id}/download", relay_download, methods=["GET"]),
]


async def _handle_http_exception(_: Request, exc: HTTPException) -> JSONResponse:
    if isinstance(exc.detail, dict):
        return JSONResponse(exc.detail, status_code=exc.status_code, headers=exc.headers)
    return JSONResponse(
        {"error": {"code": "HTTP_ERROR", "message": str(exc.detail), "retryable": False}},
        status_code=exc.status_code,
        headers=exc.headers,
    )


app = Starlette(
    routes=routes,
    exception_handlers={
        HTTPException: _handle_http_exception,
    },
)
