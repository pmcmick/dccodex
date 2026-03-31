use std::fs;
use std::path::Path;

use anyhow::Context;
use anyhow::Result;
use codex_features::Feature;
use codex_protocol::items::parse_hook_prompt_fragment;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::RolloutItem;
use codex_protocol::protocol::RolloutLine;
use codex_protocol::user_input::UserInput;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_message_item_added;
use core_test_support::responses::ev_output_text_delta;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_once;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::streaming_sse::StreamingSseChunk;
use core_test_support::streaming_sse::start_streaming_sse_server;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::Value;
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::time::sleep;

const FIRST_CONTINUATION_PROMPT: &str = "Retry with exactly the phrase meow meow meow.";
const SECOND_CONTINUATION_PROMPT: &str = "Now tighten it to just: meow.";
const BLOCKED_PROMPT_CONTEXT: &str = "Remember the blocked lighthouse note.";

fn write_stop_hook(home: &Path, block_prompts: &[&str]) -> Result<()> {
    let script_path = home.join("stop_hook.py");
    let log_path = home.join("stop_hook_log.jsonl");
    let prompts_json =
        serde_json::to_string(block_prompts).context("serialize stop hook prompts for test")?;
    let script = format!(
        r#"import json
from pathlib import Path
import sys

log_path = Path(r"{log_path}")
block_prompts = {prompts_json}

payload = json.load(sys.stdin)
existing = []
if log_path.exists():
    existing = [line for line in log_path.read_text(encoding="utf-8").splitlines() if line.strip()]

with log_path.open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(payload) + "\n")

invocation_index = len(existing)
if invocation_index < len(block_prompts):
    print(json.dumps({{"decision": "block", "reason": block_prompts[invocation_index]}}))
else:
    print(json.dumps({{"systemMessage": f"stop hook pass {{invocation_index + 1}} complete"}}))
"#,
        log_path = log_path.display(),
        prompts_json = prompts_json,
    );
    let hooks = serde_json::json!({
        "hooks": {
            "Stop": [{
                "hooks": [{
                    "type": "command",
                    "command": format!("python3 {}", script_path.display()),
                    "statusMessage": "running stop hook",
                }]
            }]
        }
    });

    fs::write(&script_path, script).context("write stop hook script")?;
    fs::write(home.join("hooks.json"), hooks.to_string()).context("write hooks.json")?;
    Ok(())
}

fn write_parallel_stop_hooks(home: &Path, prompts: &[&str]) -> Result<()> {
    let hook_entries = prompts
        .iter()
        .enumerate()
        .map(|(index, prompt)| {
            let script_path = home.join(format!("stop_hook_{index}.py"));
            let script = format!(
                r#"import json
import sys

payload = json.load(sys.stdin)
if payload["stop_hook_active"]:
    print(json.dumps({{"systemMessage": "done"}}))
else:
    print(json.dumps({{"decision": "block", "reason": {prompt:?}}}))
"#
            );
            fs::write(&script_path, script).with_context(|| {
                format!(
                    "write stop hook script fixture at {}",
                    script_path.display()
                )
            })?;
            Ok(serde_json::json!({
                "type": "command",
                "command": format!("python3 {}", script_path.display()),
            }))
        })
        .collect::<Result<Vec<_>>>()?;

    let hooks = serde_json::json!({
        "hooks": {
            "Stop": [{
                "hooks": hook_entries,
            }]
        }
    });

    fs::write(home.join("hooks.json"), hooks.to_string()).context("write hooks.json")?;
    Ok(())
}

fn write_user_prompt_submit_hook(
    home: &Path,
    blocked_prompt: &str,
    additional_context: &str,
) -> Result<()> {
    let script_path = home.join("user_prompt_submit_hook.py");
    let log_path = home.join("user_prompt_submit_hook_log.jsonl");
    let log_path = log_path.display();
    let blocked_prompt_json =
        serde_json::to_string(blocked_prompt).context("serialize blocked prompt for test")?;
    let additional_context_json = serde_json::to_string(additional_context)
        .context("serialize user prompt submit additional context for test")?;
    let script = format!(
        r#"import json
from pathlib import Path
import sys

payload = json.load(sys.stdin)
with Path(r"{log_path}").open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(payload) + "\n")

if payload.get("prompt") == {blocked_prompt_json}:
    print(json.dumps({{
        "decision": "block",
        "reason": "blocked by hook",
        "hookSpecificOutput": {{
            "hookEventName": "UserPromptSubmit",
            "additionalContext": {additional_context_json}
        }}
    }}))
"#,
    );
    let hooks = serde_json::json!({
        "hooks": {
            "UserPromptSubmit": [{
                "hooks": [{
                    "type": "command",
                    "command": format!("python3 {}", script_path.display()),
                    "statusMessage": "running user prompt submit hook",
                }]
            }]
        }
    });

    fs::write(&script_path, script).context("write user prompt submit hook script")?;
    fs::write(home.join("hooks.json"), hooks.to_string()).context("write hooks.json")?;
    Ok(())
}

fn write_parallel_user_prompt_submit_hooks(home: &Path, contexts: &[&str]) -> Result<()> {
    let log_path = home.join("user_prompt_submit_hook_log.jsonl");
    let barrier_dir = home.join("user_prompt_submit_hook_barrier");
    fs::create_dir_all(&barrier_dir).context("create user prompt submit barrier dir")?;

    let marker_paths = (0..contexts.len())
        .map(|index| barrier_dir.join(format!("hook_{index}.started")))
        .collect::<Vec<_>>();
    let marker_paths_json = serde_json::to_string(
        &marker_paths
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>(),
    )
    .context("serialize user prompt submit barrier paths")?;

    let hook_entries = contexts
        .iter()
        .enumerate()
        .map(|(index, context)| {
            let script_path = home.join(format!("user_prompt_submit_hook_{index}.py"));
            let context_json =
                serde_json::to_string(context).context("serialize user prompt submit context")?;
            let script = format!(
                r#"import json
from pathlib import Path
import sys
import time

log_path = Path(r"{log_path}")
marker_path = Path(r"{marker_path}")
all_markers = [Path(path) for path in {marker_paths_json}]
payload = json.load(sys.stdin)

with log_path.open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(payload) + "\n")

marker_path.write_text("started", encoding="utf-8")
deadline = time.monotonic() + 2.0
while time.monotonic() < deadline:
    if all(path.exists() for path in all_markers):
        print({context_json})
        raise SystemExit(0)
    time.sleep(0.02)

sys.stderr.write("parallel user prompt submit hooks did not overlap\n")
raise SystemExit(1)
"#,
                log_path = log_path.display(),
                marker_path = marker_paths[index].display(),
                marker_paths_json = marker_paths_json,
                context_json = context_json,
            );
            fs::write(&script_path, script).with_context(|| {
                format!(
                    "write user prompt submit hook script fixture at {}",
                    script_path.display()
                )
            })?;
            Ok(serde_json::json!({
                "type": "command",
                "command": format!("python3 {}", script_path.display()),
                "statusMessage": format!("running user prompt submit hook {index}"),
            }))
        })
        .collect::<Result<Vec<_>>>()?;

    let hooks = serde_json::json!({
        "hooks": {
            "UserPromptSubmit": [{
                "hooks": hook_entries,
            }]
        }
    });

    fs::write(home.join("hooks.json"), hooks.to_string()).context("write hooks.json")?;
    Ok(())
}

