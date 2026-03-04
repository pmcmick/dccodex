use tokio::process::Command;

use crate::types::Hook;
use crate::types::HookEvent;
use crate::types::HookPayload;
use crate::types::HookResponse;

#[derive(Default, Clone)]
pub struct HooksConfig {
    pub legacy_notify_argv: Option<Vec<Vec<String>>>,
    pub after_user_prompt_submit_argv: Option<Vec<Vec<String>>>,
    pub before_model_request_argv: Option<Vec<Vec<String>>>,
    pub after_model_response_created_argv: Option<Vec<Vec<String>>>,
    pub turn_started_argv: Option<Vec<Vec<String>>>,
    pub turn_completed_argv: Option<Vec<Vec<String>>>,
    pub turn_aborted_argv: Option<Vec<Vec<String>>>,
    pub session_start_argv: Option<Vec<Vec<String>>>,
    pub session_shutdown_argv: Option<Vec<Vec<String>>>,
    pub compaction_argv: Option<Vec<Vec<String>>>,
    pub after_tool_use_argv: Option<Vec<Vec<String>>>,
    pub pre_tool_use_argv: Option<Vec<Vec<String>>>,
    pub tool_failure_argv: Option<Vec<Vec<String>>>,
    pub post_tool_use_success_argv: Option<Vec<Vec<String>>>,
    pub after_model_response_completed_argv: Option<Vec<Vec<String>>>,
}

#[derive(Clone)]
pub struct Hooks {
    after_agent: Vec<Hook>,
    after_user_prompt_submit: Vec<Hook>,
    before_model_request: Vec<Hook>,
    after_model_response_created: Vec<Hook>,
    turn_started: Vec<Hook>,
    turn_completed: Vec<Hook>,
    turn_aborted: Vec<Hook>,
    session_start: Vec<Hook>,
    session_shutdown: Vec<Hook>,
    compaction: Vec<Hook>,
    after_tool_use: Vec<Hook>,
    pre_tool_use: Vec<Hook>,
    tool_failure: Vec<Hook>,
    post_tool_use_success: Vec<Hook>,
    after_model_response_completed: Vec<Hook>,
}

impl Default for Hooks {
    fn default() -> Self {
        Self::new(HooksConfig::default())
    }
}

// Hooks are arbitrary, user-specified functions that are deterministically
// executed after specific events in the Codex lifecycle.
impl Hooks {
    pub fn new(config: HooksConfig) -> Self {
        let HooksConfig {
            legacy_notify_argv,
            after_user_prompt_submit_argv,
            before_model_request_argv,
            after_model_response_created_argv,
            turn_started_argv,
            turn_completed_argv,
            turn_aborted_argv,
            session_start_argv,
            session_shutdown_argv,
            compaction_argv,
            after_tool_use_argv,
            pre_tool_use_argv,
            tool_failure_argv,
            post_tool_use_success_argv,
            after_model_response_completed_argv,
        } = config;

        let after_agent = legacy_notify_argv
            .into_iter()
            .flatten()
            .filter(|argv| !argv.is_empty() && !argv[0].is_empty())
            .map(crate::notify_hook)
            .collect();
        let after_user_prompt_submit = after_user_prompt_submit_argv
            .into_iter()
            .flatten()
            .filter(|argv| !argv.is_empty() && !argv[0].is_empty())
            .map(crate::after_user_prompt_submit_hook)
            .collect();
        let before_model_request = before_model_request_argv
            .into_iter()
            .flatten()
            .filter(|argv| !argv.is_empty() && !argv[0].is_empty())
            .map(|argv| crate::json_payload_hook("before_model_request".to_string(), argv))
            .collect();
        let after_model_response_created = after_model_response_created_argv
            .into_iter()
            .flatten()
            .filter(|argv| !argv.is_empty() && !argv[0].is_empty())
            .map(|argv| crate::json_payload_hook("after_model_response_created".to_string(), argv))
            .collect();
        let turn_started = turn_started_argv
            .into_iter()
            .flatten()
            .filter(|argv| !argv.is_empty() && !argv[0].is_empty())
            .map(|argv| crate::json_payload_hook("turn_started".to_string(), argv))
            .collect();
        let turn_completed = turn_completed_argv
            .into_iter()
            .flatten()
            .filter(|argv| !argv.is_empty() && !argv[0].is_empty())
            .map(|argv| crate::json_payload_hook("turn_completed".to_string(), argv))
            .collect();
        let turn_aborted = turn_aborted_argv
            .into_iter()
            .flatten()
            .filter(|argv| !argv.is_empty() && !argv[0].is_empty())
            .map(|argv| crate::json_payload_hook("turn_aborted".to_string(), argv))
            .collect();
        let session_start = session_start_argv
            .into_iter()
            .flatten()
            .filter(|argv| !argv.is_empty() && !argv[0].is_empty())
            .map(|argv| crate::json_payload_hook("session_start".to_string(), argv))
            .collect();
        let session_shutdown = session_shutdown_argv
            .into_iter()
            .flatten()
            .filter(|argv| !argv.is_empty() && !argv[0].is_empty())
            .map(|argv| crate::json_payload_hook("session_shutdown".to_string(), argv))
            .collect();
        let compaction = compaction_argv
            .into_iter()
            .flatten()
            .filter(|argv| !argv.is_empty() && !argv[0].is_empty())
            .map(|argv| crate::json_payload_hook("compaction".to_string(), argv))
            .collect();
        let after_tool_use = after_tool_use_argv
            .into_iter()
            .flatten()
            .filter(|argv| !argv.is_empty() && !argv[0].is_empty())
            .map(|argv| crate::json_payload_hook("after_tool_use".to_string(), argv))
            .collect();
        let pre_tool_use = pre_tool_use_argv
            .into_iter()
            .flatten()
            .filter(|argv| !argv.is_empty() && !argv[0].is_empty())
            .map(crate::pre_tool_use_hook)
            .collect();
        let tool_failure = tool_failure_argv
            .into_iter()
            .flatten()
            .filter(|argv| !argv.is_empty() && !argv[0].is_empty())
            .map(|argv| crate::json_payload_hook("tool_failure".to_string(), argv))
            .collect();
        let post_tool_use_success = post_tool_use_success_argv
            .into_iter()
            .flatten()
            .filter(|argv| !argv.is_empty() && !argv[0].is_empty())
            .map(|argv| crate::json_payload_hook("post_tool_use_success".to_string(), argv))
            .collect();
        let after_model_response_completed = after_model_response_completed_argv
            .into_iter()
            .flatten()
            .filter(|argv| !argv.is_empty() && !argv[0].is_empty())
            .map(|argv| {
                crate::json_payload_hook("after_model_response_completed".to_string(), argv)
            })
            .collect();
        Self {
            after_agent,
            after_user_prompt_submit,
            before_model_request,
            after_model_response_created,
            turn_started,
            turn_completed,
            turn_aborted,
            session_start,
            session_shutdown,
            compaction,
            after_tool_use,
            pre_tool_use,
            tool_failure,
            post_tool_use_success,
            after_model_response_completed,
        }
    }

