#!/usr/bin/env python3
"""anvil hammer — stock CPython guest. One JSON line in, one JSON line out."""

from __future__ import annotations

import ast
import io
import json
import os
import pickle
import sys
import traceback
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from typing import Any

STORE = Path(os.environ.get("ANVIL_STORE", "")).expanduser()
NAMESPACE_PATH = STORE / "namespace.pkl" if STORE else None

# Isolated user namespace. Not the process globals.
NS: dict[str, Any] = {"__name__": "__hammer__"}


def _load() -> None:
    if NAMESPACE_PATH is None or not NAMESPACE_PATH.is_file():
        return
    try:
        with NAMESPACE_PATH.open("rb") as fh:
            loaded = pickle.load(fh)
        if isinstance(loaded, dict):
            NS.clear()
            NS.update(loaded)
            NS.setdefault("__name__", "__hammer__")
    except Exception as exc:  # noqa: BLE001 — guest must start even if pickle is junk
        sys.stderr.write(f"hammer: failed to load namespace: {exc}\n")


def _persist() -> list[str]:
    if NAMESPACE_PATH is None:
        return []
    dropped: list[str] = []
    payload: dict[str, Any] = {}
    for key, value in NS.items():
        try:
            pickle.dumps(value)
        except Exception:
            dropped.append(key)
            continue
        payload[key] = value
    NAMESPACE_PATH.parent.mkdir(parents=True, exist_ok=True)
    tmp = NAMESPACE_PATH.with_suffix(".pkl.tmp")
    with tmp.open("wb") as fh:
        pickle.dump(payload, fh, protocol=pickle.HIGHEST_PROTOCOL)
    tmp.replace(NAMESPACE_PATH)
    return dropped


def _jsonable(value: Any) -> Any:
    if value is None:
        return None
    try:
        json.dumps(value)
        return value
    except TypeError:
        return {"repr": repr(value), "type": type(value).__qualname__}


def _split_last_expr(code: str) -> tuple[str | None, ast.expr | None]:
    tree = ast.parse(code, filename="<strike>", mode="exec")
    if not tree.body:
        return code, None
    last = tree.body[-1]
    if not isinstance(last, ast.Expr):
        return code, None
    prelude = ast.Module(body=tree.body[:-1], type_ignores=[])
    prelude_src = ast.unparse(prelude) if prelude.body else None
    return prelude_src, last.value


def _strike(code: str) -> dict[str, Any]:
    stdout = io.StringIO()
    stderr = io.StringIO()
    value: Any = None
    try:
        prelude, expr = _split_last_expr(code)
        with redirect_stdout(stdout), redirect_stderr(stderr):
            if prelude:
                exec(compile(prelude, "<strike>", "exec"), NS, NS)
            if expr is not None:
                value = eval(compile(ast.Expression(expr), "<strike>", "eval"), NS, NS)
        dropped = _persist()
        if dropped:
            stderr.write(
                "hammer: dropped unpicklable names: " + ", ".join(dropped) + "\n"
            )
        return {
            "ok": True,
            "value": _jsonable(value) if expr is not None else None,
            "stdout": stdout.getvalue(),
            "stderr": stderr.getvalue(),
            "error": None,
        }
    except SyntaxError:
        return {
            "ok": False,
            "value": None,
            "stdout": stdout.getvalue(),
            "stderr": stderr.getvalue(),
            "error": traceback.format_exc(),
        }
    except Exception:
        _persist()
        return {
            "ok": False,
            "value": None,
            "stdout": stdout.getvalue(),
            "stderr": stderr.getvalue(),
            "error": traceback.format_exc(),
        }


def _handle(msg: dict[str, Any]) -> dict[str, Any]:
    mid = msg.get("id", "")
    op = msg.get("op", "")
    if op == "ping":
        return {"id": mid, "ok": True, "value": "pong", "stdout": "", "stderr": "", "error": None}
    if op == "shutdown":
        return {"id": mid, "ok": True, "value": None, "stdout": "", "stderr": "", "error": None}
    if op == "reset":
        NS.clear()
        NS["__name__"] = "__hammer__"
        if NAMESPACE_PATH is not None and NAMESPACE_PATH.exists():
            NAMESPACE_PATH.unlink()
        return {"id": mid, "ok": True, "value": None, "stdout": "", "stderr": "", "error": None}
    if op == "strike":
        result = _strike(str(msg.get("code", "")))
        result["id"] = mid
        return result
    return {
        "id": mid,
        "ok": False,
        "value": None,
        "stdout": "",
        "stderr": "",
        "error": f"unknown op: {op!r}",
    }


def _self_test() -> int:
    global STORE, NAMESPACE_PATH, NS
    import tempfile

    with tempfile.TemporaryDirectory() as tmp:
        STORE = Path(tmp)
        NAMESPACE_PATH = STORE / "namespace.pkl"
        NS = {"__name__": "__hammer__"}
        r = _strike("x = 1\nx + 1")
        assert r["ok"] and r["value"] == 2, r
        NS = {"__name__": "__hammer__"}
        _load()
        r = _strike("x")
        assert r["ok"] and r["value"] == 1, r
        r = _strike("print('hi')")
        assert r["ok"] and r["stdout"] == "hi\n" and r["value"] is None, r
        r = _strike("1/0")
        assert not r["ok"] and "ZeroDivisionError" in (r["error"] or ""), r
    print("hammer self-test ok")
    return 0


def main() -> int:
    if "--self-test" in sys.argv:
        return _self_test()
    if STORE:
        STORE.mkdir(parents=True, exist_ok=True)
    _load()
    # Unbuffered line protocol. Parent talks newline-JSON.
    sys.stdout.reconfigure(line_buffering=True)
    sys.stderr.reconfigure(line_buffering=True)
    for raw in sys.stdin:
        line = raw.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError as exc:
            sys.stdout.write(
                json.dumps(
                    {
                        "id": "",
                        "ok": False,
                        "value": None,
                        "stdout": "",
                        "stderr": "",
                        "error": f"bad json: {exc}",
                    }
                )
                + "\n"
            )
            continue
        reply = _handle(msg)
        sys.stdout.write(json.dumps(reply) + "\n")
        if msg.get("op") == "shutdown":
            return 0
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