fn write_pre_tool_use_hook(
    home: &Path,
    matcher: Option<&str>,
    mode: &str,
    reason: &str,
) -> Result<()> {
    let script_path = home.join("pre_tool_use_hook.py");
    let log_path = home.join("pre_tool_use_hook_log.jsonl");
    let mode_json = serde_json::to_string(mode).context("serialize pre tool use mode")?;
    let reason_json = serde_json::to_string(reason).context("serialize pre tool use reason")?;
    let script = format!(
        r#"import json
from pathlib import Path
import sys

log_path = Path(r"{log_path}")
mode = {mode_json}
reason = {reason_json}

payload = json.load(sys.stdin)

with log_path.open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(payload) + "\n")

if mode == "json_deny":
    print(json.dumps({{
        "hookSpecificOutput": {{
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason
        }}
    }}))
elif mode == "exit_2":
    sys.stderr.write(reason + "\n")
    raise SystemExit(2)
"#,
        log_path = log_path.display(),
        mode_json = mode_json,
        reason_json = reason_json,
    );

    let mut group = serde_json::json!({
        "hooks": [{
            "type": "command",
            "command": format!("python3 {}", script_path.display()),
            "statusMessage": "running pre tool use hook",
        }]
    });
    if let Some(matcher) = matcher {
        group["matcher"] = Value::String(matcher.to_string());
    }

    let hooks = serde_json::json!({
        "hooks": {
            "PreToolUse": [group]
        }
    });

    fs::write(&script_path, script).context("write pre tool use hook script")?;
    fs::write(home.join("hooks.json"), hooks.to_string()).context("write hooks.json")?;
    Ok(())
}

fn write_parallel_pre_tool_use_hooks(
    home: &Path,
    matcher: Option<&str>,
    reason: &str,
) -> Result<()> {
    let log_path = home.join("pre_tool_use_hook_log.jsonl");
    let barrier_dir = home.join("pre_tool_use_hook_barrier");
    fs::create_dir_all(&barrier_dir).context("create pre tool use barrier dir")?;

    let marker_paths = (0..2)
        .map(|index| barrier_dir.join(format!("hook_{index}.started")))
        .collect::<Vec<_>>();
    let marker_paths_json = serde_json::to_string(
        &marker_paths
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>(),
    )
    .context("serialize pre tool use barrier paths")?;
    let reason_json = serde_json::to_string(reason).context("serialize pre tool use reason")?;

    let hook_entries = (0..2)
        .map(|index| {
            let script_path = home.join(format!("pre_tool_use_hook_{index}.py"));
            let script = format!(
                r#"import json
from pathlib import Path
import sys
import time

log_path = Path(r"{log_path}")
marker_path = Path(r"{marker_path}")
all_markers = [Path(path) for path in {marker_paths_json}]
reason = {reason_json}
payload = json.load(sys.stdin)

marker_path.write_text("started", encoding="utf-8")
deadline = time.monotonic() + 2.0
while time.monotonic() < deadline:
    if all(path.exists() for path in all_markers):
        with log_path.open("a", encoding="utf-8") as handle:
            handle.write(json.dumps(payload) + "\n")
        print(json.dumps({{
            "hookSpecificOutput": {{
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": reason
            }}
        }}))
        raise SystemExit(0)
    time.sleep(0.02)

sys.stderr.write("parallel pre tool use hooks did not overlap\n")
raise SystemExit(1)
"#,
                log_path = log_path.display(),
                marker_path = marker_paths[index].display(),
                marker_paths_json = marker_paths_json,
                reason_json = reason_json,
            );
            fs::write(&script_path, script).with_context(|| {
                format!(
                    "write pre tool use hook script fixture at {}",
                    script_path.display()
                )
            })?;
            Ok(serde_json::json!({
                "type": "command",
                "command": format!("python3 {}", script_path.display()),
                "statusMessage": format!("running pre tool use hook {index}"),
            }))
        })
        .collect::<Result<Vec<_>>>()?;

    let mut group = serde_json::json!({ "hooks": hook_entries });
    if let Some(matcher) = matcher {
        group["matcher"] = Value::String(matcher.to_string());
    }

    let hooks = serde_json::json!({
        "hooks": {
            "PreToolUse": [group]
        }
    });

    fs::write(home.join("hooks.json"), hooks.to_string()).context("write hooks.json")?;
    Ok(())
}

fn write_post_tool_use_hook(
    home: &Path,
    matcher: Option<&str>,
    mode: &str,
    reason: &str,
) -> Result<()> {
    let script_path = home.join("post_tool_use_hook.py");
    let log_path = home.join("post_tool_use_hook_log.jsonl");
    let mode_json = serde_json::to_string(mode).context("serialize post tool use mode")?;
    let reason_json = serde_json::to_string(reason).context("serialize post tool use reason")?;
    let script = format!(
        r#"import json
from pathlib import Path
import sys

log_path = Path(r"{log_path}")
mode = {mode_json}
reason = {reason_json}

payload = json.load(sys.stdin)

with log_path.open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(payload) + "\n")

if mode == "context":
    print(json.dumps({{
        "hookSpecificOutput": {{
            "hookEventName": "PostToolUse",
            "additionalContext": reason
        }}
    }}))
elif mode == "decision_block":
    print(json.dumps({{
        "decision": "block",
        "reason": reason
    }}))
elif mode == "continue_false":
    print(json.dumps({{
        "continue": False,
        "stopReason": reason
    }}))
elif mode == "exit_2":
    sys.stderr.write(reason + "\n")
    raise SystemExit(2)
"#,
        log_path = log_path.display(),
        mode_json = mode_json,
        reason_json = reason_json,
    );

    let mut group = serde_json::json!({
        "hooks": [{
            "type": "command",
            "command": format!("python3 {}", script_path.display()),
            "statusMessage": "running post tool use hook",
        }]
    });
    if let Some(matcher) = matcher {
        group["matcher"] = Value::String(matcher.to_string());
    }

    let hooks = serde_json::json!({
        "hooks": {
            "PostToolUse": [group]
        }
    });

    fs::write(&script_path, script).context("write post tool use hook script")?;
    fs::write(home.join("hooks.json"), hooks.to_string()).context("write hooks.json")?;
    Ok(())
}

fn write_session_start_hook_recording_transcript(home: &Path) -> Result<()> {
    let script_path = home.join("session_start_hook.py");
    let log_path = home.join("session_start_hook_log.jsonl");
    let script = format!(
        r#"import json
from pathlib import Path
import sys

payload = json.load(sys.stdin)
transcript_path = payload.get("transcript_path")
record = {{
    "transcript_path": transcript_path,
    "exists": Path(transcript_path).exists() if transcript_path else False,
}}

with Path(r"{log_path}").open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(record) + "\n")
"#,
        log_path = log_path.display(),
    );
    let hooks = serde_json::json!({
        "hooks": {
            "SessionStart": [{
                "hooks": [{
                    "type": "command",
                    "command": format!("python3 {}", script_path.display()),
                    "statusMessage": "running session start hook",
                }]
            }]
        }
    });

    fs::write(&script_path, script).context("write session start hook script")?;
    fs::write(home.join("hooks.json"), hooks.to_string()).context("write hooks.json")?;
    Ok(())
}

