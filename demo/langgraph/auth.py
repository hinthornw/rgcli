"""Custom auth for SSAP demo.

Authentication modes:
- SSAP relay/session JWTs (issued by sandbox_sessions_app) are accepted.
- Client bearer tokens are accepted; optionally restrict with SSAP_CLIENT_BEARER_TOKENS.
"""

from __future__ import annotations

import hashlib
import os
from typing import Any

import jwt
from jwt.exceptions import InvalidTokenError
from langgraph_sdk import Auth
from starlette.exceptions import HTTPException

auth = Auth()


def _jwt_secret() -> str:
    return os.getenv("SSAP_JWT_SECRET", "dev-only-ssap-secret-change-me")


def _jwt_issuer() -> str:
    return os.getenv("SSAP_JWT_ISSUER", "ssap-demo")


def _allowed_client_tokens() -> set[str]:
    raw = os.getenv("SSAP_CLIENT_BEARER_TOKENS", "").strip()
    if not raw:
        return set()
    return {item.strip() for item in raw.split(",") if item.strip()}


def _extract_token(
    authorization: str | None,
    headers: dict[bytes, bytes] | None,
) -> str:
    if authorization and authorization.lower().startswith("bearer "):
        token = authorization.split(" ", 1)[1].strip()
        if token:
            return token

    if headers is not None:
        api_key = headers.get(b"x-api-key", b"")
        if api_key:
            token = api_key.decode("utf-8", errors="ignore").strip()
            if token:
                return token

    raise HTTPException(
        status_code=401,
        detail="Missing bearer token or x-api-key",
        headers={"WWW-Authenticate": "Bearer"},
    )


def _client_identity_from_token(token: str) -> str:
    digest = hashlib.sha256(token.encode("utf-8")).hexdigest()[:24]
    return f"client:{digest}"


@auth.authenticate
async def authenticate(
    authorization: str | None = None,
    headers: dict[bytes, bytes] | None = None,
) -> dict[str, Any]:
    token = _extract_token(authorization, headers)

    # 1) Accept SSAP JWTs for relay/authenticated sandbox calls.
    try:
        claims = jwt.decode(
            token,
            _jwt_secret(),
            algorithms=["HS256"],
            issuer=_jwt_issuer(),
            options={"require": ["exp", "iat", "sub", "sid"]},
        )
        sub = claims.get("sub")
        sid = claims.get("sid")
        if isinstance(sub, str) and sub and isinstance(sid, str) and sid:
            return {
                "identity": sub,
                "auth_type": "ssap_session_jwt",
                "session_id": sid,
            }
    except InvalidTokenError:
        pass

    # 2) Accept client bearer token (optionally allowlist).
    allowed = _allowed_client_tokens()
    if allowed and token not in allowed:
        raise HTTPException(
            status_code=401,
            detail="Invalid client bearer token",
            headers={"WWW-Authenticate": "Bearer"},
        )
    return {
        "identity": _client_identity_from_token(token),
        "auth_type": "client_bearer",
    }
