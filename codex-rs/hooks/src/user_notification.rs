use std::process::Stdio;
use std::sync::Arc;

use serde::Deserialize;

use crate::Hook;
use crate::HookPayload;
use crate::HookPreToolUseDecision;
use crate::HookResult;
use crate::command_from_argv;

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
                            "after_user_prompt_submit hook exited with status {}: {}",
                            output.status,
                            stderr.trim(),
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
                    Ok(response) => HookResult::SuccessWithPromptAugmentation {
                        append_prompt_text: response
                            .append_prompt_text
                            .filter(|value| !value.is_empty()),
                        switch_to_plan_mode: response.switch_to_plan_mode,
                    },
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
                            "pre_tool_use hook exited with status {}: {}",
                            output.status,
                            stderr.trim(),
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