fn rollout_hook_prompt_texts(text: &str) -> Result<Vec<String>> {
    let mut texts = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let rollout: RolloutLine = serde_json::from_str(trimmed).context("parse rollout line")?;
        if let RolloutItem::ResponseItem(ResponseItem::Message { role, content, .. }) = rollout.item
            && role == "user"
        {
            for item in content {
                if let ContentItem::InputText { text } = item
                    && let Some(fragment) = parse_hook_prompt_fragment(&text)
                {
                    texts.push(fragment.text);
                }
            }
        }
    }
    Ok(texts)
}

fn request_hook_prompt_texts(
    request: &core_test_support::responses::ResponsesRequest,
) -> Vec<String> {
    request
        .message_input_texts("user")
        .into_iter()
        .filter_map(|text| parse_hook_prompt_fragment(&text).map(|fragment| fragment.text))
        .collect()
}

fn read_stop_hook_inputs(home: &Path) -> Result<Vec<serde_json::Value>> {
    fs::read_to_string(home.join("stop_hook_log.jsonl"))
        .context("read stop hook log")?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).context("parse stop hook log line"))
        .collect()
}

fn read_pre_tool_use_hook_inputs(home: &Path) -> Result<Vec<serde_json::Value>> {
    fs::read_to_string(home.join("pre_tool_use_hook_log.jsonl"))
        .context("read pre tool use hook log")?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).context("parse pre tool use hook log line"))
        .collect()
}

fn read_post_tool_use_hook_inputs(home: &Path) -> Result<Vec<serde_json::Value>> {
    fs::read_to_string(home.join("post_tool_use_hook_log.jsonl"))
        .context("read post tool use hook log")?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).context("parse post tool use hook log line"))
        .collect()
}

fn read_session_start_hook_inputs(home: &Path) -> Result<Vec<serde_json::Value>> {
    fs::read_to_string(home.join("session_start_hook_log.jsonl"))
        .context("read session start hook log")?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).context("parse session start hook log line"))
        .collect()
}

fn read_user_prompt_submit_hook_inputs(home: &Path) -> Result<Vec<serde_json::Value>> {
    fs::read_to_string(home.join("user_prompt_submit_hook_log.jsonl"))
        .context("read user prompt submit hook log")?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).context("parse user prompt submit hook log line"))
        .collect()
}

fn ev_message_item_done(id: &str, text: &str) -> Value {
    serde_json::json!({
        "type": "response.output_item.done",
        "item": {
            "type": "message",
            "role": "assistant",
            "id": id,
            "content": [{"type": "output_text", "text": text}]
        }
    })
}

fn sse_event(event: Value) -> String {
    sse(vec![event])
}

