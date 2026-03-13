use codex_config::ConfigLayerStack;
use tokio::process::Command;

use crate::engine::ClaudeHooksEngine;
use crate::engine::CommandShell;
use crate::events::session_start::SessionStartOutcome;
use crate::events::session_start::SessionStartRequest;
use crate::events::stop::StopOutcome;
use crate::events::stop::StopRequest;
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
    pub plan_finalized_argv: Option<Vec<Vec<String>>>,
    pub turn_started_argv: Option<Vec<Vec<String>>>,
    pub turn_completed_argv: Option<Vec<Vec<String>>>,
    pub plan_implementation_completed_argv: Option<Vec<Vec<String>>>,
    pub turn_aborted_argv: Option<Vec<Vec<String>>>,
    pub session_start_argv: Option<Vec<Vec<String>>>,
    pub session_shutdown_argv: Option<Vec<Vec<String>>>,
    pub compaction_argv: Option<Vec<Vec<String>>>,
    pub after_tool_use_argv: Option<Vec<Vec<String>>>,
    pub pre_tool_use_argv: Option<Vec<Vec<String>>>,
    pub tool_failure_argv: Option<Vec<Vec<String>>>,
    pub post_tool_use_success_argv: Option<Vec<Vec<String>>>,
    pub after_model_response_completed_argv: Option<Vec<Vec<String>>>,
    pub feature_enabled: bool,
    pub config_layer_stack: Option<ConfigLayerStack>,
    pub shell_program: Option<String>,
    pub shell_args: Vec<String>,
}

#[derive(Clone)]
pub struct Hooks {
    after_agent: Vec<Hook>,
    after_user_prompt_submit: Vec<Hook>,
    before_model_request: Vec<Hook>,
    after_model_response_created: Vec<Hook>,
    plan_finalized: Vec<Hook>,
    turn_started: Vec<Hook>,
    turn_completed: Vec<Hook>,
    plan_implementation_completed: Vec<Hook>,
    turn_aborted: Vec<Hook>,
    session_start: Vec<Hook>,
    session_shutdown: Vec<Hook>,
    compaction: Vec<Hook>,
    after_tool_use: Vec<Hook>,
    pre_tool_use: Vec<Hook>,
    tool_failure: Vec<Hook>,
    post_tool_use_success: Vec<Hook>,
    after_model_response_completed: Vec<Hook>,
    engine: ClaudeHooksEngine,
}

impl Default for Hooks {
    fn default() -> Self {
        Self::new(HooksConfig::default())
    }
}

