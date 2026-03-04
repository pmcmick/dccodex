#!/usr/bin/env python3
"""Example hook that saves plan-mode proposed plans to Markdown.

Reads a Codex hook payload from argv[1] and appends a Markdown section when a
`proposed_plan` is present. Intended events:
  - after_model_response_completed
  - after_agent

Default output location:
  ~/.codex/hooks/plans/<git-branch>.md
Override base directory with:
  CODEX_PLAN_LOG_DIR=/path/to/plans
"""

from __future__ import annotations

import json
import os
import pathlib
import re
import subprocess
import sys
from typing import Any

DEFAULT_PLAN_DIR = "~/.codex/hooks/plans"
PLAN_DIR_ENV = "CODEX_PLAN_LOG_DIR"
SUPPORTED_EVENTS = {"after_model_response_completed", "after_agent"}


def branch_name(cwd: str) -> str:
    try:
        result = subprocess.run(
            ["git", "branch", "--show-current"],
            cwd=cwd,
            check=False,
            capture_output=True,
            text=True,
            timeout=1.0,
        )
    except Exception:
        return "unknown-branch"

    name = result.stdout.strip()
    if not name:
        return "unknown-branch"
    return name


def safe_name(name: str) -> str:
    normalized = re.sub(r"[^a-zA-Z0-9._-]+", "-", name).strip("-")
    return normalized or "unknown-branch"


def output_path(cwd: str) -> pathlib.Path:
    base = pathlib.Path(os.environ.get(PLAN_DIR_ENV, DEFAULT_PLAN_DIR)).expanduser()
    return base / f"{safe_name(branch_name(cwd))}.md"


def extract_plan(payload: dict[str, Any]) -> tuple[str, dict[str, Any]] | None:
    event = payload.get("hook_event", {})
    event_type = event.get("event_type")
    if event_type not in SUPPORTED_EVENTS:
        return None

    proposed_plan = event.get("proposed_plan")
    if not isinstance(proposed_plan, str):
        return None
    proposed_plan = proposed_plan.strip()
    if not proposed_plan:
        return None

    return proposed_plan, event


def append_markdown(path: pathlib.Path, payload: dict[str, Any], event: dict[str, Any], plan: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    triggered_at = payload.get("triggered_at", "unknown-time")
    event_type = event.get("event_type", "unknown-event")
    session_id = payload.get("session_id", "unknown-session")
    thread_id = event.get("thread_id", "unknown-thread")
    turn_id = event.get("turn_id", "unknown-turn")
    response_id = event.get("response_id")
    cwd = payload.get("cwd", "")

    with path.open("a", encoding="utf-8") as handle:
        handle.write(f"## Proposed Plan ({triggered_at})\n\n")
        handle.write(f"- event: {event_type}\n")
        handle.write(f"- session_id: {session_id}\n")
        handle.write(f"- thread_id: {thread_id}\n")
        handle.write(f"- turn_id: {turn_id}\n")
        if response_id:
            handle.write(f"- response_id: {response_id}\n")
        if cwd:
            handle.write(f"- cwd: {cwd}\n")
        handle.write("\n")
        handle.write(f"{plan}\n\n")


def main() -> int:
    if len(sys.argv) < 2:
        return 1

    try:
        payload = json.loads(sys.argv[1])
    except json.JSONDecodeError:
        return 1

    extracted = extract_plan(payload)
    if extracted is None:
        return 0
    plan, event = extracted

    cwd = str(payload.get("cwd") or os.getcwd())
    path = output_path(cwd)
    append_markdown(path, payload, event, plan)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
