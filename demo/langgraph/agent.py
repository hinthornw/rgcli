from typing import TypedDict

from langgraph.graph import END, START, StateGraph


class State(TypedDict):
    ok: bool


def mark_ok(_: State) -> State:
    return {"ok": True}


workflow = StateGraph(State)
workflow.add_node("mark_ok", mark_ok)
workflow.add_edge(START, "mark_ok")
workflow.add_edge("mark_ok", END)
graph = workflow.compile()

