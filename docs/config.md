# Configuration

For basic configuration instructions, see [this documentation](https://developers.openai.com/codex/config-basic).

For advanced configuration instructions, see [this documentation](https://developers.openai.com/codex/config-advanced).

For a full configuration reference, see [this documentation](https://developers.openai.com/codex/config-reference).

## Home directory resolution

By default, `codex` uses `~/.codex`, while `dccodex` uses `~/.dccodex`.

Override order:
- `CODEX_HOME` (always highest priority for both binaries)
- `DCCODEX_HOME` (used only when running `dccodex`)
- default home directory (`~/.codex` or `~/.dccodex`)

## Project-local config

In addition to `~/.codex/config.toml`, Codex supports project-local config files at `.codex/config.toml`.
When both are present, project-local values override user-level values.
`dccodex` uses `.dccodex/config.toml`.

Codex discovers project-local config files between your current working directory and the project root (`.codex/config.toml` for `codex`, `.dccodex/config.toml` for `dccodex`), and applies them in order so the closest directory to your `cwd` has the highest precedence.

Project-local config is only applied for trusted projects. Add trust in
`~/.codex/config.toml` (or `~/.dccodex/config.toml` when running `dccodex`):

```toml
[projects."/absolute/path/to/project"]
trust_level = "trusted"
```

## Connecting to MCP servers

Codex can connect to MCP servers configured in `~/.codex/config.toml`
(`~/.dccodex/config.toml` for `dccodex`). See the configuration reference for
the latest MCP server options:

- https://developers.openai.com/codex/config-reference

## Apps (Connectors)

Use `$` in the composer to insert a ChatGPT connector; the popover lists accessible
apps. The `/apps` command lists available and installed apps. Connected apps appear first
and are labeled as connected; others are marked as can be installed.

## Notify

Codex can run a notification hook when the agent finishes a turn. See the configuration reference for the latest notification settings:

- https://developers.openai.com/codex/config-reference

When Codex knows which client started the turn, the legacy notify JSON payload also includes a top-level `client` field. The TUI reports `codex-tui`, and the app server reports the `clientInfo.name` value from `initialize`.

Every `notify*` hook key accepts either:
- A single command: `["python3", "/path/to/hook.py"]`
- Multiple commands: `[["python3", "/path/to/a.py"], ["python3", "/path/to/b.py"]]`

Example multi-hook fanout:

```toml
notify_on_after_tool_use = [
  ["python3", "/absolute/path/to/hook-a.py"],
  ["python3", "/absolute/path/to/hook-b.py"],
]
```

To inspect currently configured hooks from the running config:
- CLI: `codex hooks` (or `dccodex hooks`), with optional `--json`
- TUI: `/hooks`

Codex can also run a separate hook right after a user prompt is accepted, via `notify_on_user_prompt_submit` in `config.toml`. This hook receives a JSON hook payload as the final argv argument, including `event_type = "after_user_prompt_submit"`. If the script writes `{"append_prompt_text":"..."}` to stdout, that text is appended to the user prompt before model sampling. A Python script is supported, for example:

```toml
notify_on_user_prompt_submit = ["python3", "/absolute/path/to/hook.py"]
```

The hook can also request automatic plan-mode switching by writing `{"switch_to_plan_mode":true}`.
You can combine both fields in one response.

Hybrid classifier example (`hook.py`):

```python
#!/usr/bin/env python3
import json
import os
import sys
from typing import Optional

PLAN_PHRASES = (
    "develop a plan",
    "create a plan",
    "make a plan",
    "let's plan",
    "lets plan",
    "outline a plan",
    "implementation plan",
)


def latest_user_message(payload: dict) -> str:
    messages = payload.get("hook_event", {}).get("input_messages", [])
    if not messages:
        return ""
    return str(messages[-1])


def keyword_plan_intent(text: str) -> bool:
    normalized = text.lower()
    return any(phrase in normalized for phrase in PLAN_PHRASES)


def llm_plan_intent(text: str) -> Optional[tuple[bool, float]]:
    # Optional fallback:
    # - requires OPENAI_API_KEY
    # - requires: pip install openai
    api_key = os.getenv("OPENAI_API_KEY")
    if not api_key:
        return None
    try:
        from openai import OpenAI
    except Exception:
        return None

    model = os.getenv("PLAN_HOOK_MODEL", "gpt-5-mini")
    threshold = float(os.getenv("PLAN_HOOK_CONFIDENCE_THRESHOLD", "0.75"))
    prompt = (
        "Classify whether this user message is asking to plan first (not execute yet). "
        "Return strict JSON: "
        '{"switch_to_plan_mode": boolean, "confidence": number}. '
        f"Message: {text!r}"
    )
    try:
        client = OpenAI(api_key=api_key)
        response = client.responses.create(
            model=model,
            input=prompt,
            reasoning={"effort": "minimal"},
            max_output_tokens=120,
        )
        data = json.loads(response.output_text.strip())
        should_switch = bool(data.get("switch_to_plan_mode"))
        confidence = float(data.get("confidence", 0.0))
        return (should_switch and confidence >= threshold, confidence)
    except Exception as exc:
        print(f"plan hook llm fallback failed: {exc}", file=sys.stderr)
        return None


def main() -> int:
    payload = json.loads(sys.argv[1]) if len(sys.argv) > 1 else {}
    text = latest_user_message(payload)

    switch_to_plan_mode = False
    append_prompt_text = None

    # Fast local path.
    if keyword_plan_intent(text):
        switch_to_plan_mode = True

    # Optional model-based path for ambiguous prompts.
    llm_result = llm_plan_intent(text)
    if llm_result is not None:
        llm_switch, _confidence = llm_result
        switch_to_plan_mode = switch_to_plan_mode or llm_switch

    # Optional: append an extra instruction when we route to plan mode.
    if switch_to_plan_mode:
        append_prompt_text = (
            "Focus on planning only. Produce a decision-complete plan before execution."
        )

    output = {"switch_to_plan_mode": switch_to_plan_mode}
    if append_prompt_text:
        output["append_prompt_text"] = append_prompt_text
    print(json.dumps(output))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
```

Codex can also run a post-tool hook after every tool execution via `notify_on_after_tool_use`. This hook receives a JSON hook payload as the final argv argument, including `event_type = "after_tool_use"`:

```toml
notify_on_after_tool_use = ["python3", "/absolute/path/to/hook.py"]
```

Codex can run a hook immediately before each model sampling request via
`notify_on_before_model_request`, with `event_type = "before_model_request"`:

```toml
notify_on_before_model_request = ["python3", "/absolute/path/to/hook.py"]
```

Codex can run a hook when the model stream reports `response.created` via
`notify_on_model_response_created`, with
`event_type = "after_model_response_created"`:

```toml
notify_on_model_response_created = ["python3", "/absolute/path/to/hook.py"]
```

Codex can also run a hook immediately before each tool execution via
`notify_on_pre_tool_use`, with `event_type = "pre_tool_use"`:

```toml
notify_on_pre_tool_use = ["python3", "/absolute/path/to/hook.py"]
```

`pre_tool_use` hooks can return JSON on stdout to control behavior:

```json
{"decision":"allow"}
{"decision":"deny","message":"blocked by policy"}
{"decision":"replace","output":"Use list_dir instead of shell for this.","success":false}
```

`replace` skips the original tool call and returns the provided `output` text to the model.
`success` controls the returned tool success flag (defaults to `false` for replacements).

Codex can run a hook when tool execution fails via `notify_on_tool_failure`,
with `event_type = "tool_failure"`:

```toml
notify_on_tool_failure = ["python3", "/absolute/path/to/hook.py"]
```

Codex can run a hook when tool execution succeeds via
`notify_on_post_tool_use_success`, with
`event_type = "post_tool_use_success"`:

```toml
notify_on_post_tool_use_success = ["python3", "/absolute/path/to/hook.py"]
```

Codex can also run a hook after every model `response.completed` event via
`notify_on_model_response_completed`. This hook receives
`event_type = "after_model_response_completed"` and includes per-response
token usage (`token_usage`) when the backend provides it. In Plan mode, this
event also includes `proposed_plan` when the assistant emits a
`<proposed_plan>...</proposed_plan>` block:

```toml
notify_on_model_response_completed = ["python3", "/absolute/path/to/hook.py"]
```

An example script is available at
`scripts/model_response_usage_hook.py`. It appends one JSON object per
completed model response to a JSONL file for downstream token telemetry.

For plan capture, use `scripts/proposed_plan_capture_hook.py` to append plan
text to a branch-scoped Markdown file:

```toml
notify_on_model_response_completed = ["python3", "/absolute/path/to/repo/scripts/proposed_plan_capture_hook.py"]
```

Default output path is `~/.codex/hooks/plans/<git-branch>.md`.
Override the base directory with:

```shell
CODEX_PLAN_LOG_DIR=/tmp/codex-plans codex
```

Example config:

```toml
notify_on_model_response_completed = ["python3", "/absolute/path/to/repo/scripts/model_response_usage_hook.py"]
```

Optional log output path override (default is
`~/.codex/hooks/model_response_usage.jsonl`):

```shell
CODEX_MODEL_RESPONSE_USAGE_LOG=/tmp/codex-response-usage.jsonl codex
```

Codex can also run turn lifecycle hooks:

```toml
notify_on_turn_started = ["python3", "/absolute/path/to/hook.py"]
notify_on_turn_completed = ["python3", "/absolute/path/to/hook.py"]
notify_on_turn_aborted = ["python3", "/absolute/path/to/hook.py"]
```

These emit `event_type` values `turn_started`, `turn_completed`, and
`turn_aborted`.

Codex can run session lifecycle hooks:

```toml
notify_on_session_start = ["python3", "/absolute/path/to/hook.py"]
notify_on_session_shutdown = ["python3", "/absolute/path/to/hook.py"]
```

These emit `event_type` values `session_start` and `session_shutdown`.

Codex can run a compaction lifecycle hook:

```toml
notify_on_compaction = ["python3", "/absolute/path/to/hook.py"]
```

This emits `event_type = "compaction"` with:
- `trigger`: `manual`, `auto_pre_turn`, or `auto_mid_turn`
- `strategy`: `local` or `remote`
- `status`: `started`, `completed`, or `failed`
- `error`: present only when `status = "failed"`

Hook execution logging is emitted at debug level for:
- `after_user_prompt_submit`
- `before_model_request`
- `after_model_response_created`
- `turn_started`
- `turn_completed`
- `turn_aborted`
- `session_start`
- `session_shutdown`
- `compaction`
- `pre_tool_use`
- `tool_failure`
- `post_tool_use_success`
- `after_tool_use`
- `after_agent`
- `after_model_response_completed`

To see these logs:

```shell
RUST_LOG=codex_core=debug codex
```

## JSON Schema

The generated JSON Schema for `config.toml` lives at `codex-rs/core/config.schema.json`.

## SQLite State DB

Codex stores the SQLite-backed state DB under `sqlite_home` (config key) or the
`CODEX_SQLITE_HOME` environment variable. When unset, WorkspaceWrite sandbox
sessions default to a temp directory; other modes default to `CODEX_HOME`.

## Notices

Codex stores "do not show again" flags for some UI prompts under the `[notice]` table.

## Plan mode defaults

`plan_mode_reasoning_effort` lets you set a Plan-mode-specific default reasoning
effort override. When unset, Plan mode uses the built-in Plan preset default
(currently `medium`). When explicitly set (including `none`), it overrides the
Plan preset. The string value `none` means "no reasoning" (an explicit Plan
override), not "inherit the global default". There is currently no separate
config value for "follow the global default in Plan mode".

Ctrl+C/Ctrl+D quitting uses a ~1 second double-press hint (`ctrl + c again to quit`).
