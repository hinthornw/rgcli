from __future__ import annotations

from typing import Annotated, Any, TypedDict

import httpx
from langchain_core.messages import AIMessage, BaseMessage
from langchain_core.runnables import RunnableConfig
from langgraph.graph import END, START, StateGraph
from langgraph.graph.message import add_messages

from sandbox_sessions_app import (
    SandboxSessionMode,
    _langsmith_api_key,
    ensure_session_record,
)


class State(TypedDict):
    messages: Annotated[list[BaseMessage], add_messages]


def _latest_user_text(messages: list[BaseMessage]) -> str:
    for message in reversed(messages):
        if getattr(message, "type", "") in {"human", "user"}:
            content = getattr(message, "content", "")
            if isinstance(content, str):
                return content
            if isinstance(content, list):
                parts: list[str] = []
                for item in content:
                    if isinstance(item, str):
                        parts.append(item)
                    elif isinstance(item, dict):
                        text = item.get("text")
                        if isinstance(text, str):
                            parts.append(text)
                return "".join(parts)
            return str(content)
    return ""


def _principal_id_from_config(config: RunnableConfig) -> str:
    configurable = config.get("configurable", {})
    principal = configurable.get("langgraph_auth_user_id")
    if isinstance(principal, str) and principal:
        return principal
    user_obj = configurable.get("langgraph_auth_user")
    if isinstance(user_obj, dict):
        identity = user_obj.get("identity") or user_obj.get("id")
        if isinstance(identity, str) and identity:
            return identity
    return "client:anonymous"


def _thread_id_from_config(config: RunnableConfig) -> str:
    configurable = config.get("configurable", {})
    thread_id = configurable.get("thread_id")
    if isinstance(thread_id, str) and thread_id:
        return thread_id
    raise ValueError("Missing thread_id in run config")


async def _execute(dataplane_url: str, command: str) -> dict[str, Any]:
    payload = {"command": command}
    url = f"{dataplane_url.rstrip('/')}/execute"
    api_key = _langsmith_api_key()
    async with httpx.AsyncClient(timeout=120) as client:
        resp = await client.post(url, json=payload, headers={"X-Api-Key": api_key})
    resp.raise_for_status()
    parsed = resp.json()
    if not isinstance(parsed, dict):
        raise ValueError("Unexpected dataplane response payload")
    return parsed


def _format_exec_result(command: str, sandbox_id: str, result: dict[str, Any]) -> str:
    stdout = str(result.get("stdout", ""))
    stderr = str(result.get("stderr", ""))
    exit_code = int(result.get("exit_code", -1))
    return (
        f"$ {command}\n"
        f"sandbox: {sandbox_id}\n"
        f"exit_code: {exit_code}\n\n"
        f"stdout:\n{stdout or '(empty)'}\n\n"
        f"stderr:\n{stderr or '(empty)'}"
    )


async def respond(state: State, config: RunnableConfig) -> State:
    try:
        principal_id = _principal_id_from_config(config)
        thread_id = _thread_id_from_config(config)
    except ValueError as exc:
        return {"messages": [AIMessage(content=f"Configuration error: {exc}")]}

    session = await ensure_session_record(
        principal_id=principal_id,
        thread_id=thread_id,
        mode=SandboxSessionMode.ensure,
    )

    user_text = _latest_user_text(state.get("messages", [])).strip()
    if user_text.startswith("/exec "):
        command = user_text[len("/exec ") :].strip()
        if not command:
            return {"messages": [AIMessage(content="Usage: /exec <shell command>")]}
        try:
            result = await _execute(session["dataplane_url"], command)
            return {
                "messages": [
                    AIMessage(
                        content=_format_exec_result(
                            command=command,
                            sandbox_id=session["sandbox_name"],
                            result=result,
                        )
                    )
                ]
            }
        except Exception as exc:
            return {"messages": [AIMessage(content=f"Sandbox command failed: {exc}")]}

    if user_text == "/sandbox":
        return {
            "messages": [
                AIMessage(
                    content=(
                        f"sandbox ready\n"
                        f"id: {session['sandbox_name']}\n"
                        f"provider: {session['provider']}\n"
                        f"thread_id: {thread_id}\n"
                        f"principal: {principal_id}\n"
                        "Use `/exec <command>` to run commands."
                    )
                )
            ]
        }

    return {
        "messages": [
            AIMessage(
                content=(
                    "Sandbox is attached for this thread.\n"
                    "Use `/sandbox` to view details or `/exec <command>` to run commands."
                )
            )
        ]
    }


workflow = StateGraph(State)
workflow.add_node("respond", respond)
workflow.add_edge(START, "respond")
workflow.add_edge("respond", END)
graph = workflow.compile()