impl Hooks {
    pub fn new(config: HooksConfig) -> Self {
        let HooksConfig {
            legacy_notify_argv,
            after_user_prompt_submit_argv,
            before_model_request_argv,
            after_model_response_created_argv,
            plan_finalized_argv,
            turn_started_argv,
            turn_completed_argv,
            plan_implementation_completed_argv,
            turn_aborted_argv,
            session_start_argv,
            session_shutdown_argv,
            compaction_argv,
            after_tool_use_argv,
            pre_tool_use_argv,
            tool_failure_argv,
            post_tool_use_success_argv,
            after_model_response_completed_argv,
            feature_enabled,
            config_layer_stack,
            shell_program,
            shell_args,
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
        let plan_finalized = plan_finalized_argv
            .into_iter()
            .flatten()
            .filter(|argv| !argv.is_empty() && !argv[0].is_empty())
            .map(|argv| crate::json_payload_hook("plan_finalized".to_string(), argv))
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
        let plan_implementation_completed = plan_implementation_completed_argv
            .into_iter()
            .flatten()
            .filter(|argv| !argv.is_empty() && !argv[0].is_empty())
            .map(|argv| crate::json_payload_hook("plan_implementation_completed".to_string(), argv))
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
        let engine = ClaudeHooksEngine::new(
            feature_enabled,
            config_layer_stack.as_ref(),
            CommandShell {
                program: shell_program.unwrap_or_default(),
                args: shell_args,
            },
        );
        Self {
            after_agent,
            after_user_prompt_submit,
            before_model_request,
            after_model_response_created,
            plan_finalized,
            turn_started,
            turn_completed,
            plan_implementation_completed,
            turn_aborted,
            session_start,
            session_shutdown,
            compaction,
            after_tool_use,
            pre_tool_use,
            tool_failure,
            post_tool_use_success,
            after_model_response_completed,
            engine,
        }
    }

    pub fn startup_warnings(&self) -> &[String] {
        self.engine.warnings()
    }

    fn hooks_for_event(&self, hook_event: &HookEvent) -> &[Hook] {
        match hook_event {
            HookEvent::AfterAgent { .. } => &self.after_agent,
            HookEvent::AfterUserPromptSubmit { .. } => &self.after_user_prompt_submit,
            HookEvent::BeforeModelRequest { .. } => &self.before_model_request,
            HookEvent::AfterModelResponseCreated { .. } => &self.after_model_response_created,
            HookEvent::PlanFinalized { .. } => &self.plan_finalized,
            HookEvent::TurnStarted { .. } => &self.turn_started,
            HookEvent::TurnCompleted { .. } => &self.turn_completed,
            HookEvent::PlanImplementationCompleted { .. } => &self.plan_implementation_completed,
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

    pub fn preview_session_start(
        &self,
        request: &SessionStartRequest,
    ) -> Vec<codex_protocol::protocol::HookRunSummary> {
        self.engine.preview_session_start(request)
    }

    pub async fn run_session_start(
        &self,
        request: SessionStartRequest,
        turn_id: Option<String>,
    ) -> SessionStartOutcome {
        self.engine.run_session_start(request, turn_id).await
    }

    pub fn preview_stop(
        &self,
        request: &StopRequest,
    ) -> Vec<codex_protocol::protocol::HookRunSummary> {
        self.engine.preview_stop(request)
    }

    pub async fn run_stop(&self, request: StopRequest) -> StopOutcome {
        self.engine.run_stop(request).await
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
    use std::path::PathBuf;
    use std::sync::Arc;

    use chrono::TimeZone;
    use chrono::Utc;
    use codex_protocol::ThreadId;
    use pretty_assertions::assert_eq;
    use tokio::sync::Mutex;

    use crate::Hook;
    use crate::HookEvent;
    use crate::HookEventPlanFinalized;
    use crate::HookEventTurnCompleted;
    use crate::HookPayload;
    use crate::HookResult;
    use crate::Hooks;
    use crate::HooksConfig;

    fn test_payload() -> HookPayload {
        HookPayload {
            session_id: ThreadId::new(),
            cwd: PathBuf::from("/tmp"),
            client: Some("codex-tui".to_string()),
            triggered_at: Utc
                .with_ymd_and_hms(2025, 1, 1, 0, 0, 0)
                .single()
                .expect("valid timestamp"),
            hook_event: HookEvent::TurnCompleted {
                event: HookEventTurnCompleted {
                    thread_id: ThreadId::new(),
                    turn_id: "turn-1".to_string(),
                    last_agent_message: Some("done".to_string()),
                },
            },
        }
    }

    fn recording_hook(name: &str, calls: Arc<Mutex<Vec<String>>>, result: HookResult) -> Hook {
        let name_string = name.to_string();
        Hook {
            name: name_string.clone(),
            func: Arc::new(move |_| {
                let calls = Arc::clone(&calls);
                let hook_name = name_string.clone();
                let result = match &result {
                    HookResult::Success => HookResult::Success,
                    HookResult::SuccessWithPromptAugmentation {
                        append_prompt_text,
                        switch_to_plan_mode,
                    } => HookResult::SuccessWithPromptAugmentation {
                        append_prompt_text: append_prompt_text.clone(),
                        switch_to_plan_mode: *switch_to_plan_mode,
                    },
                    HookResult::SuccessWithPreToolUseDecision(decision) => {
                        HookResult::SuccessWithPreToolUseDecision(decision.clone())
                    }
                    HookResult::FailedContinue(error) => {
                        HookResult::FailedContinue(std::io::Error::other(error.to_string()).into())
                    }
                    HookResult::FailedAbort(error) => {
                        HookResult::FailedAbort(std::io::Error::other(error.to_string()).into())
                    }
                };
                Box::pin(async move {
                    calls.lock().await.push(hook_name);
                    result
                })
            }),
        }
    }

    #[tokio::test]
    async fn dispatch_runs_all_hooks_for_event_in_order() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let hooks = Hooks {
            turn_completed: vec![
                recording_hook("first", Arc::clone(&calls), HookResult::Success),
                recording_hook("second", Arc::clone(&calls), HookResult::Success),
                recording_hook("third", Arc::clone(&calls), HookResult::Success),
            ],
            ..Hooks::default()
        };

        let outcomes = hooks.dispatch(test_payload()).await;

        assert_eq!(
            calls.lock().await.clone(),
            vec![
                "first".to_string(),
                "second".to_string(),
                "third".to_string()
            ]
        );
        assert_eq!(outcomes.len(), 3);
        assert_eq!(outcomes[0].hook_name, "first");
        assert_eq!(outcomes[1].hook_name, "second");
        assert_eq!(outcomes[2].hook_name, "third");
    }

    #[tokio::test]
    async fn dispatch_stops_after_failed_abort() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let hooks = Hooks {
            turn_completed: vec![
                recording_hook("first", Arc::clone(&calls), HookResult::Success),
                recording_hook(
                    "aborter",
                    Arc::clone(&calls),
                    HookResult::FailedAbort(std::io::Error::other("stop").into()),
                ),
                recording_hook("third", Arc::clone(&calls), HookResult::Success),
            ],
            ..Hooks::default()
        };

        let outcomes = hooks.dispatch(test_payload()).await;

        assert_eq!(
            calls.lock().await.clone(),
            vec!["first".to_string(), "aborter".to_string()]
        );
        assert_eq!(outcomes.len(), 2);
        assert_eq!(outcomes[0].hook_name, "first");
        assert_eq!(outcomes[1].hook_name, "aborter");
        assert!(matches!(outcomes[1].result, HookResult::FailedAbort(_)));
    }

    #[tokio::test]
    async fn dispatch_continues_after_failed_continue() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let hooks = Hooks {
            turn_completed: vec![
                recording_hook("first", Arc::clone(&calls), HookResult::Success),
                recording_hook(
                    "continue-on-error",
                    Arc::clone(&calls),
                    HookResult::FailedContinue(std::io::Error::other("keep going").into()),
                ),
                recording_hook("third", Arc::clone(&calls), HookResult::Success),
            ],
            ..Hooks::default()
        };

        let outcomes = hooks.dispatch(test_payload()).await;

        assert_eq!(
            calls.lock().await.clone(),
            vec![
                "first".to_string(),
                "continue-on-error".to_string(),
                "third".to_string()
            ]
        );
        assert_eq!(outcomes.len(), 3);
        assert!(matches!(outcomes[1].result, HookResult::FailedContinue(_)));
    }

    #[test]
    fn hooks_config_preserves_multiple_commands_for_same_event() {
        let hooks = Hooks::new(HooksConfig {
            turn_completed_argv: Some(vec![
                vec!["/bin/echo".to_string(), "first".to_string()],
                vec!["/bin/echo".to_string(), "second".to_string()],
            ]),
            ..HooksConfig::default()
        });

        assert_eq!(hooks.turn_completed.len(), 2);
        assert_eq!(hooks.turn_completed[0].name, "turn_completed");
        assert_eq!(hooks.turn_completed[1].name, "turn_completed");
    }

    #[test]
    fn hooks_config_preserves_multiple_commands_for_plan_finalized() {
        let hooks = Hooks::new(HooksConfig {
            plan_finalized_argv: Some(vec![
                vec!["/bin/echo".to_string(), "first".to_string()],
                vec!["/bin/echo".to_string(), "second".to_string()],
            ]),
            ..HooksConfig::default()
        });

        assert_eq!(hooks.plan_finalized.len(), 2);
        assert_eq!(hooks.plan_finalized[0].name, "plan_finalized");
        assert_eq!(hooks.plan_finalized[1].name, "plan_finalized");
    }

    #[test]
    fn hooks_config_preserves_multiple_commands_for_plan_implementation_completed() {
        let hooks = Hooks::new(HooksConfig {
            plan_implementation_completed_argv: Some(vec![
                vec!["/bin/echo".to_string(), "first".to_string()],
                vec!["/bin/echo".to_string(), "second".to_string()],
            ]),
            ..HooksConfig::default()
        });

        assert_eq!(hooks.plan_implementation_completed.len(), 2);
        assert_eq!(
            hooks.plan_implementation_completed[0].name,
            "plan_implementation_completed"
        );
        assert_eq!(
            hooks.plan_implementation_completed[1].name,
            "plan_implementation_completed"
        );
    }

    #[tokio::test]
    async fn dispatch_uses_plan_finalized_hooks_for_plan_finalized_event() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let hooks = Hooks {
            plan_finalized: vec![recording_hook(
                "plan-finalized",
                Arc::clone(&calls),
                HookResult::Success,
            )],
            ..Hooks::default()
        };
        let payload = HookPayload {
            session_id: ThreadId::new(),
            cwd: PathBuf::from("/tmp"),
            client: Some("codex-tui".to_string()),
            triggered_at: Utc
                .with_ymd_and_hms(2025, 1, 1, 0, 0, 0)
                .single()
                .expect("valid timestamp"),
            hook_event: HookEvent::PlanFinalized {
                event: HookEventPlanFinalized {
                    thread_id: ThreadId::new(),
                    turn_id: "turn-1".to_string(),
                    plan_id: "thread:turn-1".to_string(),
                    plan_text: "Implement the feature".to_string(),
                    parent_thread_id: None,
                    original_user_request: Some("Implement the feature".to_string()),
                    plan_summary: Some("Implement the feature".to_string()),
                },
            },
        };

        let outcomes = hooks.dispatch(payload).await;

        assert_eq!(
            calls.lock().await.clone(),
            vec!["plan-finalized".to_string()]
        );
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].hook_name, "plan-finalized");
    }
}