fn request_message_input_texts(body: &[u8], role: &str) -> Vec<String> {
    let body: Value = match serde_json::from_slice(body) {
        Ok(body) => body,
        Err(error) => panic!("parse request body: {error}"),
    };
    body.get("input")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("message"))
        .filter(|item| item.get("role").and_then(Value::as_str) == Some(role))
        .filter_map(|item| item.get("content").and_then(Value::as_array))
        .flatten()
        .filter(|span| span.get("type").and_then(Value::as_str) == Some("input_text"))
        .filter_map(|span| span.get("text").and_then(Value::as_str).map(str::to_owned))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stop_hook_can_block_multiple_times_in_same_turn() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let responses = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                ev_assistant_message("msg-1", "draft one"),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_response_created("resp-2"),
                ev_assistant_message("msg-2", "draft two"),
                ev_completed("resp-2"),
            ]),
            sse(vec![
                ev_response_created("resp-3"),
                ev_assistant_message("msg-3", "final draft"),
                ev_completed("resp-3"),
            ]),
        ],
    )
    .await;

    let mut builder = test_codex()
        .with_pre_build_hook(|home| {
            if let Err(error) = write_stop_hook(
                home,
                &[FIRST_CONTINUATION_PROMPT, SECOND_CONTINUATION_PROMPT],
            ) {
                panic!("failed to write stop hook test fixture: {error}");
            }
        })
        .with_config(|config| {
            config
                .features
                .enable(Feature::CodexHooks)
                .expect("test config should allow feature update");
        });
    let test = builder.build(&server).await?;

    test.submit_turn("hello from the sea").await?;

    let requests = responses.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(
        request_hook_prompt_texts(&requests[1]),
        vec![FIRST_CONTINUATION_PROMPT.to_string()],
        "second request should include the first continuation prompt as user hook context",
    );
    assert_eq!(
        request_hook_prompt_texts(&requests[2]),
        vec![
            FIRST_CONTINUATION_PROMPT.to_string(),
            SECOND_CONTINUATION_PROMPT.to_string(),
        ],
        "third request should retain hook prompts in user history",
    );

    let hook_inputs = read_stop_hook_inputs(test.codex_home_path())?;
    assert_eq!(hook_inputs.len(), 3);
    let stop_turn_ids = hook_inputs
        .iter()
        .map(|input| {
            input["turn_id"]
                .as_str()
                .expect("stop hook input turn_id")
                .to_string()
        })
        .collect::<Vec<_>>();
    assert!(
        stop_turn_ids.iter().all(|turn_id| !turn_id.is_empty()),
        "stop hook turn ids should be non-empty",
    );
    let first_stop_turn_id = stop_turn_ids
        .first()
        .expect("stop hook inputs should include a first turn id")
        .clone();
    assert_eq!(
        stop_turn_ids,
        vec![
            first_stop_turn_id.clone(),
            first_stop_turn_id.clone(),
            first_stop_turn_id,
        ],
    );
    assert_eq!(
        hook_inputs
            .iter()
            .map(|input| input["stop_hook_active"]
                .as_bool()
                .expect("stop_hook_active bool"))
            .collect::<Vec<_>>(),
        vec![false, true, true],
    );

    let rollout_path = test.codex.rollout_path().expect("rollout path");
    let rollout_text = fs::read_to_string(&rollout_path)?;
    let hook_prompt_texts = rollout_hook_prompt_texts(&rollout_text)?;
    assert!(
        hook_prompt_texts.contains(&FIRST_CONTINUATION_PROMPT.to_string()),
        "rollout should persist the first continuation prompt",
    );
    assert!(
        hook_prompt_texts.contains(&SECOND_CONTINUATION_PROMPT.to_string()),
        "rollout should persist the second continuation prompt",
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_start_hook_sees_materialized_transcript_path() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let _response = mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-1"),
            ev_assistant_message("msg-1", "hello from the reef"),
            ev_completed("resp-1"),
        ]),
    )
    .await;

    let mut builder = test_codex()
        .with_pre_build_hook(|home| {
            if let Err(error) = write_session_start_hook_recording_transcript(home) {
                panic!("failed to write session start hook test fixture: {error}");
            }
        })
        .with_config(|config| {
            config
                .features
                .enable(Feature::CodexHooks)
                .expect("test config should allow feature update");
        });
    let test = builder.build(&server).await?;

    test.submit_turn("hello").await?;

    let hook_inputs = read_session_start_hook_inputs(test.codex_home_path())?;
    assert_eq!(hook_inputs.len(), 1);
    assert_eq!(
        hook_inputs[0]
            .get("transcript_path")
            .and_then(Value::as_str)
            .map(str::is_empty),
        Some(false)
    );
    assert_eq!(hook_inputs[0].get("exists"), Some(&Value::Bool(true)));

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resumed_thread_keeps_stop_continuation_prompt_in_history() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let initial_responses = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                ev_assistant_message("msg-1", "initial draft"),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_response_created("resp-2"),
                ev_assistant_message("msg-2", "revised draft"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    let mut initial_builder = test_codex()
        .with_pre_build_hook(|home| {
            if let Err(error) = write_stop_hook(home, &[FIRST_CONTINUATION_PROMPT]) {
                panic!("failed to write stop hook test fixture: {error}");
            }
        })
        .with_config(|config| {
            config
                .features
                .enable(Feature::CodexHooks)
                .expect("test config should allow feature update");
        });
    let initial = initial_builder.build(&server).await?;
    let home = initial.home.clone();
    let rollout_path = initial
        .session_configured
        .rollout_path
        .clone()
        .expect("rollout path");

    initial.submit_turn("tell me something").await?;

    assert_eq!(initial_responses.requests().len(), 2);

    let resumed_response = mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-3"),
            ev_assistant_message("msg-3", "fresh turn after resume"),
            ev_completed("resp-3"),
        ]),
    )
    .await;

    let mut resume_builder = test_codex().with_config(|config| {
        config
            .features
            .enable(Feature::CodexHooks)
            .expect("test config should allow feature update");
    });
    let resumed = resume_builder.resume(&server, home, rollout_path).await?;

    resumed.submit_turn("and now continue").await?;

    let resumed_request = resumed_response.single_request();
    assert_eq!(
        request_hook_prompt_texts(&resumed_request),
        vec![FIRST_CONTINUATION_PROMPT.to_string()],
        "resumed request should keep the persisted continuation prompt in user history",
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn multiple_blocking_stop_hooks_persist_multiple_hook_prompt_fragments() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let responses = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                ev_assistant_message("msg-1", "draft one"),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_response_created("resp-2"),
                ev_assistant_message("msg-2", "final draft"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    let mut builder = test_codex()
        .with_pre_build_hook(|home| {
            if let Err(error) = write_parallel_stop_hooks(
                home,
                &[FIRST_CONTINUATION_PROMPT, SECOND_CONTINUATION_PROMPT],
            ) {
                panic!("failed to write parallel stop hook fixtures: {error}");
            }
        })
        .with_config(|config| {
            config
                .features
                .enable(Feature::CodexHooks)
                .expect("test config should allow feature update");
        });
    let test = builder.build(&server).await?;

    test.submit_turn("hello again").await?;

    let requests = responses.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        request_hook_prompt_texts(&requests[1]),
        vec![
            FIRST_CONTINUATION_PROMPT.to_string(),
            SECOND_CONTINUATION_PROMPT.to_string(),
        ],
        "second request should receive one user hook prompt message with both fragments",
    );

    let rollout_path = test.codex.rollout_path().expect("rollout path");
    let rollout_text = fs::read_to_string(&rollout_path)?;
    assert_eq!(
        rollout_hook_prompt_texts(&rollout_text)?,
        vec![
            FIRST_CONTINUATION_PROMPT.to_string(),
            SECOND_CONTINUATION_PROMPT.to_string(),
        ],
        "rollout should preserve both hook prompt fragments in order",
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn blocked_user_prompt_submit_persists_additional_context_for_next_turn() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let response = mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-1"),
            ev_assistant_message("msg-1", "second prompt handled"),
            ev_completed("resp-1"),
        ]),
    )
    .await;

    let mut builder = test_codex()
        .with_pre_build_hook(|home| {
            if let Err(error) =
                write_user_prompt_submit_hook(home, "blocked first prompt", BLOCKED_PROMPT_CONTEXT)
            {
                panic!("failed to write user prompt submit hook test fixture: {error}");
            }
        })
        .with_config(|config| {
            config
                .features
                .enable(Feature::CodexHooks)
                .expect("test config should allow feature update");
        });
    let test = builder.build(&server).await?;

    test.submit_turn("blocked first prompt").await?;
    test.submit_turn("second prompt").await?;

    let request = response.single_request();
    assert!(
        request
            .message_input_texts("developer")
            .contains(&BLOCKED_PROMPT_CONTEXT.to_string()),
        "second request should include developer context persisted from the blocked prompt",
    );
    assert!(
        request
            .message_input_texts("user")
            .iter()
            .all(|text| !text.contains("blocked first prompt")),
        "blocked prompt should not be sent to the model",
    );
    assert!(
        request
            .message_input_texts("user")
            .iter()
            .any(|text| text.contains("second prompt")),
        "second request should include the accepted prompt",
    );

    let hook_inputs = read_user_prompt_submit_hook_inputs(test.codex_home_path())?;
    assert_eq!(hook_inputs.len(), 2);
    assert_eq!(
        hook_inputs
            .iter()
            .map(|input| {
                input["prompt"]
                    .as_str()
                    .expect("user prompt submit hook prompt")
                    .to_string()
            })
            .collect::<Vec<_>>(),
        vec![
            "blocked first prompt".to_string(),
            "second prompt".to_string()
        ],
    );
    assert!(
        hook_inputs.iter().all(|input| input["turn_id"]
            .as_str()
            .is_some_and(|turn_id| !turn_id.is_empty())),
        "blocked and accepted prompt hooks should both receive a non-empty turn_id",
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn parallel_user_prompt_submit_hooks_merge_context_without_serializing() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let response = mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-1"),
            ev_assistant_message("msg-1", "parallel hook prompt handled"),
            ev_completed("resp-1"),
        ]),
    )
    .await;

    let mut builder = test_codex()
        .with_pre_build_hook(|home| {
            if let Err(error) = write_parallel_user_prompt_submit_hooks(
                home,
                &["context from hook A", "context from hook B"],
            ) {
                panic!("failed to write parallel user prompt submit hooks: {error}");
            }
        })
        .with_config(|config| {
            config
                .features
                .enable(Feature::CodexHooks)
                .expect("test config should allow feature update");
        });
    let test = builder.build(&server).await?;

    test.submit_turn("parallel hook prompt").await?;

    let request = response.single_request();
    let developer_texts = request.message_input_texts("developer");
    assert!(
        developer_texts.contains(&"context from hook A".to_string()),
        "request should include context from the first parallel hook",
    );
    assert!(
        developer_texts.contains(&"context from hook B".to_string()),
        "request should include context from the second parallel hook",
    );

    let hook_inputs = read_user_prompt_submit_hook_inputs(test.codex_home_path())?;
    assert_eq!(hook_inputs.len(), 2);
    assert!(
        hook_inputs
            .iter()
            .all(|input| input["prompt"] == "parallel hook prompt"),
        "both user prompt submit hooks should observe the same prompt",
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn blocked_queued_prompt_does_not_strand_earlier_accepted_prompt() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let (gate_completed_tx, gate_completed_rx) = oneshot::channel();
    let first_chunks = vec![
        StreamingSseChunk {
            gate: None,
            body: sse_event(ev_response_created("resp-1")),
        },
        StreamingSseChunk {
            gate: None,
            body: sse_event(ev_message_item_added("msg-1", "")),
        },
        StreamingSseChunk {
            gate: None,
            body: sse_event(ev_output_text_delta("first ")),
        },
        StreamingSseChunk {
            gate: None,
            body: sse_event(ev_message_item_done("msg-1", "first response")),
        },
        StreamingSseChunk {
            gate: Some(gate_completed_rx),
            body: sse_event(ev_completed("resp-1")),
        },
    ];
    let second_chunks = vec![StreamingSseChunk {
        gate: None,
        body: sse(vec![
            ev_response_created("resp-2"),
            ev_assistant_message("msg-2", "accepted queued prompt handled"),
            ev_completed("resp-2"),
        ]),
    }];
    let (server, _completions) =
        start_streaming_sse_server(vec![first_chunks, second_chunks]).await;

    let mut builder = test_codex()
        .with_model("gpt-5.1")
        .with_pre_build_hook(|home| {
            if let Err(error) =
                write_user_prompt_submit_hook(home, "blocked queued prompt", BLOCKED_PROMPT_CONTEXT)
            {
                panic!("failed to write user prompt submit hook test fixture: {error}");
            }
        })
        .with_config(|config| {
            config
                .features
                .enable(Feature::CodexHooks)
                .expect("test config should allow feature update");
        });
    let test = builder.build_with_streaming_server(&server).await?;

    test.codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "initial prompt".to_string(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
        })
        .await?;

    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::AgentMessageContentDelta(_))
    })
    .await;

    for text in ["accepted queued prompt", "blocked queued prompt"] {
        test.codex
            .submit(Op::UserInput {
                items: vec![UserInput::Text {
                    text: text.to_string(),
                    text_elements: Vec::new(),
                }],
                final_output_json_schema: None,
            })
            .await?;
    }

    sleep(Duration::from_millis(100)).await;
    let _ = gate_completed_tx.send(());

    let requests = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let requests = server.requests().await;
            if requests.len() >= 2 {
                break requests;
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("second request should arrive")
    .into_iter()
    .collect::<Vec<_>>();

    sleep(Duration::from_millis(100)).await;

    assert_eq!(requests.len(), 2);

    let second_user_texts = request_message_input_texts(&requests[1], "user");
    assert!(
        second_user_texts.contains(&"accepted queued prompt".to_string()),
        "second request should include the accepted queued prompt",
    );
    assert!(
        !second_user_texts.contains(&"blocked queued prompt".to_string()),
        "second request should not include the blocked queued prompt",
    );

    let hook_inputs = read_user_prompt_submit_hook_inputs(test.codex_home_path())?;
    assert_eq!(hook_inputs.len(), 3);
    assert_eq!(
        hook_inputs
            .iter()
            .map(|input| {
                input["prompt"]
                    .as_str()
                    .expect("queued prompt hook prompt")
                    .to_string()
            })
            .collect::<Vec<_>>(),
        vec![
            "initial prompt".to_string(),
            "accepted queued prompt".to_string(),
            "blocked queued prompt".to_string(),
        ],
    );
    let queued_turn_ids = hook_inputs
        .iter()
        .map(|input| {
            input["turn_id"]
                .as_str()
                .expect("queued prompt hook turn_id")
                .to_string()
        })
        .collect::<Vec<_>>();
    assert!(
        queued_turn_ids.iter().all(|turn_id| !turn_id.is_empty()),
        "queued prompt hook turn ids should be non-empty",
    );
    let first_queued_turn_id = queued_turn_ids
        .first()
        .expect("queued prompt hook inputs should include a first turn id")
        .clone();
    assert_eq!(
        queued_turn_ids,
        vec![
            first_queued_turn_id.clone(),
            first_queued_turn_id.clone(),
            first_queued_turn_id,
        ],
    );

    server.shutdown().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pre_tool_use_blocks_shell_command_before_execution() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let call_id = "pretooluse-shell-command";
    let marker = std::env::temp_dir().join("pretooluse-shell-command-marker");
    let command = format!("printf blocked > {}", marker.display());
    let args = serde_json::json!({ "command": command });
    let responses = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                core_test_support::responses::ev_function_call(
                    call_id,
                    "shell_command",
                    &serde_json::to_string(&args)?,
                ),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_response_created("resp-2"),
                ev_assistant_message("msg-1", "hook blocked it"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    let mut builder = test_codex()
        .with_pre_build_hook(|home| {
            if let Err(error) =
                write_pre_tool_use_hook(home, Some("^Bash$"), "json_deny", "blocked by pre hook")
            {
                panic!("failed to write pre tool use hook test fixture: {error}");
            }
        })
        .with_config(|config| {
            config
                .features
                .enable(Feature::CodexHooks)
                .expect("test config should allow feature update");
        });
    let test = builder.build(&server).await?;

    if marker.exists() {
        fs::remove_file(&marker).context("remove leftover pre tool use marker")?;
    }

    test.submit_turn_with_policy(
        "run the blocked shell command",
        codex_protocol::protocol::SandboxPolicy::DangerFullAccess,
    )
    .await?;

    let requests = responses.requests();
    assert_eq!(requests.len(), 2);
    let output_item = requests[1].function_call_output(call_id);
    let output = output_item
        .get("output")
        .and_then(Value::as_str)
        .expect("shell command output string");
    assert!(
        output.contains("Command blocked by PreToolUse hook: blocked by pre hook"),
        "blocked tool output should surface the hook reason",
    );
    assert!(
        output.contains(&format!("Command: {command}")),
        "blocked tool output should surface the blocked command",
    );
    assert!(
        !marker.exists(),
        "blocked command should not create marker file"
    );

    let hook_inputs = read_pre_tool_use_hook_inputs(test.codex_home_path())?;
    assert_eq!(hook_inputs.len(), 1);
    assert_eq!(hook_inputs[0]["hook_event_name"], "PreToolUse");
    assert_eq!(hook_inputs[0]["tool_name"], "Bash");
    assert_eq!(hook_inputs[0]["tool_use_id"], call_id);
    assert_eq!(hook_inputs[0]["tool_input"]["command"], command);
    let transcript_path = hook_inputs[0]["transcript_path"]
        .as_str()
        .expect("pre tool use hook transcript_path");
    assert!(
        !transcript_path.is_empty(),
        "pre tool use hook should receive a non-empty transcript_path",
    );
    assert!(
        Path::new(transcript_path).exists(),
        "pre tool use hook transcript_path should be materialized on disk",
    );
    assert!(
        hook_inputs[0]["turn_id"]
            .as_str()
            .is_some_and(|turn_id| !turn_id.is_empty())
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn parallel_pre_tool_use_hooks_block_without_serializing() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let call_id = "parallel-pretooluse-shell-command";
    let marker = std::env::temp_dir().join("parallel-pretooluse-shell-command-marker");
    let command = format!("printf blocked > {}", marker.display());
    let args = serde_json::json!({ "command": command });
    let responses = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                core_test_support::responses::ev_function_call(
                    call_id,
                    "shell_command",
                    &serde_json::to_string(&args)?,
                ),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_response_created("resp-2"),
                ev_assistant_message("msg-1", "parallel hook blocked it"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    let mut builder = test_codex()
        .with_pre_build_hook(|home| {
            if let Err(error) =
                write_parallel_pre_tool_use_hooks(home, Some("^Bash$"), "blocked by parallel hooks")
            {
                panic!("failed to write parallel pre tool use hooks: {error}");
            }
        })
        .with_config(|config| {
            config
                .features
                .enable(Feature::CodexHooks)
                .expect("test config should allow feature update");
        });
    let test = builder.build(&server).await?;

    if marker.exists() {
        fs::remove_file(&marker).context("remove leftover parallel pre tool use marker")?;
    }

    test.submit_turn("run the blocked shell command with parallel hooks")
        .await?;

    let requests = responses.requests();
    assert_eq!(requests.len(), 2);
    let output_item = requests[1].function_call_output(call_id);
    let output = output_item
        .get("output")
        .and_then(Value::as_str)
        .expect("shell command output string");
    assert!(
        output.contains("blocked by parallel hooks"),
        "blocked tool output should surface the parallel hook reason",
    );
    assert!(
        !marker.exists(),
        "blocked command should not create marker file"
    );

    let hook_inputs = read_pre_tool_use_hook_inputs(test.codex_home_path())?;
    assert_eq!(hook_inputs.len(), 2);
    assert!(
        hook_inputs
            .iter()
            .all(|input| input["tool_input"]["command"] == command),
        "both pre tool use hooks should observe the same command",
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pre_tool_use_blocks_local_shell_before_execution() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let call_id = "pretooluse-local-shell";
    let marker = std::env::temp_dir().join("pretooluse-local-shell-marker");
    let command = vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        format!("printf blocked > {}", marker.display()),
    ];
    let responses = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                core_test_support::responses::ev_local_shell_call(
                    call_id,
                    "completed",
                    command.iter().map(String::as_str).collect(),
                ),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_response_created("resp-2"),
                ev_assistant_message("msg-1", "local shell blocked"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    let mut builder = test_codex()
        .with_pre_build_hook(|home| {
            if let Err(error) =
                write_pre_tool_use_hook(home, Some("^Bash$"), "json_deny", "blocked local shell")
            {
                panic!("failed to write pre tool use hook test fixture: {error}");
            }
        })
        .with_config(|config| {
            config
                .features
                .enable(Feature::CodexHooks)
                .expect("test config should allow feature update");
        });
    let test = builder.build(&server).await?;

    if marker.exists() {
        fs::remove_file(&marker).context("remove leftover local shell marker")?;
    }

    test.submit_turn("run the blocked local shell command")
        .await?;

    let requests = responses.requests();
    assert_eq!(requests.len(), 2);
    let output_item = requests[1].function_call_output(call_id);
    let output = output_item
        .get("output")
        .and_then(Value::as_str)
        .expect("local shell output string");
    assert!(
        output.contains("Command blocked by PreToolUse hook: blocked local shell"),
        "blocked local shell output should surface the hook reason",
    );
    assert!(
        output.contains(&format!(
            "Command: {}",
            codex_shell_command::parse_command::shlex_join(&command)
        )),
        "blocked local shell output should surface the blocked command",
    );
    assert!(
        !marker.exists(),
        "blocked local shell command should not execute"
    );

    let hook_inputs = read_pre_tool_use_hook_inputs(test.codex_home_path())?;
    assert_eq!(hook_inputs.len(), 1);
    assert_eq!(
        hook_inputs[0]["tool_input"]["command"],
        codex_shell_command::parse_command::shlex_join(&command),
    );
    assert!(
        hook_inputs[0]["turn_id"]
            .as_str()
            .is_some_and(|turn_id| !turn_id.is_empty())
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pre_tool_use_blocks_exec_command_before_execution() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let call_id = "pretooluse-exec-command";
    let marker = std::env::temp_dir().join("pretooluse-exec-command-marker");
    let command = format!("printf blocked > {}", marker.display());
    let args = serde_json::json!({ "cmd": command });
    let responses = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                core_test_support::responses::ev_function_call(
                    call_id,
                    "exec_command",
                    &serde_json::to_string(&args)?,
                ),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_response_created("resp-2"),
                ev_assistant_message("msg-1", "exec command blocked"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    let mut builder = test_codex()
        .with_pre_build_hook(|home| {
            if let Err(error) =
                write_pre_tool_use_hook(home, Some("^Bash$"), "exit_2", "blocked exec command")
            {
                panic!("failed to write pre tool use hook test fixture: {error}");
            }
        })
        .with_config(|config| {
            config.use_experimental_unified_exec_tool = true;
            config
                .features
                .enable(Feature::CodexHooks)
                .expect("test config should allow feature update");
            config
                .features
                .enable(Feature::UnifiedExec)
                .expect("test config should allow feature update");
        });
    let test = builder.build(&server).await?;

    if marker.exists() {
        fs::remove_file(&marker).context("remove leftover exec marker")?;
    }

    test.submit_turn("run the blocked exec command").await?;

    let requests = responses.requests();
    assert_eq!(requests.len(), 2);
    let output_item = requests[1].function_call_output(call_id);
    let output = output_item
        .get("output")
        .and_then(Value::as_str)
        .expect("exec command output string");
    assert!(
        output.contains("Command blocked by PreToolUse hook: blocked exec command"),
        "blocked exec command output should surface the hook reason",
    );
    assert!(
        output.contains(&format!("Command: {command}")),
        "blocked exec command output should surface the blocked command",
    );
    assert!(!marker.exists(), "blocked exec command should not execute");

    let hook_inputs = read_pre_tool_use_hook_inputs(test.codex_home_path())?;
    assert_eq!(hook_inputs.len(), 1);
    assert_eq!(hook_inputs[0]["tool_use_id"], call_id);
    assert_eq!(hook_inputs[0]["tool_input"]["command"], command);
    assert!(
        hook_inputs[0]["turn_id"]
            .as_str()
            .is_some_and(|turn_id| !turn_id.is_empty())
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pre_tool_use_does_not_fire_for_non_shell_tools() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let call_id = "pretooluse-update-plan";
    let args = serde_json::json!({
        "plan": [{
            "step": "watch the tide",
            "status": "pending",
        }]
    });
    let responses = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                core_test_support::responses::ev_function_call(
                    call_id,
                    "update_plan",
                    &serde_json::to_string(&args)?,
                ),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_response_created("resp-2"),
                ev_assistant_message("msg-1", "plan updated"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    let mut builder = test_codex()
        .with_pre_build_hook(|home| {
            if let Err(error) =
                write_pre_tool_use_hook(home, /*matcher*/ None, "json_deny", "should not fire")
            {
                panic!("failed to write pre tool use hook test fixture: {error}");
            }
        })
        .with_config(|config| {
            config
                .features
                .enable(Feature::CodexHooks)
                .expect("test config should allow feature update");
        });
    let test = builder.build(&server).await?;

    test.submit_turn("update the plan").await?;

    let requests = responses.requests();
    assert_eq!(requests.len(), 2);
    let output_item = requests[1].function_call_output(call_id);
    let output = output_item
        .get("output")
        .and_then(Value::as_str)
        .expect("update plan output string");
    assert!(
        !output.contains("should not fire"),
        "non-shell tool output should not be blocked by PreToolUse",
    );

    let hook_log_path = test.codex_home_path().join("pre_tool_use_hook_log.jsonl");
    assert!(
        !hook_log_path.exists(),
        "non-shell tools should not trigger pre tool use hooks",
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pre_tool_use_blocks_apply_patch_before_execution() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let call_id = "pretooluse-apply-patch";
    let file_name = "hook_blocked_apply_patch.txt";
    let patch = format!("*** Begin Patch\n*** Add File: {file_name}\n+blocked\n*** End Patch\n");
    let responses = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                core_test_support::responses::ev_apply_patch_function_call(call_id, &patch),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_response_created("resp-2"),
                ev_assistant_message("msg-1", "apply patch blocked"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    let mut builder = test_codex()
        .with_pre_build_hook(|home| {
            if let Err(error) =
                write_pre_tool_use_hook(home, Some("^Edit$"), "json_deny", "blocked apply patch")
            {
                panic!("failed to write pre tool use hook test fixture: {error}");
            }
        })
        .with_config(|config| {
            config.include_apply_patch_tool = true;
            config
                .features
                .enable(Feature::CodexHooks)
                .expect("test config should allow feature update");
        });
    let test = builder.build(&server).await?;
    let target_path = test.workspace_path(file_name);
    if target_path.exists() {
        fs::remove_file(&target_path).context("remove leftover apply_patch target")?;
    }

    test.submit_turn("apply the blocked patch").await?;

    let requests = responses.requests();
    assert_eq!(requests.len(), 2);
    let output_item = requests[1].function_call_output(call_id);
    let output = output_item
        .get("output")
        .and_then(Value::as_str)
        .expect("apply_patch output string");
    assert!(
        output.contains("Tool blocked by PreToolUse hook: blocked apply patch"),
        "blocked apply_patch output should surface the hook reason",
    );
    assert!(
        output.contains("Tool: Edit"),
        "blocked apply_patch output should surface the canonical hook tool name",
    );
    assert!(
        output.contains(&format!("File: {}", target_path.display())),
        "blocked apply_patch output should surface the targeted file",
    );
    assert!(
        !target_path.exists(),
        "blocked apply_patch should not create the target file",
    );

    let hook_inputs = read_pre_tool_use_hook_inputs(test.codex_home_path())?;
    assert_eq!(hook_inputs.len(), 1);
    assert_eq!(hook_inputs[0]["tool_name"], "Edit");
    assert_eq!(hook_inputs[0]["tool_use_id"], call_id);
    assert_eq!(
        hook_inputs[0]["tool_input"]["file_path"],
        Value::String(target_path.display().to_string()),
    );
    assert_eq!(
        hook_inputs[0]["tool_input"]["file_paths"],
        Value::Array(vec![Value::String(target_path.display().to_string())]),
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn post_tool_use_records_additional_context_for_shell_command() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let call_id = "posttooluse-shell-command";
    let command = "printf post-tool-output".to_string();
    let args = serde_json::json!({ "command": command });
    let responses = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                core_test_support::responses::ev_function_call(
                    call_id,
                    "shell_command",
                    &serde_json::to_string(&args)?,
                ),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_response_created("resp-2"),
                ev_assistant_message("msg-1", "post hook context observed"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    let post_context = "Remember the bash post-tool note.";
    let mut builder = test_codex()
        .with_pre_build_hook(|home| {
            if let Err(error) =
                write_post_tool_use_hook(home, Some("^Bash$"), "context", post_context)
            {
                panic!("failed to write post tool use hook test fixture: {error}");
            }
        })
        .with_config(|config| {
            config
                .features
                .enable(Feature::CodexHooks)
                .expect("test config should allow feature update");
        });
    let test = builder.build(&server).await?;

    test.submit_turn("run the shell command with post hook")
        .await?;

    let requests = responses.requests();
    assert_eq!(requests.len(), 2);
    assert!(
        requests[1]
            .message_input_texts("developer")
            .contains(&post_context.to_string()),
        "follow-up request should include post tool use additional context",
    );
    let output_item = requests[1].function_call_output(call_id);
    let output = output_item
        .get("output")
        .and_then(Value::as_str)
        .expect("shell command output string");
    assert!(
        output.contains("post-tool-output"),
        "shell command output should still reach the model",
    );

    let hook_inputs = read_post_tool_use_hook_inputs(test.codex_home_path())?;
    assert_eq!(hook_inputs.len(), 1);
    assert_eq!(hook_inputs[0]["hook_event_name"], "PostToolUse");
    assert_eq!(hook_inputs[0]["tool_name"], "Bash");
    assert_eq!(hook_inputs[0]["tool_use_id"], call_id);
    assert_eq!(hook_inputs[0]["tool_input"]["command"], command);
    assert_eq!(
        hook_inputs[0]["tool_response"],
        Value::String("post-tool-output".to_string())
    );
    let transcript_path = hook_inputs[0]["transcript_path"]
        .as_str()
        .expect("post tool use hook transcript_path");
    assert!(
        !transcript_path.is_empty(),
        "post tool use hook should receive a non-empty transcript_path",
    );
    assert!(
        Path::new(transcript_path).exists(),
        "post tool use hook transcript_path should be materialized on disk",
    );
    assert!(
        hook_inputs[0]["turn_id"]
            .as_str()
            .is_some_and(|turn_id| !turn_id.is_empty())
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn post_tool_use_block_decision_replaces_shell_command_output_with_reason() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let call_id = "posttooluse-shell-command-block";
    let command = "printf blocked-output".to_string();
    let args = serde_json::json!({ "command": command });
    let responses = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                core_test_support::responses::ev_function_call(
                    call_id,
                    "shell_command",
                    &serde_json::to_string(&args)?,
                ),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_response_created("resp-2"),
                ev_assistant_message("msg-1", "post hook feedback observed"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    let reason = "bash output looked sketchy";
    let mut builder = test_codex()
        .with_pre_build_hook(|home| {
            if let Err(error) =
                write_post_tool_use_hook(home, Some("^Bash$"), "decision_block", reason)
            {
                panic!("failed to write post tool use hook test fixture: {error}");
            }
        })
        .with_config(|config| {
            config
                .features
                .enable(Feature::CodexHooks)
                .expect("test config should allow feature update");
        });
    let test = builder.build(&server).await?;

    test.submit_turn("run the shell command with blocking post hook")
        .await?;

    let requests = responses.requests();
    assert_eq!(requests.len(), 2);
    let output_item = requests[1].function_call_output(call_id);
    let output = output_item
        .get("output")
        .and_then(Value::as_str)
        .expect("shell command output string");
    assert_eq!(output, reason);

    let hook_inputs = read_post_tool_use_hook_inputs(test.codex_home_path())?;
    assert_eq!(hook_inputs.len(), 1);
    assert_eq!(
        hook_inputs[0]["tool_response"],
        Value::String("blocked-output".to_string())
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn post_tool_use_continue_false_replaces_shell_command_output_with_stop_reason() -> Result<()>
{
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let call_id = "posttooluse-shell-command-stop";
    let command = "printf stop-output".to_string();
    let args = serde_json::json!({ "command": command });
    let responses = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                core_test_support::responses::ev_function_call(
                    call_id,
                    "shell_command",
                    &serde_json::to_string(&args)?,
                ),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_response_created("resp-2"),
                ev_assistant_message("msg-1", "post hook stop observed"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    let stop_reason = "Execution halted by post-tool hook";
    let mut builder = test_codex()
        .with_pre_build_hook(|home| {
            if let Err(error) =
                write_post_tool_use_hook(home, Some("^Bash$"), "continue_false", stop_reason)
            {
                panic!("failed to write post tool use hook test fixture: {error}");
            }
        })
        .with_config(|config| {
            config
                .features
                .enable(Feature::CodexHooks)
                .expect("test config should allow feature update");
        });
    let test = builder.build(&server).await?;

    test.submit_turn("run the shell command with stop-style post hook")
        .await?;

    let requests = responses.requests();
    assert_eq!(requests.len(), 2);
    let output_item = requests[1].function_call_output(call_id);
    let output = output_item
        .get("output")
        .and_then(Value::as_str)
        .expect("shell command output string");
    assert_eq!(output, stop_reason);

    let hook_inputs = read_post_tool_use_hook_inputs(test.codex_home_path())?;
    assert_eq!(hook_inputs.len(), 1);
    assert_eq!(
        hook_inputs[0]["tool_response"],
        Value::String("stop-output".to_string())
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn post_tool_use_records_additional_context_for_local_shell() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let call_id = "posttooluse-local-shell";
    let command = vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        "printf local-post-tool-output".to_string(),
    ];
    let responses = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                core_test_support::responses::ev_local_shell_call(
                    call_id,
                    "completed",
                    command.iter().map(String::as_str).collect(),
                ),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_response_created("resp-2"),
                ev_assistant_message("msg-1", "local shell post hook context observed"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    let post_context = "Remember the local shell post-tool note.";
    let mut builder = test_codex()
        .with_pre_build_hook(|home| {
            if let Err(error) =
                write_post_tool_use_hook(home, Some("^Bash$"), "context", post_context)
            {
                panic!("failed to write post tool use hook test fixture: {error}");
            }
        })
        .with_config(|config| {
            config
                .features
                .enable(Feature::CodexHooks)
                .expect("test config should allow feature update");
        });
    let test = builder.build(&server).await?;

    test.submit_turn("run the local shell command with post hook")
        .await?;

    let requests = responses.requests();
    assert_eq!(requests.len(), 2);
    assert!(
        requests[1]
            .message_input_texts("developer")
            .contains(&post_context.to_string()),
        "follow-up request should include local shell post tool use additional context",
    );
    let hook_inputs = read_post_tool_use_hook_inputs(test.codex_home_path())?;
    assert_eq!(hook_inputs.len(), 1);
    assert_eq!(
        hook_inputs[0]["tool_input"]["command"],
        codex_shell_command::parse_command::shlex_join(&command),
    );
    assert_eq!(
        hook_inputs[0]["tool_response"],
        Value::String("local-post-tool-output".to_string()),
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn post_tool_use_exit_two_replaces_one_shot_exec_command_output_with_feedback() -> Result<()>
{
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let call_id = "posttooluse-exec-command";
    let command = "printf post-hook-output".to_string();
    let args = serde_json::json!({ "cmd": command, "tty": false });
    let responses = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                core_test_support::responses::ev_function_call(
                    call_id,
                    "exec_command",
                    &serde_json::to_string(&args)?,
                ),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_response_created("resp-2"),
                ev_assistant_message("msg-1", "post hook blocked the exec result"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    let mut builder = test_codex()
        .with_pre_build_hook(|home| {
            if let Err(error) =
                write_post_tool_use_hook(home, Some("^Bash$"), "exit_2", "blocked by post hook")
            {
                panic!("failed to write post tool use hook test fixture: {error}");
            }
        })
        .with_config(|config| {
            config.use_experimental_unified_exec_tool = true;
            config
                .features
                .enable(Feature::CodexHooks)
                .expect("test config should allow feature update");
            config
                .features
                .enable(Feature::UnifiedExec)
                .expect("test config should allow feature update");
        });
    let test = builder.build(&server).await?;

    test.submit_turn("run the exec command with post hook")
        .await?;

    let requests = responses.requests();
    assert_eq!(requests.len(), 2);
    let output_item = requests[1].function_call_output(call_id);
    let output = output_item
        .get("output")
        .and_then(Value::as_str)
        .expect("exec command output string");
    assert_eq!(output, "blocked by post hook");

    let hook_inputs = read_post_tool_use_hook_inputs(test.codex_home_path())?;
    assert_eq!(hook_inputs.len(), 1);
    assert_eq!(hook_inputs[0]["tool_use_id"], call_id);
    assert_eq!(hook_inputs[0]["tool_input"]["command"], command);
    assert_eq!(
        hook_inputs[0]["tool_response"],
        Value::String("post-hook-output".to_string())
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn post_tool_use_does_not_fire_for_non_shell_tools() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let call_id = "posttooluse-update-plan";
    let args = serde_json::json!({
        "plan": [{
            "step": "watch the tide",
            "status": "pending",
        }]
    });
    let responses = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                core_test_support::responses::ev_function_call(
                    call_id,
                    "update_plan",
                    &serde_json::to_string(&args)?,
                ),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_response_created("resp-2"),
                ev_assistant_message("msg-1", "plan updated"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    let mut builder = test_codex()
        .with_pre_build_hook(|home| {
            if let Err(error) = write_post_tool_use_hook(
                home,
                /*matcher*/ None,
                "decision_block",
                "should not fire",
            ) {
                panic!("failed to write post tool use hook test fixture: {error}");
            }
        })
        .with_config(|config| {
            config
                .features
                .enable(Feature::CodexHooks)
                .expect("test config should allow feature update");
        });
    let test = builder.build(&server).await?;

    test.submit_turn("update the plan").await?;

    let requests = responses.requests();
    assert_eq!(requests.len(), 2);
    let output_item = requests[1].function_call_output(call_id);
    let output = output_item
        .get("output")
        .and_then(Value::as_str)
        .expect("update plan output string");
    assert!(
        !output.contains("should not fire"),
        "non-shell tool output should not be affected by PostToolUse",
    );

    let hook_log_path = test.codex_home_path().join("post_tool_use_hook_log.jsonl");
    assert!(
        !hook_log_path.exists(),
        "non-shell tools should not trigger post tool use hooks",
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn post_tool_use_records_additional_context_for_apply_patch() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let call_id = "posttooluse-apply-patch";
    let file_name = "hook_post_apply_patch.txt";
    let patch = format!("*** Begin Patch\n*** Add File: {file_name}\n+post hook\n*** End Patch\n");
    let responses = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                core_test_support::responses::ev_apply_patch_function_call(call_id, &patch),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_response_created("resp-2"),
                ev_assistant_message("msg-1", "apply patch post hook context observed"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    let post_context = "Remember the edit hook note.";
    let mut builder = test_codex()
        .with_pre_build_hook(|home| {
            if let Err(error) =
                write_post_tool_use_hook(home, Some("^Edit$"), "context", post_context)
            {
                panic!("failed to write post tool use hook test fixture: {error}");
            }
        })
        .with_config(|config| {
            config.include_apply_patch_tool = true;
            config
                .features
                .enable(Feature::CodexHooks)
                .expect("test config should allow feature update");
        });
    let test = builder.build(&server).await?;
    let target_path = test.workspace_path(file_name);
    if target_path.exists() {
        fs::remove_file(&target_path).context("remove leftover apply_patch target")?;
    }

    test.submit_turn("apply the patch with post hook").await?;

    let requests = responses.requests();
    assert_eq!(requests.len(), 2);
    assert!(
        requests[1]
            .message_input_texts("developer")
            .contains(&post_context.to_string()),
        "follow-up request should include apply_patch post tool use additional context",
    );
    assert!(
        target_path.exists(),
        "apply_patch should create the target file"
    );

    let hook_inputs = read_post_tool_use_hook_inputs(test.codex_home_path())?;
    assert_eq!(hook_inputs.len(), 1);
    assert_eq!(hook_inputs[0]["tool_name"], "Edit");
    assert_eq!(hook_inputs[0]["tool_use_id"], call_id);
    assert_eq!(
        hook_inputs[0]["tool_input"]["file_path"],
        Value::String(target_path.display().to_string()),
    );
    assert!(
        hook_inputs[0]["tool_response"]
            .as_str()
            .is_some_and(|text| text.contains(file_name)),
        "apply_patch post hook should receive a textual tool response mentioning the file",
    );

    Ok(())
}
