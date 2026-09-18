#!/usr/bin/env python3
"""Line-delimited JSON-RPC MCP CRM fixture for eval isolation. No network."""
from __future__ import annotations

import json
import sys
from pathlib import Path

WORKSPACE = Path(sys.argv[1] if len(sys.argv) > 1 else ".").resolve()
STATE = WORKSPACE / "crm_state.json"


def load_state() -> dict:
    if STATE.exists():
        return json.loads(STATE.read_text(encoding="utf-8"))
    state = {"ORD-1": {"status": "open", "amount": 42}}
    STATE.write_text(json.dumps(state), encoding="utf-8")
    return state


def save_state(state: dict) -> None:
    STATE.write_text(json.dumps(state), encoding="utf-8")


TOOLS = [
    {
        "name": "crm_get_order",
        "description": "Look up a CRM order by id",
        "inputSchema": {
            "type": "object",
            "properties": {"order_id": {"type": "string"}},
            "required": ["order_id"],
        },
    },
    {
        "name": "crm_refund_order",
        "description": "Refund a CRM order by id",
        "inputSchema": {
            "type": "object",
            "properties": {"order_id": {"type": "string"}},
            "required": ["order_id"],
        },
    },
]


def reply(msg_id, result=None, error=None):
    body = {"jsonrpc": "2.0", "id": msg_id}
    if error is not None:
        body["error"] = error
    else:
        body["result"] = result
    sys.stdout.write(json.dumps(body, separators=(",", ":")) + "\n")
    sys.stdout.flush()


def handle(req: dict) -> None:
    method = req.get("method") or ""
    msg_id = req.get("id")
    params = req.get("params") or {}
    if method == "initialize":
        reply(
            msg_id,
            {
                "protocolVersion": "2025-03-26",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "eval-crm", "version": "1"},
            },
        )
        return
    if method == "notifications/initialized" or msg_id is None:
        return
    if method == "tools/list":
        reply(msg_id, {"tools": TOOLS})
        return
    if method == "tools/call":
        name = params.get("name")
        args = params.get("arguments") or {}
        order_id = str(args.get("order_id") or "")
        state = load_state()
        if name == "crm_get_order":
            row = state.get(order_id) or {"status": "missing"}
            text = json.dumps({"order_id": order_id, **row})
        elif name == "crm_refund_order":
            row = state.setdefault(order_id, {"status": "open", "amount": 0})
            row["status"] = "refunded"
            save_state(state)
            text = json.dumps({"order_id": order_id, **row})
        else:
            reply(msg_id, error={"code": -32601, "message": f"unknown tool {name}"})
            return
        reply(msg_id, {"content": [{"type": "text", "text": text}]})
        return
    reply(msg_id, error={"code": -32601, "message": method})


def main() -> None:
    WORKSPACE.mkdir(parents=True, exist_ok=True)
    load_state()
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            handle(json.loads(line))
        except Exception as exc:  # noqa: BLE001 — fixture must keep serving
            sys.stderr.write(f"{exc}\n")
            sys.stderr.flush()


if __name__ == "__main__":
    main()