    fn hooks_for_event(&self, hook_event: &HookEvent) -> &[Hook] {
        match hook_event {
            HookEvent::AfterAgent { .. } => &self.after_agent,
            HookEvent::AfterUserPromptSubmit { .. } => &self.after_user_prompt_submit,
            HookEvent::BeforeModelRequest { .. } => &self.before_model_request,
            HookEvent::AfterModelResponseCreated { .. } => &self.after_model_response_created,
            HookEvent::TurnStarted { .. } => &self.turn_started,
            HookEvent::TurnCompleted { .. } => &self.turn_completed,
            HookEvent::TurnAborted { .. } => &self.turn_aborted,
            HookEvent::SessionStart { .. } => &self.session_start,
            HookEvent::SessionShutdown { .. } => &self.session_shutdown,
            HookEvent::Compaction { .. } => &self.compaction,
            HookEvent::AfterToolUse { .. } => &self.after_tool_use,
            HookEvent::PreToolUse { .. } => &self.pre_tool_use,
            HookEvent::ToolFailure { .. } => &self.tool_failure,
            HookEvent::PostToolUseSuccess { .. } => &self.post_tool_use_success,
            HookEvent::AfterModelResponseCompleted { .. } => &self.after_model_response_completed,
        }
    }

    pub async fn dispatch(&self, hook_payload: HookPayload) -> Vec<HookResponse> {
        let hooks = self.hooks_for_event(&hook_payload.hook_event);
        let mut outcomes = Vec::with_capacity(hooks.len());
        for hook in hooks {
            let outcome = hook.execute(&hook_payload).await;
            let should_abort_operation = outcome.result.should_abort_operation();
            outcomes.push(outcome);
            if should_abort_operation {
                break;
            }
        }

        outcomes
    }
}

