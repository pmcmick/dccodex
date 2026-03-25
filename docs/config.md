# Configuration

For basic configuration instructions, see [this documentation](https://developers.openai.com/codex/config-basic).

For advanced configuration instructions, see [this documentation](https://developers.openai.com/codex/config-advanced).

For a full configuration reference, see [this documentation](https://developers.openai.com/codex/config-reference).

## Config file location

Codex reads `~/.codex/config.toml` by default.

DCCodex uses `~/.dccodex/config.toml` by default. When DCCodex is running in a repository, it also prefers a project-local `.dccodex/config.toml` over `.codex/config.toml`.

## Connecting to MCP servers

Codex can connect to MCP servers configured in `~/.codex/config.toml`. DCCodex uses `~/.dccodex/config.toml`. See the configuration reference for the latest MCP server options:

- https://developers.openai.com/codex/config-reference

## MCP tool approvals

Codex stores per-tool approval overrides for custom MCP servers under
`mcp_servers` in `~/.codex/config.toml`:

```toml
[mcp_servers.docs.tools.search]
approval_mode = "approve"
```

## Apps (Connectors)

Use `$` in the composer to insert a ChatGPT connector; the popover lists accessible
apps. The `/apps` command lists available and installed apps. Connected apps appear first
and are labeled as connected; others are marked as can be installed.

## Hooks

Codex supports two hook families:

- legacy `notify`
- event-specific `notify_on_*` hooks

DCCodex also keeps the Claude Code-compatible `hooks.json` engine for:

- `SessionStart`
- `UserPromptSubmit`
- `Stop`

`notify` runs when an agent turn finishes. `notify_on_*` hooks run for specific lifecycle events and are the preferred extension point when you need structured automation.

For a practical setup guide with examples and event-by-event recommendations, see [hooks.md](./hooks.md).

Each hook command is configured as an argv array. A single command looks like this:

```toml
notify_on_turn_completed = ["python3", "/absolute/path/hook.py"]
```

Multiple commands for the same event are configured as a list of argv arrays:

```toml
notify_on_turn_completed = [
  ["python3", "/absolute/path/first-hook.py"],
  ["python3", "/absolute/path/second-hook.py"],
]
```

Each configured command receives the event payload JSON as its final argv argument.

### Supported `notify_on_*` hooks

- `notify_on_user_prompt_submit`
- `notify_on_before_model_request`
- `notify_on_model_response_created`
- `notify_on_model_response_completed`
- `notify_on_turn_started`
- `notify_on_turn_completed`
- `notify_on_turn_aborted`
- `notify_on_session_start`
- `notify_on_session_shutdown`
- `notify_on_compaction`
- `notify_on_after_tool_use`
- `notify_on_pre_tool_use`
- `notify_on_tool_failure`
- `notify_on_post_tool_use_success`

### Payload shape

Every event hook receives a JSON payload with this outer structure:

```json
{
  "session_id": "thread_123",
  "cwd": "/workspace/project",
  "client": "codex-tui",
  "triggered_at": "2025-01-01T00:00:00Z",
  "hook_event": {
    "event_type": "turn_completed"
  }
}
```

`hook_event` then contains event-specific fields.

Common examples:

- `notify_on_before_model_request`
  - `thread_id`
  - `turn_id`
  - `model`
  - `sampling_request_index`
  - `input_messages`
- `notify_on_model_response_completed`
  - `thread_id`
  - `turn_id`
  - `response_id`
  - `token_usage`
  - `needs_follow_up`
  - `proposed_plan`
- `notify_on_compaction`
  - `thread_id`
  - `turn_id`
  - `trigger`
  - `strategy`
  - `status`
  - `error`
- tool lifecycle hooks
  - `turn_id`
  - `call_id`
  - `tool_name`
  - `tool_kind`
  - `tool_input`
  - sandbox metadata
  - duration/output or error preview, depending on event

The Rust payload types live in `codex-rs/hooks/src/types.rs` if you need the canonical wire shape.

### Special hook outputs

Most `notify_on_*` hooks are fire-and-forget notifications. Two hooks can actively influence behavior.

`notify_on_user_prompt_submit` may return JSON on stdout with:

```json
{
  "append_prompt_text": "extra instructions to add to the user turn",
  "switch_to_plan_mode": true
}
```

`append_prompt_text` appends another user-visible message to the outbound request. `switch_to_plan_mode` upgrades the turn into Plan mode.

`notify_on_pre_tool_use` may return JSON on stdout with a tool decision:

```json
{
  "decision": "deny",
  "message": "shell access is disabled for this repo"
}
```

or:

```json
{
  "decision": "replace",
  "output": "synthetic tool result",
  "success": true
}
```

`deny` blocks execution. `replace` skips the real tool run and returns the provided output instead.

### Failure behavior

Hooks run in configured order.

- normal success allows later hooks to continue
- hook failures may either continue or abort, depending on the hook result produced by the runtime
- `pre_tool_use` can stop the real tool execution without treating that as a tool failure
- `tool_failure` only runs for real tool-handler failures
- `post_tool_use_success` only runs after real tool-handler success

### DCCodex-specific hook architecture

If you are extending DCCodex rather than upstream Codex, it helps to think in three layers:

1. `notify`
   legacy end-of-turn compatibility hook
2. Claude-compatible `hooks.json`
   synchronous policy hooks for `SessionStart`, `UserPromptSubmit`, and `Stop`
3. DCCodex `notify_on_*`
   JSON lifecycle hooks covering model request boundaries, plan events, compaction, and tool execution

The DCCodex JSON hooks are the non-standard feature set in this repo. Their canonical runtime definitions live in:

- `codex-rs/hooks/src/types.rs`
- `codex-rs/hooks/src/registry.rs`
- `codex-rs/core/src/codex.rs`
- `codex-rs/core/src/tools/registry.rs`

If a hook exits non-zero or emits invalid JSON for a structured response hook, Codex logs the failure and applies the hook-specific failure handling for that event.

### Example shell hook

This example records `before_model_request` payloads:

```toml
notify_on_before_model_request = ["bash", "/absolute/path/log-before-model-request.sh"]
```

```bash
#!/usr/bin/env bash
set -euo pipefail
payload="${@: -1}"
printf '%s\n' "$payload" >> /tmp/codex-before-model-request.jsonl
```

### Legacy notify

Codex can run a legacy notification hook when the agent finishes a turn.

- `notify`

When Codex knows which client started the turn, the legacy notify JSON payload also includes a top-level `client` field. The TUI reports `codex-tui`, and the app server reports the `clientInfo.name` value from `initialize`.

## JSON Schema

The generated JSON Schema for `config.toml` lives at `codex-rs/core/config.schema.json`.

## SQLite State DB

Codex stores the SQLite-backed state DB under `sqlite_home` (config key) or the
`CODEX_SQLITE_HOME` environment variable. When unset, WorkspaceWrite sandbox
sessions default to a temp directory; other modes default to `CODEX_HOME`.

## Custom CA Certificates

Codex can trust a custom root CA bundle for outbound HTTPS and secure websocket
connections when enterprise proxies or gateways intercept TLS. This applies to
login flows and to Codex's other external connections, including Codex
components that build reqwest clients or secure websocket clients through the
shared `codex-client` CA-loading path and remote MCP connections that use it.

Set `CODEX_CA_CERTIFICATE` to the path of a PEM file containing one or more
certificate blocks to use a Codex-specific CA bundle. If
`CODEX_CA_CERTIFICATE` is unset, Codex falls back to `SSL_CERT_FILE`. If
neither variable is set, Codex uses the system root certificates.

`CODEX_CA_CERTIFICATE` takes precedence over `SSL_CERT_FILE`. Empty values are
treated as unset.

The PEM file may contain multiple certificates. Codex also tolerates OpenSSL
`TRUSTED CERTIFICATE` labels and ignores well-formed `X509 CRL` sections in the
same bundle. If the file is empty, unreadable, or malformed, the affected Codex
HTTP or secure websocket connection reports a user-facing error that points
back to these environment variables.

## Notices

Codex stores "do not show again" flags for some UI prompts under the `[notice]` table.

## Plan mode defaults

`plan_mode_reasoning_effort` lets you set a Plan-mode-specific default reasoning
effort override. When unset, Plan mode uses the built-in Plan preset default
(currently `medium`). When explicitly set (including `none`), it overrides the
Plan preset. The string value `none` means "no reasoning" (an explicit Plan
override), not "inherit the global default". There is currently no separate
config value for "follow the global default in Plan mode".

## Realtime start instructions

`experimental_realtime_start_instructions` lets you replace the built-in
developer message Codex inserts when realtime becomes active. It only affects
the realtime start message in prompt history and does not change websocket
backend prompt settings or the realtime end/inactive message.

Ctrl+C/Ctrl+D quitting uses a ~1 second double-press hint (`ctrl + c again to quit`).
