#!/usr/bin/env python3
"""Example hook for notify_on_model_response_completed.

Reads the Codex hook payload from argv[1] and appends one JSON object per
completed model response to a JSONL file. This is intended for lightweight
token-usage telemetry pipelines.
"""

from __future__ import annotations

import json
import os
import pathlib
import sys
from typing import Any

DEFAULT_LOG_PATH = "~/.codex/hooks/model_response_usage.jsonl"
LOG_PATH_ENV = "CODEX_MODEL_RESPONSE_USAGE_LOG"
TARGET_EVENT = "after_model_response_completed"


def usage_int(usage: dict[str, Any], key: str) -> int:
    value = usage.get(key, 0)
    if isinstance(value, int):
        return value
    try:
        return int(value)
    except (TypeError, ValueError):
        return 0


def build_record(payload: dict[str, Any]) -> dict[str, Any] | None:
    event = payload.get("hook_event", {})
    if event.get("event_type") != TARGET_EVENT:
        return None

    usage = event.get("token_usage") or {}
    input_tokens = usage_int(usage, "input_tokens")
    cached_input_tokens = usage_int(usage, "cached_input_tokens")
    output_tokens = usage_int(usage, "output_tokens")
    reasoning_output_tokens = usage_int(usage, "reasoning_output_tokens")
    total_tokens = usage_int(usage, "total_tokens")
    non_cached_input_tokens = max(input_tokens - cached_input_tokens, 0)

    return {
        "triggered_at": payload.get("triggered_at"),
        "session_id": payload.get("session_id"),
        "thread_id": event.get("thread_id"),
        "turn_id": event.get("turn_id"),
        "response_id": event.get("response_id"),
        "client": payload.get("client"),
        "cwd": payload.get("cwd"),
        "can_append": bool(event.get("can_append", False)),
        "needs_follow_up": bool(event.get("needs_follow_up", False)),
        "input_tokens": input_tokens,
        "cached_input_tokens": cached_input_tokens,
        "non_cached_input_tokens": non_cached_input_tokens,
        "output_tokens": output_tokens,
        "reasoning_output_tokens": reasoning_output_tokens,
        "total_tokens": total_tokens,
    }


def output_path() -> pathlib.Path:
    configured = os.environ.get(LOG_PATH_ENV, DEFAULT_LOG_PATH)
    return pathlib.Path(configured).expanduser()


def main() -> int:
    if len(sys.argv) < 2:
        return 1

    try:
        payload = json.loads(sys.argv[1])
    except json.JSONDecodeError:
        return 1

    record = build_record(payload)
    if record is None:
        return 0

    path = output_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(record, sort_keys=True))
        handle.write("\n")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