pub fn command_from_argv(argv: &[String]) -> Option<Command> {
    let (program, args) = argv.split_first()?;
    if program.is_empty() {
        return None;
    }
    let mut command = Command::new(program);
    command.args(args);
    Some(command)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::process::Stdio;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    use anyhow::Result;
    use chrono::TimeZone;
    use chrono::Utc;
    use codex_protocol::ThreadId;
    use pretty_assertions::assert_eq;
    use serde_json::to_string;
    use tempfile::tempdir;
    use tokio::time::timeout;

    use super::*;
    use crate::types::HookCompactionStatus;
    use crate::types::HookCompactionStrategy;
    use crate::types::HookCompactionTrigger;
    use crate::types::HookEventAfterAgent;
    use crate::types::HookEventAfterModelResponseCompleted;
    use crate::types::HookEventAfterModelResponseCreated;
    use crate::types::HookEventAfterToolUse;
    use crate::types::HookEventAfterUserPromptSubmit;
    use crate::types::HookEventBeforeModelRequest;
    use crate::types::HookEventCompaction;
    use crate::types::HookEventPostToolUseSuccess;
    use crate::types::HookEventPreToolUse;
    use crate::types::HookEventSessionShutdown;
    use crate::types::HookEventSessionStart;
    use crate::types::HookEventToolFailure;
    use crate::types::HookEventTurnAborted;
    use crate::types::HookEventTurnCompleted;
    use crate::types::HookEventTurnStarted;
    use crate::types::HookResult;
    use crate::types::HookToolInput;
    use crate::types::HookToolKind;

    const CWD: &str = "/tmp";
    const INPUT_MESSAGE: &str = "hello";

    fn hook_payload(label: &str) -> HookPayload {
        HookPayload {
            session_id: ThreadId::new(),
            cwd: PathBuf::from(CWD),
            client: None,
            triggered_at: Utc
                .with_ymd_and_hms(2025, 1, 1, 0, 0, 0)
                .single()
                .expect("valid timestamp"),
            hook_event: HookEvent::AfterAgent {
                event: HookEventAfterAgent {
                    thread_id: ThreadId::new(),
                    turn_id: format!("turn-{label}"),
                    input_messages: vec![INPUT_MESSAGE.to_string()],
                    last_assistant_message: Some("hi".to_string()),
                    proposed_plan: None,
                },
            },
        }
    }

    fn counting_success_hook(calls: &Arc<AtomicUsize>, name: &str) -> Hook {
        let hook_name = name.to_string();
        let calls = Arc::clone(calls);
        Hook {
            name: hook_name,
            func: Arc::new(move |_| {
                let calls = Arc::clone(&calls);
                Box::pin(async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    HookResult::Success
                })
            }),
        }
    }

    fn failing_continue_hook(calls: &Arc<AtomicUsize>, name: &str, message: &str) -> Hook {
        let hook_name = name.to_string();
        let message = message.to_string();
        let calls = Arc::clone(calls);
        Hook {
            name: hook_name,
            func: Arc::new(move |_| {
                let calls = Arc::clone(&calls);
                let message = message.clone();
                Box::pin(async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    HookResult::FailedContinue(std::io::Error::other(message).into())
                })
            }),
        }
    }

    fn failing_abort_hook(calls: &Arc<AtomicUsize>, name: &str, message: &str) -> Hook {
        let hook_name = name.to_string();
        let message = message.to_string();
        let calls = Arc::clone(calls);
        Hook {
            name: hook_name,
            func: Arc::new(move |_| {
                let calls = Arc::clone(&calls);
                let message = message.clone();
                Box::pin(async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    HookResult::FailedAbort(std::io::Error::other(message).into())
                })
            }),
        }
    }

    fn after_tool_use_payload(label: &str) -> HookPayload {
        HookPayload {
            session_id: ThreadId::new(),
            cwd: PathBuf::from(CWD),
            client: None,
            triggered_at: Utc
                .with_ymd_and_hms(2025, 1, 1, 0, 0, 0)
                .single()
                .expect("valid timestamp"),
            hook_event: HookEvent::AfterToolUse {
                event: HookEventAfterToolUse {
                    turn_id: format!("turn-{label}"),
                    call_id: format!("call-{label}"),
                    tool_name: "apply_patch".to_string(),
                    tool_kind: HookToolKind::Custom,
                    tool_input: HookToolInput::Custom {
                        input: "*** Begin Patch".to_string(),
                    },
                    executed: true,
                    success: true,
                    duration_ms: 1,
                    mutating: true,
                    sandbox: "none".to_string(),
                    sandbox_policy: "danger-full-access".to_string(),
                    output_preview: "ok".to_string(),
                },
            },
        }
    }

    fn after_user_prompt_submit_payload(label: &str) -> HookPayload {
        HookPayload {
            session_id: ThreadId::new(),
            cwd: PathBuf::from(CWD),
            client: None,
            triggered_at: Utc
                .with_ymd_and_hms(2025, 1, 1, 0, 0, 0)
                .single()
                .expect("valid timestamp"),
            hook_event: HookEvent::AfterUserPromptSubmit {
                event: HookEventAfterUserPromptSubmit {
                    thread_id: ThreadId::new(),
                    turn_id: format!("turn-{label}"),
                    input_messages: vec![INPUT_MESSAGE.to_string()],
                },
            },
        }
    }

    fn after_model_response_completed_payload(label: &str) -> HookPayload {
        HookPayload {
            session_id: ThreadId::new(),
            cwd: PathBuf::from(CWD),
            client: None,
            triggered_at: Utc
                .with_ymd_and_hms(2025, 1, 1, 0, 0, 0)
                .single()
                .expect("valid timestamp"),
            hook_event: HookEvent::AfterModelResponseCompleted {
                event: HookEventAfterModelResponseCompleted {
                    thread_id: ThreadId::new(),
                    turn_id: format!("turn-{label}"),
                    response_id: format!("resp-{label}"),
                    token_usage: None,
                    needs_follow_up: false,
                    proposed_plan: None,
                },
            },
        }
    }

    fn before_model_request_payload(label: &str) -> HookPayload {
        HookPayload {
            session_id: ThreadId::new(),
            cwd: PathBuf::from(CWD),
            client: None,
            triggered_at: Utc
                .with_ymd_and_hms(2025, 1, 1, 0, 0, 0)
                .single()
                .expect("valid timestamp"),
            hook_event: HookEvent::BeforeModelRequest {
                event: HookEventBeforeModelRequest {
                    thread_id: ThreadId::new(),
                    turn_id: format!("turn-{label}"),
                    model: "gpt-5-codex".to_string(),
                    sampling_request_index: 1,
                    input_messages: vec![INPUT_MESSAGE.to_string()],
                },
            },
        }
    }

    fn after_model_response_created_payload(label: &str) -> HookPayload {
        HookPayload {
            session_id: ThreadId::new(),
            cwd: PathBuf::from(CWD),
            client: None,
            triggered_at: Utc
                .with_ymd_and_hms(2025, 1, 1, 0, 0, 0)
                .single()
                .expect("valid timestamp"),
            hook_event: HookEvent::AfterModelResponseCreated {
                event: HookEventAfterModelResponseCreated {
                    thread_id: ThreadId::new(),
                    turn_id: format!("turn-{label}"),
                    model: "gpt-5-codex".to_string(),
                    sampling_request_index: 1,
                },
            },
        }
    }

    fn turn_started_payload(label: &str) -> HookPayload {
        HookPayload {
            session_id: ThreadId::new(),
            cwd: PathBuf::from(CWD),
            client: None,
            triggered_at: Utc
                .with_ymd_and_hms(2025, 1, 1, 0, 0, 0)
                .single()
                .expect("valid timestamp"),
            hook_event: HookEvent::TurnStarted {
                event: HookEventTurnStarted {
                    thread_id: ThreadId::new(),
                    turn_id: format!("turn-{label}"),
                    model_context_window: Some(200_000),
                    collaboration_mode_kind: codex_protocol::config_types::ModeKind::Default,
                },
            },
        }
    }

    fn turn_completed_payload(label: &str) -> HookPayload {
        HookPayload {
            session_id: ThreadId::new(),
            cwd: PathBuf::from(CWD),
            client: None,
            triggered_at: Utc
                .with_ymd_and_hms(2025, 1, 1, 0, 0, 0)
                .single()
                .expect("valid timestamp"),
            hook_event: HookEvent::TurnCompleted {
                event: HookEventTurnCompleted {
                    thread_id: ThreadId::new(),
                    turn_id: format!("turn-{label}"),
                    last_agent_message: Some("ok".to_string()),
                },
            },
        }
    }

    fn turn_aborted_payload(label: &str) -> HookPayload {
        HookPayload {
            session_id: ThreadId::new(),
            cwd: PathBuf::from(CWD),
            client: None,
            triggered_at: Utc
                .with_ymd_and_hms(2025, 1, 1, 0, 0, 0)
                .single()
                .expect("valid timestamp"),
            hook_event: HookEvent::TurnAborted {
                event: HookEventTurnAborted {
                    thread_id: ThreadId::new(),
                    turn_id: Some(format!("turn-{label}")),
                    reason: codex_protocol::protocol::TurnAbortReason::Interrupted,
                },
            },
        }
    }

    fn session_start_payload() -> HookPayload {
        HookPayload {
            session_id: ThreadId::new(),
            cwd: PathBuf::from(CWD),
            client: None,
            triggered_at: Utc
                .with_ymd_and_hms(2025, 1, 1, 0, 0, 0)
                .single()
                .expect("valid timestamp"),
            hook_event: HookEvent::SessionStart {
                event: HookEventSessionStart {
                    thread_id: ThreadId::new(),
                    model: "gpt-5-codex".to_string(),
                    model_provider_id: "openai".to_string(),
                    cwd: PathBuf::from(CWD),
                },
            },
        }
    }

    fn session_shutdown_payload() -> HookPayload {
        HookPayload {
            session_id: ThreadId::new(),
            cwd: PathBuf::from(CWD),
            client: None,
            triggered_at: Utc
                .with_ymd_and_hms(2025, 1, 1, 0, 0, 0)
                .single()
                .expect("valid timestamp"),
            hook_event: HookEvent::SessionShutdown {
                event: HookEventSessionShutdown {
                    thread_id: ThreadId::new(),
                },
            },
        }
    }

    fn compaction_payload(label: &str) -> HookPayload {
        HookPayload {
            session_id: ThreadId::new(),
            cwd: PathBuf::from(CWD),
            client: None,
            triggered_at: Utc
                .with_ymd_and_hms(2025, 1, 1, 0, 0, 0)
                .single()
                .expect("valid timestamp"),
            hook_event: HookEvent::Compaction {
                event: HookEventCompaction {
                    thread_id: ThreadId::new(),
                    turn_id: format!("turn-{label}"),
                    trigger: HookCompactionTrigger::Manual,
                    strategy: HookCompactionStrategy::Local,
                    status: HookCompactionStatus::Started,
                    error: None,
                },
            },
        }
    }

    fn pre_tool_use_payload(label: &str) -> HookPayload {
        HookPayload {
            session_id: ThreadId::new(),
            cwd: PathBuf::from(CWD),
            client: None,
            triggered_at: Utc
                .with_ymd_and_hms(2025, 1, 1, 0, 0, 0)
                .single()
                .expect("valid timestamp"),
            hook_event: HookEvent::PreToolUse {
                event: HookEventPreToolUse {
                    turn_id: format!("turn-{label}"),
                    call_id: format!("call-{label}"),
                    tool_name: "apply_patch".to_string(),
                    tool_kind: HookToolKind::Custom,
                    tool_input: HookToolInput::Custom {
                        input: "*** Begin Patch".to_string(),
                    },
                    mutating: true,
                    sandbox: "none".to_string(),
                    sandbox_policy: "danger-full-access".to_string(),
                },
            },
        }
    }

    fn tool_failure_payload(label: &str) -> HookPayload {
        HookPayload {
            session_id: ThreadId::new(),
            cwd: PathBuf::from(CWD),
            client: None,
            triggered_at: Utc
                .with_ymd_and_hms(2025, 1, 1, 0, 0, 0)
                .single()
                .expect("valid timestamp"),
            hook_event: HookEvent::ToolFailure {
                event: HookEventToolFailure {
                    turn_id: format!("turn-{label}"),
                    call_id: format!("call-{label}"),
                    tool_name: "apply_patch".to_string(),
                    tool_kind: HookToolKind::Custom,
                    tool_input: HookToolInput::Custom {
                        input: "*** Begin Patch".to_string(),
                    },
                    duration_ms: 1,
                    mutating: true,
                    sandbox: "none".to_string(),
                    sandbox_policy: "danger-full-access".to_string(),
                    error_preview: "tool failed".to_string(),
                },
            },
        }
    }

    fn post_tool_use_success_payload(label: &str) -> HookPayload {
        HookPayload {
            session_id: ThreadId::new(),
            cwd: PathBuf::from(CWD),
            client: None,
            triggered_at: Utc
                .with_ymd_and_hms(2025, 1, 1, 0, 0, 0)
                .single()
                .expect("valid timestamp"),
            hook_event: HookEvent::PostToolUseSuccess {
                event: HookEventPostToolUseSuccess {
                    turn_id: format!("turn-{label}"),
                    call_id: format!("call-{label}"),
                    tool_name: "apply_patch".to_string(),
                    tool_kind: HookToolKind::Custom,
                    tool_input: HookToolInput::Custom {
                        input: "*** Begin Patch".to_string(),
                    },
                    duration_ms: 1,
                    mutating: true,
                    sandbox: "none".to_string(),
                    sandbox_policy: "danger-full-access".to_string(),
                    output_preview: "ok".to_string(),
                },
            },
        }
    }

    #[test]
    fn command_from_argv_returns_none_for_empty_args() {
        assert!(command_from_argv(&[]).is_none());
        assert!(command_from_argv(&["".to_string()]).is_none());
    }

    #[tokio::test]
    async fn command_from_argv_builds_command() -> Result<()> {
        let argv = if cfg!(windows) {
            vec![
                "cmd".to_string(),
                "/C".to_string(),
                "echo hello world".to_string(),
            ]
        } else {
            vec!["echo".to_string(), "hello".to_string(), "world".to_string()]
        };
        let mut command = command_from_argv(&argv).ok_or_else(|| anyhow::anyhow!("command"))?;
        let output = command.stdout(Stdio::piped()).output().await?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let trimmed = stdout.trim_end_matches(['\r', '\n']);
        assert_eq!(trimmed, "hello world");
        Ok(())
    }

    #[test]
    fn hooks_new_requires_program_name() {
        assert!(Hooks::new(HooksConfig::default()).after_agent.is_empty());
        assert!(
            Hooks::new(HooksConfig {
                legacy_notify_argv: Some(vec![]),
                ..HooksConfig::default()
            })
            .after_agent
            .is_empty()
        );
        assert!(
            Hooks::new(HooksConfig {
                legacy_notify_argv: Some(vec![vec!["".to_string()]]),
                ..HooksConfig::default()
            })
            .after_agent
            .is_empty()
        );
        assert_eq!(
            Hooks::new(HooksConfig {
                legacy_notify_argv: Some(vec![vec!["notify-send".to_string()]]),
                ..HooksConfig::default()
            })
            .after_agent
            .len(),
            1
        );
        assert_eq!(
            Hooks::new(HooksConfig {
                after_user_prompt_submit_argv: Some(vec![vec!["python3".to_string()]]),
                ..HooksConfig::default()
            })
            .after_user_prompt_submit
            .len(),
            1
        );
        assert_eq!(
            Hooks::new(HooksConfig {
                after_tool_use_argv: Some(vec![vec!["python3".to_string()]]),
                ..HooksConfig::default()
            })
            .after_tool_use
            .len(),
            1
        );
        assert_eq!(
            Hooks::new(HooksConfig {
                before_model_request_argv: Some(vec![vec!["python3".to_string()]]),
                ..HooksConfig::default()
            })
            .before_model_request
            .len(),
            1
        );
        assert_eq!(
            Hooks::new(HooksConfig {
                after_model_response_created_argv: Some(vec![vec!["python3".to_string()]]),
                ..HooksConfig::default()
            })
            .after_model_response_created
            .len(),
            1
        );
        assert_eq!(
            Hooks::new(HooksConfig {
                turn_started_argv: Some(vec![vec!["python3".to_string()]]),
                ..HooksConfig::default()
            })
            .turn_started
            .len(),
            1
        );
        assert_eq!(
            Hooks::new(HooksConfig {
                turn_completed_argv: Some(vec![vec!["python3".to_string()]]),
                ..HooksConfig::default()
            })
            .turn_completed
            .len(),
            1
        );
        assert_eq!(
            Hooks::new(HooksConfig {
                turn_aborted_argv: Some(vec![vec!["python3".to_string()]]),
                ..HooksConfig::default()
            })
            .turn_aborted
            .len(),
            1
        );
        assert_eq!(
            Hooks::new(HooksConfig {
                session_start_argv: Some(vec![vec!["python3".to_string()]]),
                ..HooksConfig::default()
            })
            .session_start
            .len(),
            1
        );
        assert_eq!(
            Hooks::new(HooksConfig {
                session_shutdown_argv: Some(vec![vec!["python3".to_string()]]),
                ..HooksConfig::default()
            })
            .session_shutdown
            .len(),
            1
        );
        assert_eq!(
            Hooks::new(HooksConfig {
                compaction_argv: Some(vec![vec!["python3".to_string()]]),
                ..HooksConfig::default()
            })
            .compaction
            .len(),
            1
        );
        assert_eq!(
            Hooks::new(HooksConfig {
                pre_tool_use_argv: Some(vec![vec!["python3".to_string()]]),
                ..HooksConfig::default()
            })
            .pre_tool_use
            .len(),
            1
        );
        assert_eq!(
            Hooks::new(HooksConfig {
                tool_failure_argv: Some(vec![vec!["python3".to_string()]]),
                ..HooksConfig::default()
            })
            .tool_failure
            .len(),
            1
        );
        assert_eq!(
            Hooks::new(HooksConfig {
                post_tool_use_success_argv: Some(vec![vec!["python3".to_string()]]),
                ..HooksConfig::default()
            })
            .post_tool_use_success
            .len(),
            1
        );
        assert_eq!(
            Hooks::new(HooksConfig {
                after_model_response_completed_argv: Some(vec![vec!["python3".to_string()]]),
                ..HooksConfig::default()
            })
            .after_model_response_completed
            .len(),
            1
        );
        assert_eq!(
            Hooks::new(HooksConfig {
                after_tool_use_argv: Some(vec![
                    vec!["python3".to_string(), "/tmp/a.py".to_string()],
                    vec!["python3".to_string(), "/tmp/b.py".to_string()],
                ]),
                ..HooksConfig::default()
            })
            .after_tool_use
            .len(),
            2
        );
    }

    #[tokio::test]
    async fn dispatch_executes_hook() {
        let calls = Arc::new(AtomicUsize::new(0));
        let hooks = Hooks {
            after_agent: vec![counting_success_hook(&calls, "counting")],
            ..Hooks::default()
        };

        let outcomes = hooks.dispatch(hook_payload("1")).await;
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].hook_name, "counting");
        assert!(matches!(outcomes[0].result, HookResult::Success));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn default_hook_is_noop_and_continues() {
        let payload = hook_payload("d");
        let outcome = Hook::default().execute(&payload).await;
        assert_eq!(outcome.hook_name, "default");
        assert!(matches!(outcome.result, HookResult::Success));
    }

    #[tokio::test]
    async fn dispatch_executes_multiple_hooks_for_same_event() {
        let calls = Arc::new(AtomicUsize::new(0));
        let hooks = Hooks {
            after_agent: vec![
                counting_success_hook(&calls, "counting-1"),
                counting_success_hook(&calls, "counting-2"),
            ],
            ..Hooks::default()
        };

        let outcomes = hooks.dispatch(hook_payload("2")).await;
        assert_eq!(outcomes.len(), 2);
        assert_eq!(outcomes[0].hook_name, "counting-1");
        assert_eq!(outcomes[1].hook_name, "counting-2");
        assert!(matches!(outcomes[0].result, HookResult::Success));
        assert!(matches!(outcomes[1].result, HookResult::Success));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn dispatch_stops_when_hook_requests_abort() {
        let calls = Arc::new(AtomicUsize::new(0));
        let hooks = Hooks {
            after_agent: vec![
                failing_abort_hook(&calls, "abort", "hook failed"),
                counting_success_hook(&calls, "counting"),
            ],
            ..Hooks::default()
        };

        let outcomes = hooks.dispatch(hook_payload("3")).await;
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].hook_name, "abort");
        assert!(matches!(outcomes[0].result, HookResult::FailedAbort(_)));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn dispatch_executes_after_tool_use_hooks() {
        let calls = Arc::new(AtomicUsize::new(0));
        let hooks = Hooks {
            after_tool_use: vec![counting_success_hook(&calls, "counting")],
            ..Hooks::default()
        };

        let outcomes = hooks.dispatch(after_tool_use_payload("p")).await;
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].hook_name, "counting");
        assert!(matches!(outcomes[0].result, HookResult::Success));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn dispatch_executes_after_user_prompt_submit_hooks() {
        let calls = Arc::new(AtomicUsize::new(0));
        let hooks = Hooks {
            after_user_prompt_submit: vec![counting_success_hook(&calls, "counting")],
            ..Hooks::default()
        };

        let outcomes = hooks.dispatch(after_user_prompt_submit_payload("u")).await;
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].hook_name, "counting");
        assert!(matches!(outcomes[0].result, HookResult::Success));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn dispatch_executes_after_model_response_completed_hooks() {
        let calls = Arc::new(AtomicUsize::new(0));
        let hooks = Hooks {
            after_model_response_completed: vec![counting_success_hook(&calls, "counting")],
            ..Hooks::default()
        };

        let outcomes = hooks
            .dispatch(after_model_response_completed_payload("m"))
            .await;
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].hook_name, "counting");
        assert!(matches!(outcomes[0].result, HookResult::Success));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn dispatch_executes_before_model_request_hooks() {
        let calls = Arc::new(AtomicUsize::new(0));
        let hooks = Hooks {
            before_model_request: vec![counting_success_hook(&calls, "counting")],
            ..Hooks::default()
        };

        let outcomes = hooks.dispatch(before_model_request_payload("b")).await;
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].hook_name, "counting");
        assert!(matches!(outcomes[0].result, HookResult::Success));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn dispatch_executes_after_model_response_created_hooks() {
        let calls = Arc::new(AtomicUsize::new(0));
        let hooks = Hooks {
            after_model_response_created: vec![counting_success_hook(&calls, "counting")],
            ..Hooks::default()
        };

        let outcomes = hooks
            .dispatch(after_model_response_created_payload("c"))
            .await;
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].hook_name, "counting");
        assert!(matches!(outcomes[0].result, HookResult::Success));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn dispatch_executes_turn_hooks() {
        let calls = Arc::new(AtomicUsize::new(0));
        let hooks = Hooks {
            turn_started: vec![counting_success_hook(&calls, "started")],
            turn_completed: vec![counting_success_hook(&calls, "completed")],
            turn_aborted: vec![counting_success_hook(&calls, "aborted")],
            ..Hooks::default()
        };

        let started = hooks.dispatch(turn_started_payload("s")).await;
        assert_eq!(started.len(), 1);
        assert_eq!(started[0].hook_name, "started");
        assert!(matches!(started[0].result, HookResult::Success));

        let completed = hooks.dispatch(turn_completed_payload("c")).await;
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].hook_name, "completed");
        assert!(matches!(completed[0].result, HookResult::Success));

        let aborted = hooks.dispatch(turn_aborted_payload("a")).await;
        assert_eq!(aborted.len(), 1);
        assert_eq!(aborted[0].hook_name, "aborted");
        assert!(matches!(aborted[0].result, HookResult::Success));

        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn dispatch_executes_session_hooks() {
        let calls = Arc::new(AtomicUsize::new(0));
        let hooks = Hooks {
            session_start: vec![counting_success_hook(&calls, "start")],
            session_shutdown: vec![counting_success_hook(&calls, "shutdown")],
            ..Hooks::default()
        };

        let start = hooks.dispatch(session_start_payload()).await;
        assert_eq!(start.len(), 1);
        assert_eq!(start[0].hook_name, "start");
        assert!(matches!(start[0].result, HookResult::Success));

        let shutdown = hooks.dispatch(session_shutdown_payload()).await;
        assert_eq!(shutdown.len(), 1);
        assert_eq!(shutdown[0].hook_name, "shutdown");
        assert!(matches!(shutdown[0].result, HookResult::Success));

        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn dispatch_executes_compaction_hooks() {
        let calls = Arc::new(AtomicUsize::new(0));
        let hooks = Hooks {
            compaction: vec![counting_success_hook(&calls, "compaction")],
            ..Hooks::default()
        };

        let outcomes = hooks.dispatch(compaction_payload("cmp")).await;
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].hook_name, "compaction");
        assert!(matches!(outcomes[0].result, HookResult::Success));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn dispatch_executes_pre_tool_use_hooks() {
        let calls = Arc::new(AtomicUsize::new(0));
        let hooks = Hooks {
            pre_tool_use: vec![counting_success_hook(&calls, "counting")],
            ..Hooks::default()
        };

        let outcomes = hooks.dispatch(pre_tool_use_payload("pre")).await;
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].hook_name, "counting");
        assert!(matches!(outcomes[0].result, HookResult::Success));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn dispatch_executes_tool_failure_hooks() {
        let calls = Arc::new(AtomicUsize::new(0));
        let hooks = Hooks {
            tool_failure: vec![counting_success_hook(&calls, "counting")],
            ..Hooks::default()
        };

        let outcomes = hooks.dispatch(tool_failure_payload("fail")).await;
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].hook_name, "counting");
        assert!(matches!(outcomes[0].result, HookResult::Success));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn dispatch_executes_post_tool_use_success_hooks() {
        let calls = Arc::new(AtomicUsize::new(0));
        let hooks = Hooks {
            post_tool_use_success: vec![counting_success_hook(&calls, "counting")],
            ..Hooks::default()
        };

        let outcomes = hooks.dispatch(post_tool_use_success_payload("post")).await;
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].hook_name, "counting");
        assert!(matches!(outcomes[0].result, HookResult::Success));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn dispatch_continues_after_continueable_failure() {
        let calls = Arc::new(AtomicUsize::new(0));
        let hooks = Hooks {
            after_agent: vec![
                failing_continue_hook(&calls, "failing", "hook failed"),
                counting_success_hook(&calls, "counting"),
            ],
            ..Hooks::default()
        };

        let outcomes = hooks.dispatch(hook_payload("err")).await;
        assert_eq!(outcomes.len(), 2);
        assert_eq!(outcomes[0].hook_name, "failing");
        assert!(matches!(outcomes[0].result, HookResult::FailedContinue(_)));
        assert_eq!(outcomes[1].hook_name, "counting");
        assert!(matches!(outcomes[1].result, HookResult::Success));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn dispatch_returns_after_tool_use_failure_outcome() {
        let calls = Arc::new(AtomicUsize::new(0));
        let hooks = Hooks {
            after_tool_use: vec![failing_continue_hook(
                &calls,
                "failing",
                "after_tool_use hook failed",
            )],
            ..Hooks::default()
        };

        let outcomes = hooks.dispatch(after_tool_use_payload("err-tool")).await;
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].hook_name, "failing");
        assert!(matches!(outcomes[0].result, HookResult::FailedContinue(_)));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn hook_executes_program_with_payload_argument_unix() -> Result<()> {
        let temp_dir = tempdir()?;
        let payload_path = temp_dir.path().join("payload.json");
        let payload_path_arg = payload_path.to_string_lossy().into_owned();
        let hook = Hook {
            name: "write_payload".to_string(),
            func: Arc::new(move |payload: &HookPayload| {
                let payload_path_arg = payload_path_arg.clone();
                Box::pin(async move {
                    let json = to_string(payload).expect("serialize hook payload");
                    let mut command = command_from_argv(&[
                        "/bin/sh".to_string(),
                        "-c".to_string(),
                        "printf '%s' \"$2\" > \"$1\"".to_string(),
                        "sh".to_string(),
                        payload_path_arg,
                        json,
                    ])
                    .expect("build command");
                    command.status().await.expect("run hook command");
                    HookResult::Success
                })
            }),
        };

        let payload = hook_payload("4");
        let expected = to_string(&payload)?;

        let hooks = Hooks {
            after_agent: vec![hook],
            ..Hooks::default()
        };
        let outcomes = hooks.dispatch(payload).await;
        assert_eq!(outcomes.len(), 1);
        assert!(matches!(outcomes[0].result, HookResult::Success));

        let contents = timeout(Duration::from_secs(2), async {
            loop {
                if let Ok(contents) = fs::read_to_string(&payload_path)
                    && !contents.is_empty()
                {
                    return contents;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await?;

        assert_eq!(contents, expected);
        Ok(())
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn hook_executes_program_with_payload_argument_windows() -> Result<()> {
        let temp_dir = tempdir()?;
        let payload_path = temp_dir.path().join("payload.json");
        let payload_path_arg = payload_path.to_string_lossy().into_owned();
        let script_path = temp_dir.path().join("write_payload.ps1");
        fs::write(&script_path, "[IO.File]::WriteAllText($args[0], $args[1])")?;
        let script_path_arg = script_path.to_string_lossy().into_owned();
        let hook = Hook {
            name: "write_payload".to_string(),
            func: Arc::new(move |payload: &HookPayload| {
                let payload_path_arg = payload_path_arg.clone();
                let script_path_arg = script_path_arg.clone();
                Box::pin(async move {
                    let json = to_string(payload).expect("serialize hook payload");
                    let mut command = command_from_argv(&[
                        "powershell.exe".to_string(),
                        "-NoLogo".to_string(),
                        "-NoProfile".to_string(),
                        "-ExecutionPolicy".to_string(),
                        "Bypass".to_string(),
                        "-File".to_string(),
                        script_path_arg,
                        payload_path_arg,
                        json,
                    ])
                    .expect("build command");
                    command.status().await.expect("run hook command");
                    HookResult::Success
                })
            }),
        };

        let payload = hook_payload("4");
        let expected = to_string(&payload)?;

        let hooks = Hooks {
            after_agent: vec![hook],
            ..Hooks::default()
        };
        let outcomes = hooks.dispatch(payload).await;
        assert_eq!(outcomes.len(), 1);
        assert!(matches!(outcomes[0].result, HookResult::Success));

        let contents = timeout(Duration::from_secs(2), async {
            loop {
                if let Ok(contents) = fs::read_to_string(&payload_path)
                    && !contents.is_empty()
                {
                    return contents;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await?;

        assert_eq!(contents, expected);
        Ok(())
    }
}
