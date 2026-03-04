use std::process::Stdio;
use std::sync::Arc;

use serde::Deserialize;
use serde::Serialize;

use crate::Hook;
use crate::HookEvent;
use crate::HookPayload;
use crate::HookPreToolUseDecision;
use crate::HookResult;
use crate::command_from_argv;

/// Legacy notify payload appended as the final argv argument for backward compatibility.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum UserNotification {
    #[serde(rename_all = "kebab-case")]
    AgentTurnComplete {
        thread_id: String,
        turn_id: String,
        cwd: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        client: Option<String>,

        /// Messages that the user sent to the agent to initiate the turn.
        input_messages: Vec<String>,

        /// The last message sent by the assistant in the turn.
        last_assistant_message: Option<String>,

        /// The latest proposed plan text, when Plan mode emitted one.
        #[serde(skip_serializing_if = "Option::is_none")]
        proposed_plan: Option<String>,
    },
}

pub fn legacy_notify_json(payload: &HookPayload) -> Result<String, serde_json::Error> {
    match &payload.hook_event {
        HookEvent::AfterAgent { event } => {
            serde_json::to_string(&UserNotification::AgentTurnComplete {
                thread_id: event.thread_id.to_string(),
                turn_id: event.turn_id.clone(),
                cwd: payload.cwd.display().to_string(),
                client: payload.client.clone(),
                input_messages: event.input_messages.clone(),
                last_assistant_message: event.last_assistant_message.clone(),
                proposed_plan: event.proposed_plan.clone(),
            })
        }
        _ => Err(serde_json::Error::io(std::io::Error::other(
            "legacy notify payload is only supported for after_agent",
        ))),
    }
}

pub fn notify_hook(argv: Vec<String>) -> Hook {
    let argv = Arc::new(argv);
    Hook {
        name: "legacy_notify".to_string(),
        func: Arc::new(move |payload: &HookPayload| {
            let argv = Arc::clone(&argv);
            Box::pin(async move {
                let mut command = match command_from_argv(&argv) {
                    Some(command) => command,
                    None => return HookResult::Success,
                };
                if let Ok(notify_payload) = legacy_notify_json(payload) {
                    command.arg(notify_payload);
                }

                // Backwards-compat: match legacy notify behavior (argv + JSON arg, fire-and-forget).
                command
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());

                match command.spawn() {
                    Ok(_) => HookResult::Success,
                    Err(err) => HookResult::FailedContinue(err.into()),
                }
            })
        }),
    }
}

