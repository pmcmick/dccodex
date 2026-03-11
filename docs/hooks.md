# Hooks

Hooks let you run your own programs at well-defined points in the Codex lifecycle.

They are useful when you want Codex to integrate with your local environment without changing Codex itself.

Common reasons to use hooks:

- record detailed local audit logs
- send desktop notifications or chat alerts
- inject repo-specific instructions before a model request
- automatically switch some prompts into Plan mode
- block or replace risky tool calls
- capture tool failures for debugging
- track compaction activity in long-running sessions

## Where to configure hooks

Codex reads hooks from `config.toml`.

- Codex default home: `~/.codex/config.toml`
- DCCodex default home: `~/.dccodex/config.toml`
- DCCodex project-local override: `.dccodex/config.toml`

For general configuration details, see [config.md](./config.md).

## How hook commands are defined

Each hook is configured as an argv array. Codex executes the configured program and appends the hook payload JSON as the final argument.

Single command:

```toml
notify_on_turn_completed = ["python3", "/absolute/path/turn-complete.py"]
```

Multiple commands for the same event:

```toml
notify_on_turn_completed = [
  ["python3", "/absolute/path/a.py"],
  ["python3", "/absolute/path/b.py"],
]
```

Hooks for the same event run in configured order.

## What every hook receives

Every hook receives one JSON payload as the final argv value.

Outer payload fields:

- `session_id`
- `cwd`
- `client`
- `triggered_at`
- `hook_event`

Example:

```json
{
  "session_id": "thread_123",
  "cwd": "/workspace/project",
  "client": "codex-tui",
  "triggered_at": "2025-01-01T00:00:00Z",
  "hook_event": {
    "event_type": "turn_completed",
    "thread_id": "thread_123",
    "turn_id": "turn_456"
  }
}
```

The exact event payloads are defined in `codex-rs/hooks/src/types.rs`.

## Supported hooks

### `notify`

Legacy end-of-turn notification hook.

Use this when:

- you only need a single "turn finished" event
- you are migrating older local scripts

### `notify_on_user_prompt_submit`

Runs after the user turn is assembled and before sampling starts.

Use this when:

- you want to append repo-specific instructions
- you want to auto-switch some requests into Plan mode

Special stdout behavior:

```json
{
  "append_prompt_text": "Remember to keep API compatibility.",
  "switch_to_plan_mode": true
}
```

### `notify_on_before_model_request`

Runs immediately before a model request is issued.

Use this when:

- you want to log the exact request cadence
- you want visibility into retries or multi-request turns
- you want to inspect the final input message set

Useful fields:

- `model`
- `sampling_request_index`
- `input_messages`

### `notify_on_model_response_created`

Runs when a response object is created.

Use this when:

- you want request/response lifecycle tracing
- you want to correlate later events by response creation timing

### `notify_on_model_response_completed`

Runs after a response completes.

Use this when:

- you want token usage logging
- you want to capture `needs_follow_up`
- you want to extract a parsed `proposed_plan`

Useful fields:

- `response_id`
- `token_usage`
- `needs_follow_up`
- `proposed_plan`

### `notify_on_turn_started`

Runs when a turn begins.

Use this when:

- you want lifecycle timing around whole turns
- you want to log collaboration mode or context window state

### `notify_on_turn_completed`

Runs when a turn completes successfully.

Use this when:

- you want durable success notifications
- you want per-turn logging that is more explicit than legacy `notify`

### `notify_on_turn_aborted`

Runs when a turn aborts.

Use this when:

- you want failure notifications
- you need the abort reason for local automation

### `notify_on_session_start`

Runs when a session is configured.

Use this when:

- you want session bootstrapping logs
- you want to record model/cwd/client metadata at startup

Note: this is separate from the upstream session-start hook engine. DCCodex keeps the upstream engine and also supports this JSON event hook.

### `notify_on_session_shutdown`

Runs when the session shuts down.

Use this when:

- you want cleanup logging
- you want to close external trackers cleanly

### `notify_on_compaction`

Runs at compaction lifecycle boundaries.

Use this when:

- you want to understand when compaction is happening
- you want to distinguish local vs remote compaction
- you want alerts on compaction failures

Useful fields:

- `trigger`: `manual`, `auto_pre_turn`, `auto_mid_turn`
- `strategy`: `local`, `remote`
- `status`: `started`, `completed`, `failed`
- `error`

### `notify_on_after_tool_use`

Runs after a tool path completes, including metadata about what happened.

Use this when:

- you want a broad tool audit trail
- you want timing and output previews

### `notify_on_pre_tool_use`

Runs before a tool executes.

Use this when:

- you want to block tools in certain repos
- you want to stub or replace tool output
- you want extra policy checks before command execution

Special stdout behavior:

Deny:

```json
{
  "decision": "deny",
  "message": "shell access is disabled for this repo"
}
```

Replace:

```json
{
  "decision": "replace",
  "output": "synthetic tool result",
  "success": true
}
```

### `notify_on_tool_failure`

Runs when a tool fails.

Use this when:

- you want alerts for broken commands
- you want to capture failure previews for later debugging

### `notify_on_post_tool_use_success`

Runs only after successful tool execution.

Use this when:

- you want success-only automation
- you do not want to mix failure events into downstream logs

## Hook behavior and ordering

Rules:

- hooks for one event run in configured order
- most hooks are fire-and-forget notifications
- `notify_on_user_prompt_submit` and `notify_on_pre_tool_use` can actively change behavior
- a hook failure may either continue or abort, depending on the hook result used internally
- if a hook returns `FailedAbort`, later hooks for that same event do not run

## Example scripts

### Example: append request logs

```toml
notify_on_before_model_request = ["bash", "/absolute/path/log-before-model-request.sh"]
```

```bash
#!/usr/bin/env bash
set -euo pipefail
payload="${@: -1}"
printf '%s\n' "$payload" >> /tmp/codex-before-model-request.jsonl
```

### Example: desktop notification on success

```toml
notify_on_turn_completed = ["bash", "/absolute/path/notify-success.sh"]
```

```bash
#!/usr/bin/env bash
set -euo pipefail
payload="${@: -1}"
turn_id="$(printf '%s' "$payload" | jq -r '.hook_event.turn_id')"
notify-send "Codex turn completed" "$turn_id"
```

### Example: deny shell usage in one repo

```toml
notify_on_pre_tool_use = ["python3", "/absolute/path/block-shell.py"]
```

```python
#!/usr/bin/env python3
import json
import sys

payload = json.loads(sys.argv[-1])
event = payload["hook_event"]

if event["tool_name"] == "shell":
    print(json.dumps({
        "decision": "deny",
        "message": "shell is blocked in this repository",
    }))
```

## Recommended starting point

If you are new to hooks, start with:

1. `notify_on_turn_completed`
2. `notify_on_before_model_request`
3. `notify_on_pre_tool_use` only if you need policy enforcement

That gives you visibility first, then control once you understand your workflow.
