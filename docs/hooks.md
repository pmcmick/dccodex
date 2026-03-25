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

## Two Hook Systems

DCCodex supports two related but distinct hook systems:

- Claude Code-compatible hooks configured via `hooks.json`
- DCCodex JSON event hooks configured via `config.toml`

They solve different problems.

Claude Code-compatible hooks:

- events: `SessionStart`, `UserPromptSubmit`, `Stop`
- payload arrives on stdin
- command stdout may return structured control data
- these hooks can stop turn submission, inject additional model context, or continue a stopped turn with a follow-up prompt

DCCodex JSON event hooks:

- keys like `notify_on_before_model_request` and `notify_on_pre_tool_use`
- payload JSON is appended as the final argv argument
- most are notifications; a small subset can actively change runtime behavior
- these hooks are the main extension surface for DCCodex-specific lifecycle events such as compaction, plan finalization, model request boundaries, and tool execution tracing

If you are trying to reproduce Claude Code policy hooks, focus first on:

- `hooks.json` `UserPromptSubmit` / `Stop`
- `notify_on_pre_tool_use`
- `notify_on_before_model_request`

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

For DCCodex JSON event hooks, "configured order" means:

- the runtime invokes hooks in declaration order
- control hooks observe earlier decisions before later hooks run
- fire-and-forget notifier hooks are launched in that order, but they may finish in a different order because they run as child processes

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

`sampling_request_index` counts the 1-based request number within the current turn. A turn with a tool call usually has multiple model requests:

1. initial request
2. request after the first tool output is recorded
3. and so on

### `notify_on_model_response_created`

Runs when a response object is created.

Use this when:

- you want request/response lifecycle tracing
- you want to correlate later events by response creation timing

Useful fields:

- `model`
- `sampling_request_index`

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

This hook runs for:

- real tool execution
- `notify_on_pre_tool_use` deny decisions
- `notify_on_pre_tool_use` replacement decisions

Use the payload to distinguish those cases:

- `executed = true` means the real tool handler ran
- `executed = false` means a pre-tool hook short-circuited execution

### `notify_on_pre_tool_use`

Runs before a tool executes.

Use this when:

- you want to block tools in certain repos
- you want to stub or replace tool output
- you want extra policy checks before command execution

This is the DCCodex JSON-hook control point that most closely matches Claude Code-style tool policy hooks.

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

This hook only runs for real tool-handler failures. It does not run when `notify_on_pre_tool_use` denies or replaces a tool call.

### `notify_on_post_tool_use_success`

Runs only after successful tool execution.

Use this when:

- you want success-only automation
- you do not want to mix failure events into downstream logs

This hook only runs after a real tool handler succeeds. It does not run for pre-tool replacements, even if the replacement marks itself as successful.

## Hook behavior and ordering

Rules:

- hooks for one event run in configured order
- most JSON event hooks are fire-and-forget notifications
- Claude-compatible `SessionStart`, `UserPromptSubmit`, and `Stop` hooks are synchronous and structured
- `notify_on_user_prompt_submit` and `notify_on_pre_tool_use` can actively change behavior
- a hook failure may either continue or abort, depending on the hook result used internally
- if a hook returns `FailedAbort`, later hooks for that same event do not run

Important practical details:

- `notify_on_before_model_request` fires immediately before the request is sent to the provider
- `notify_on_model_response_created` fires when the provider acknowledges response creation
- `notify_on_model_response_completed` fires after the full streamed response is complete
- `notify_on_after_tool_use` is the broad audit hook
- `notify_on_tool_failure` and `notify_on_post_tool_use_success` split the real tool path into failure-only vs success-only follow-ups

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