pub fn json_payload_hook(name: String, argv: Vec<String>) -> Hook {
    let argv = Arc::new(argv);
    Hook {
        name,
        func: Arc::new(move |payload: &HookPayload| {
            let argv = Arc::clone(&argv);
            Box::pin(async move {
                let mut command = match command_from_argv(&argv) {
                    Some(command) => command,
                    None => return HookResult::Success,
                };
                match serde_json::to_string(payload) {
                    Ok(json_payload) => {
                        command.arg(json_payload);
                    }
                    Err(err) => return HookResult::FailedContinue(err.into()),
                }

                command
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());

                match command.spawn() {
                    Ok(_) => HookResult::Success,
                    Err(err) => HookResult::FailedContinue(err.into()),
                }
            })
        }),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AfterUserPromptSubmitHookResponse {
    #[serde(default)]
    append_prompt_text: Option<String>,
    #[serde(default)]
    switch_to_plan_mode: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PreToolUseHookDecision {
    Allow,
    Deny,
    Replace,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreToolUseHookResponse {
    #[serde(default)]
    decision: Option<PreToolUseHookDecision>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    output: Option<String>,
    #[serde(default)]
    success: Option<bool>,
}

pub fn pre_tool_use_hook(argv: Vec<String>) -> Hook {
    let argv = Arc::new(argv);
    Hook {
        name: "pre_tool_use".to_string(),
        func: Arc::new(move |payload: &HookPayload| {
            let argv = Arc::clone(&argv);
            Box::pin(async move {
                let mut command = match command_from_argv(&argv) {
                    Some(command) => command,
                    None => return HookResult::Success,
                };
                let json_payload = match serde_json::to_string(payload) {
                    Ok(json_payload) => json_payload,
                    Err(err) => return HookResult::FailedContinue(err.into()),
                };
                command.arg(json_payload);
                command
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());

                let output = match command.output().await {
                    Ok(output) => output,
                    Err(err) => return HookResult::FailedContinue(err.into()),
                };
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return HookResult::FailedContinue(
                        std::io::Error::other(format!(
                            "pre_tool_use hook exited with status {status}: {stderr}",
                            status = output.status,
                            stderr = stderr.trim(),
                        ))
                        .into(),
                    );
                }

                let stdout = String::from_utf8_lossy(&output.stdout);
                let stdout = stdout.trim();
                if stdout.is_empty() {
                    return HookResult::Success;
                }

                match serde_json::from_str::<PreToolUseHookResponse>(stdout) {
                    Ok(response) => match response.decision {
                        Some(PreToolUseHookDecision::Allow) | None => HookResult::Success,
                        Some(PreToolUseHookDecision::Deny) => {
                            let message = response
                                .message
                                .filter(|msg| !msg.is_empty())
                                .unwrap_or_else(|| {
                                    "tool call denied by pre_tool_use hook".to_string()
                                });
                            HookResult::SuccessWithPreToolUseDecision(
                                HookPreToolUseDecision::Deny { message },
                            )
                        }
                        Some(PreToolUseHookDecision::Replace) => {
                            let Some(output) = response.output.filter(|value| !value.is_empty())
                            else {
                                return HookResult::FailedContinue(
                                    std::io::Error::other(
                                        "pre_tool_use hook returned decision=replace without non-empty output",
                                    )
                                    .into(),
                                );
                            };
                            HookResult::SuccessWithPreToolUseDecision(
                                HookPreToolUseDecision::Replace {
                                    output,
                                    success: response.success.unwrap_or(false),
                                },
                            )
                        }
                    },
                    Err(err) => HookResult::FailedContinue(
                        std::io::Error::other(format!(
                            "pre_tool_use hook returned invalid JSON on stdout: {err}"
                        ))
                        .into(),
                    ),
                }
            })
        }),
    }
}

pub fn after_user_prompt_submit_hook(argv: Vec<String>) -> Hook {
    let argv = Arc::new(argv);
    Hook {
        name: "after_user_prompt_submit".to_string(),
        func: Arc::new(move |payload: &HookPayload| {
            let argv = Arc::clone(&argv);
            Box::pin(async move {
                let mut command = match command_from_argv(&argv) {
                    Some(command) => command,
                    None => return HookResult::Success,
                };
                let json_payload = match serde_json::to_string(payload) {
                    Ok(json_payload) => json_payload,
                    Err(err) => return HookResult::FailedContinue(err.into()),
                };
                command.arg(json_payload);
                command
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());

                let output = match command.output().await {
                    Ok(output) => output,
                    Err(err) => return HookResult::FailedContinue(err.into()),
                };
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return HookResult::FailedContinue(
                        std::io::Error::other(format!(
                            "after_user_prompt_submit hook exited with status {status}: {stderr}",
                            status = output.status,
                            stderr = stderr.trim(),
                        ))
                        .into(),
                    );
                }

                let stdout = String::from_utf8_lossy(&output.stdout);
                let stdout = stdout.trim();
                if stdout.is_empty() {
                    return HookResult::Success;
                }

                match serde_json::from_str::<AfterUserPromptSubmitHookResponse>(stdout) {
                    Ok(response) => {
                        let append_prompt_text =
                            response.append_prompt_text.filter(|text| !text.is_empty());
                        if append_prompt_text.is_none() && !response.switch_to_plan_mode {
                            HookResult::Success
                        } else if response.switch_to_plan_mode {
                            HookResult::SuccessWithPromptAugmentation {
                                append_prompt_text,
                                switch_to_plan_mode: true,
                            }
                        } else if let Some(text) = append_prompt_text {
                            HookResult::SuccessWithAppendedUserPrompt(text)
                        } else {
                            HookResult::Success
                        }
                    }
                    Err(err) => HookResult::FailedContinue(
                        std::io::Error::other(format!(
                            "after_user_prompt_submit hook returned invalid JSON on stdout: {err}"
                        ))
                        .into(),
                    ),
                }
            })
        }),
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use codex_protocol::ThreadId;
    use pretty_assertions::assert_eq;
    use serde_json::Value;
    use serde_json::json;

    use super::*;

    fn expected_notification_json() -> Value {
        json!({
            "type": "agent-turn-complete",
            "thread-id": "b5f6c1c2-1111-2222-3333-444455556666",
            "turn-id": "12345",
            "cwd": "/Users/example/project",
            "client": "codex-tui",
            "input-messages": ["Rename `foo` to `bar` and update the callsites."],
            "last-assistant-message": "Rename complete and verified `cargo build` succeeds.",
        })
    }

    fn pre_tool_use_payload() -> HookPayload {
        HookPayload {
            session_id: ThreadId::from_string("b5f6c1c2-1111-2222-3333-444455556666")
                .expect("valid session id"),
            cwd: std::path::Path::new("/Users/example/project").to_path_buf(),
            client: Some("codex-tui".to_string()),
            triggered_at: chrono::Utc::now(),
            hook_event: HookEvent::PreToolUse {
                event: crate::HookEventPreToolUse {
                    turn_id: "turn-1".to_string(),
                    call_id: "call-1".to_string(),
                    tool_name: "local_shell".to_string(),
                    tool_kind: crate::HookToolKind::LocalShell,
                    tool_input: crate::HookToolInput::LocalShell {
                        params: crate::HookToolInputLocalShell {
                            command: vec!["echo".to_string(), "hello".to_string()],
                            workdir: None,
                            timeout_ms: None,
                            sandbox_permissions: None,
                            prefix_rule: None,
                            justification: None,
                        },
                    },
                    mutating: false,
                    sandbox: "none".to_string(),
                    sandbox_policy: "workspace-write".to_string(),
                },
            },
        }
    }

    #[test]
    fn test_user_notification() -> Result<()> {
        let notification = UserNotification::AgentTurnComplete {
            thread_id: "b5f6c1c2-1111-2222-3333-444455556666".to_string(),
            turn_id: "12345".to_string(),
            cwd: "/Users/example/project".to_string(),
            client: Some("codex-tui".to_string()),
            input_messages: vec!["Rename `foo` to `bar` and update the callsites.".to_string()],
            last_assistant_message: Some(
                "Rename complete and verified `cargo build` succeeds.".to_string(),
            ),
            proposed_plan: None,
        };
        let serialized = serde_json::to_string(&notification)?;
        let actual: Value = serde_json::from_str(&serialized)?;
        assert_eq!(actual, expected_notification_json());
        Ok(())
    }

    #[test]
    fn legacy_notify_json_matches_historical_wire_shape() -> Result<()> {
        let payload = HookPayload {
            session_id: ThreadId::new(),
            cwd: std::path::Path::new("/Users/example/project").to_path_buf(),
            client: Some("codex-tui".to_string()),
            triggered_at: chrono::Utc::now(),
            hook_event: HookEvent::AfterAgent {
                event: crate::HookEventAfterAgent {
                    thread_id: ThreadId::from_string("b5f6c1c2-1111-2222-3333-444455556666")
                        .expect("valid thread id"),
                    turn_id: "12345".to_string(),
                    input_messages: vec![
                        "Rename `foo` to `bar` and update the callsites.".to_string(),
                    ],
                    last_assistant_message: Some(
                        "Rename complete and verified `cargo build` succeeds.".to_string(),
                    ),
                    proposed_plan: None,
                },
            },
        };

        let serialized = legacy_notify_json(&payload)?;
        let actual: Value = serde_json::from_str(&serialized)?;
        assert_eq!(actual, expected_notification_json());

        Ok(())
    }

    #[tokio::test]
    async fn json_payload_hook_appends_hook_payload_as_json() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let output_path = temp.path().join("hook.json");
        let script_path = temp.path().join("capture.py");
        std::fs::write(
            &script_path,
            r#"#!/usr/bin/env python3
import pathlib
import sys
pathlib.Path(sys.argv[1]).write_text(sys.argv[2], encoding="utf-8")
"#,
        )?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script_path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script_path, perms)?;
        }

        let payload = HookPayload {
            session_id: ThreadId::from_string("b5f6c1c2-1111-2222-3333-444455556666")
                .expect("valid session id"),
            cwd: std::path::Path::new("/Users/example/project").to_path_buf(),
            client: Some("codex-tui".to_string()),
            triggered_at: chrono::Utc::now(),
            hook_event: HookEvent::AfterAgent {
                event: crate::HookEventAfterAgent {
                    thread_id: ThreadId::from_string("b5f6c1c2-1111-2222-3333-444455556666")
                        .expect("valid thread id"),
                    turn_id: "12345".to_string(),
                    input_messages: vec![
                        "Rename `foo` to `bar` and update the callsites.".to_string(),
                    ],
                    last_assistant_message: Some(
                        "Rename complete and verified `cargo build` succeeds.".to_string(),
                    ),
                    proposed_plan: None,
                },
            },
        };

        let argv = vec![
            "python3".to_string(),
            script_path.display().to_string(),
            output_path.display().to_string(),
        ];
        let hook = json_payload_hook("json_payload_test".to_string(), argv);
        let response = hook.execute(&payload).await;
        assert!(matches!(response.result, HookResult::Success));

        let mut writes = 0;
        while writes < 100 && !output_path.exists() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            writes += 1;
        }
        assert!(output_path.exists());

        let actual: Value = serde_json::from_str(&std::fs::read_to_string(output_path)?)?;
        let expected = serde_json::to_value(payload)?;
        assert_eq!(actual, expected);

        Ok(())
    }

    #[tokio::test]
    async fn after_user_prompt_submit_hook_returns_appended_prompt_text() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let script_path = temp.path().join("append_prompt.py");
        std::fs::write(
            &script_path,
            r#"#!/usr/bin/env python3
import json
import sys

payload = json.loads(sys.argv[1])
turn_id = payload["hook_event"]["turn_id"]
print(json.dumps({"append_prompt_text": f"hook-note-for-{turn_id}"}))
"#,
        )?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script_path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script_path, perms)?;
        }

        let payload = HookPayload {
            session_id: ThreadId::from_string("b5f6c1c2-1111-2222-3333-444455556666")
                .expect("valid session id"),
            cwd: std::path::Path::new("/Users/example/project").to_path_buf(),
            client: Some("codex-tui".to_string()),
            triggered_at: chrono::Utc::now(),
            hook_event: HookEvent::AfterUserPromptSubmit {
                event: crate::HookEventAfterUserPromptSubmit {
                    thread_id: ThreadId::from_string("b5f6c1c2-1111-2222-3333-444455556666")
                        .expect("valid thread id"),
                    turn_id: "12345".to_string(),
                    input_messages: vec!["hello".to_string()],
                },
            },
        };

        let argv = vec!["python3".to_string(), script_path.display().to_string()];
        let hook = after_user_prompt_submit_hook(argv);
        let response = hook.execute(&payload).await;
        assert!(matches!(
            response.result,
            HookResult::SuccessWithAppendedUserPrompt(ref text) if text == "hook-note-for-12345"
        ));

        Ok(())
    }

    #[tokio::test]
    async fn after_user_prompt_submit_hook_returns_plan_mode_switch_signal() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let script_path = temp.path().join("plan_mode.py");
        std::fs::write(
            &script_path,
            r#"#!/usr/bin/env python3
import json
print(json.dumps({"switch_to_plan_mode": True}))
"#,
        )?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script_path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script_path, perms)?;
        }

        let payload = HookPayload {
            session_id: ThreadId::from_string("b5f6c1c2-1111-2222-3333-444455556666")
                .expect("valid session id"),
            cwd: std::path::Path::new("/Users/example/project").to_path_buf(),
            client: Some("codex-tui".to_string()),
            triggered_at: chrono::Utc::now(),
            hook_event: HookEvent::AfterUserPromptSubmit {
                event: crate::HookEventAfterUserPromptSubmit {
                    thread_id: ThreadId::from_string("b5f6c1c2-1111-2222-3333-444455556666")
                        .expect("valid thread id"),
                    turn_id: "12345".to_string(),
                    input_messages: vec!["hello".to_string()],
                },
            },
        };

        let argv = vec!["python3".to_string(), script_path.display().to_string()];
        let hook = after_user_prompt_submit_hook(argv);
        let response = hook.execute(&payload).await;
        assert!(matches!(
            response.result,
            HookResult::SuccessWithPromptAugmentation {
                append_prompt_text: None,
                switch_to_plan_mode: true
            }
        ));

        Ok(())
    }

    #[tokio::test]
    async fn pre_tool_use_hook_returns_allow_by_default() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let script_path = temp.path().join("allow.py");
        std::fs::write(
            &script_path,
            r#"#!/usr/bin/env python3
import json
print(json.dumps({"decision": "allow"}))
"#,
        )?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script_path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script_path, perms)?;
        }

        let argv = vec!["python3".to_string(), script_path.display().to_string()];
        let hook = pre_tool_use_hook(argv);
        let response = hook.execute(&pre_tool_use_payload()).await;
        assert!(matches!(response.result, HookResult::Success));

        Ok(())
    }

    #[tokio::test]
    async fn pre_tool_use_hook_returns_deny_decision() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let script_path = temp.path().join("deny.py");
        std::fs::write(
            &script_path,
            r#"#!/usr/bin/env python3
import json
print(json.dumps({"decision": "deny", "message": "blocked by policy"}))
"#,
        )?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script_path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script_path, perms)?;
        }

        let argv = vec!["python3".to_string(), script_path.display().to_string()];
        let hook = pre_tool_use_hook(argv);
        let response = hook.execute(&pre_tool_use_payload()).await;
        assert!(matches!(
            response.result,
            HookResult::SuccessWithPreToolUseDecision(HookPreToolUseDecision::Deny { ref message })
                if message == "blocked by policy"
        ));

        Ok(())
    }

    #[tokio::test]
    async fn pre_tool_use_hook_returns_replace_decision() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let script_path = temp.path().join("replace.py");
        std::fs::write(
            &script_path,
            r#"#!/usr/bin/env python3
import json
print(json.dumps({"decision": "replace", "output": "use list_dir instead", "success": False}))
"#,
        )?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script_path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script_path, perms)?;
        }

        let argv = vec!["python3".to_string(), script_path.display().to_string()];
        let hook = pre_tool_use_hook(argv);
        let response = hook.execute(&pre_tool_use_payload()).await;
        assert!(matches!(
            response.result,
            HookResult::SuccessWithPreToolUseDecision(HookPreToolUseDecision::Replace {
                ref output,
                success: false
            }) if output == "use list_dir instead"
        ));

        Ok(())
    }
}
