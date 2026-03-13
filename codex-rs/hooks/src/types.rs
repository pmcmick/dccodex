use std::path::PathBuf;
use std::sync::Arc;

use chrono::DateTime;
use chrono::SecondsFormat;
use chrono::Utc;
use codex_protocol::ThreadId;
use codex_protocol::config_types::ModeKind;
use codex_protocol::models::SandboxPermissions;
use codex_protocol::protocol::TokenUsage;
use codex_protocol::protocol::TurnAbortReason;
use futures::future::BoxFuture;
use serde::Serialize;
use serde::Serializer;

pub type HookFn = Arc<dyn for<'a> Fn(&'a HookPayload) -> BoxFuture<'a, HookResult> + Send + Sync>;

#[derive(Debug)]
pub enum HookResult {
    /// Success: hook completed successfully.
    Success,
    /// SuccessWithPromptAugmentation: hook completed successfully and returned
    /// optional prompt augmentation actions for the submitted user prompt.
    SuccessWithPromptAugmentation {
        append_prompt_text: Option<String>,
        switch_to_plan_mode: bool,
    },
    /// SuccessWithPreToolUseDecision: pre_tool_use hook completed successfully
    /// and returned a tool dispatch decision.
    SuccessWithPreToolUseDecision(HookPreToolUseDecision),
    /// FailedContinue: hook failed, but other subsequent hooks should still execute and the
    /// operation should continue.
    FailedContinue(Box<dyn std::error::Error + Send + Sync + 'static>),
    /// FailedAbort: hook failed, other subsequent hooks should not execute, and the operation
    /// should be aborted.
    FailedAbort(Box<dyn std::error::Error + Send + Sync + 'static>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookPreToolUseDecision {
    Deny { message: String },
    Replace { output: String, success: bool },
}

impl HookResult {
    pub fn should_abort_operation(&self) -> bool {
        matches!(self, Self::FailedAbort(_))
    }

    pub fn appended_user_prompt(&self) -> Option<&str> {
        match self {
            Self::SuccessWithPromptAugmentation {
                append_prompt_text: Some(text),
                ..
            } => Some(text),
            _ => None,
        }
    }

    pub fn switch_to_plan_mode(&self) -> bool {
        match self {
            Self::SuccessWithPromptAugmentation {
                switch_to_plan_mode,
                ..
            } => *switch_to_plan_mode,
            _ => false,
        }
    }
}

#[derive(Debug)]
pub struct HookResponse {
    pub hook_name: String,
    pub result: HookResult,
}

#[derive(Clone)]
pub struct Hook {
    pub name: String,
    pub func: HookFn,
}

impl Default for Hook {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            func: Arc::new(|_| Box::pin(async { HookResult::Success })),
        }
    }
}

impl Hook {
    pub async fn execute(&self, payload: &HookPayload) -> HookResponse {
        HookResponse {
            hook_name: self.name.clone(),
            result: (self.func)(payload).await,
        }
    }
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct HookPayload {
    pub session_id: ThreadId,
    pub cwd: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
    #[serde(serialize_with = "serialize_triggered_at")]
    pub triggered_at: DateTime<Utc>,
    pub hook_event: HookEvent,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct HookEventAfterAgent {
    pub thread_id: ThreadId,
    pub turn_id: String,
    pub input_messages: Vec<String>,
    pub last_assistant_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proposed_plan: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct HookEventAfterUserPromptSubmit {
    pub thread_id: ThreadId,
    pub turn_id: String,
    pub input_messages: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct HookEventBeforeModelRequest {
    pub thread_id: ThreadId,
    pub turn_id: String,
    pub model: String,
    pub sampling_request_index: u32,
    pub input_messages: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct HookEventAfterModelResponseCreated {
    pub thread_id: ThreadId,
    pub turn_id: String,
    pub model: String,
    pub sampling_request_index: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct HookEventAfterModelResponseCompleted {
    pub thread_id: ThreadId,
    pub turn_id: String,
    pub response_id: String,
    pub token_usage: Option<TokenUsage>,
    pub needs_follow_up: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proposed_plan: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct HookEventPlanFinalized {
    pub thread_id: ThreadId,
    pub turn_id: String,
    pub plan_id: String,
    pub plan_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_thread_id: Option<ThreadId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_user_request: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct HookEventTurnStarted {
    pub thread_id: ThreadId,
    pub turn_id: String,
    pub model_context_window: Option<i64>,
    pub collaboration_mode_kind: ModeKind,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct HookEventTurnCompleted {
    pub thread_id: ThreadId,
    pub turn_id: String,
    pub last_agent_message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct HookEventPlanImplementationCompleted {
    pub thread_id: ThreadId,
    pub parent_thread_id: ThreadId,
    pub turn_id: String,
    pub last_agent_message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct HookEventTurnAborted {
    pub thread_id: ThreadId,
    pub turn_id: Option<String>,
    pub reason: TurnAbortReason,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct HookEventSessionStart {
    pub thread_id: ThreadId,
    pub model: String,
    pub model_provider_id: String,
    pub cwd: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct HookEventSessionShutdown {
    pub thread_id: ThreadId,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HookCompactionTrigger {
    Manual,
    AutoPreTurn,
    AutoMidTurn,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HookCompactionStrategy {
    Local,
    Remote,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HookCompactionStatus {
    Started,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct HookEventCompaction {
    pub thread_id: ThreadId,
    pub turn_id: String,
    pub trigger: HookCompactionTrigger,
    pub strategy: HookCompactionStrategy,
    pub status: HookCompactionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HookToolKind {
    Function,
    Custom,
    LocalShell,
    Mcp,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct HookToolInputLocalShell {
    pub command: Vec<String>,
    pub workdir: Option<String>,
    pub timeout_ms: Option<u64>,
    pub sandbox_permissions: Option<SandboxPermissions>,
    pub prefix_rule: Option<Vec<String>>,
    pub justification: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "input_type", rename_all = "snake_case")]
pub enum HookToolInput {
    Function {
        arguments: String,
    },
    Custom {
        input: String,
    },
    LocalShell {
        params: HookToolInputLocalShell,
    },
    Mcp {
        server: String,
        tool: String,
        arguments: String,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct HookEventAfterToolUse {
    pub turn_id: String,
    pub call_id: String,
    pub tool_name: String,
    pub tool_kind: HookToolKind,
    pub tool_input: HookToolInput,
    pub executed: bool,
    pub success: bool,
    pub duration_ms: u64,
    pub mutating: bool,
    pub sandbox: String,
    pub sandbox_policy: String,
    pub output_preview: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct HookEventPreToolUse {
    pub turn_id: String,
    pub call_id: String,
    pub tool_name: String,
    pub tool_kind: HookToolKind,
    pub tool_input: HookToolInput,
    pub mutating: bool,
    pub sandbox: String,
    pub sandbox_policy: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct HookEventToolFailure {
    pub turn_id: String,
    pub call_id: String,
    pub tool_name: String,
    pub tool_kind: HookToolKind,
    pub tool_input: HookToolInput,
    pub duration_ms: u64,
    pub mutating: bool,
    pub sandbox: String,
    pub sandbox_policy: String,
    pub error_preview: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct HookEventPostToolUseSuccess {
    pub turn_id: String,
    pub call_id: String,
    pub tool_name: String,
    pub tool_kind: HookToolKind,
    pub tool_input: HookToolInput,
    pub duration_ms: u64,
    pub mutating: bool,
    pub sandbox: String,
    pub sandbox_policy: String,
    pub output_preview: String,
}

fn serialize_triggered_at<S>(value: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&value.to_rfc3339_opts(SecondsFormat::Secs, true))
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event_type", rename_all = "snake_case")]
pub enum HookEvent {
    AfterAgent {
        #[serde(flatten)]
        event: HookEventAfterAgent,
    },
    AfterUserPromptSubmit {
        #[serde(flatten)]
        event: HookEventAfterUserPromptSubmit,
    },
    BeforeModelRequest {
        #[serde(flatten)]
        event: HookEventBeforeModelRequest,
    },
    AfterModelResponseCreated {
        #[serde(flatten)]
        event: HookEventAfterModelResponseCreated,
    },
    PlanFinalized {
        #[serde(flatten)]
        event: HookEventPlanFinalized,
    },
    TurnStarted {
        #[serde(flatten)]
        event: HookEventTurnStarted,
    },
    TurnCompleted {
        #[serde(flatten)]
        event: HookEventTurnCompleted,
    },
    PlanImplementationCompleted {
        #[serde(flatten)]
        event: HookEventPlanImplementationCompleted,
    },
    TurnAborted {
        #[serde(flatten)]
        event: HookEventTurnAborted,
    },
    SessionStart {
        #[serde(flatten)]
        event: HookEventSessionStart,
    },
    SessionShutdown {
        #[serde(flatten)]
        event: HookEventSessionShutdown,
    },
    Compaction {
        #[serde(flatten)]
        event: HookEventCompaction,
    },
    AfterToolUse {
        #[serde(flatten)]
        event: HookEventAfterToolUse,
    },
    PreToolUse {
        #[serde(flatten)]
        event: HookEventPreToolUse,
    },
    ToolFailure {
        #[serde(flatten)]
        event: HookEventToolFailure,
    },
    PostToolUseSuccess {
        #[serde(flatten)]
        event: HookEventPostToolUseSuccess,
    },
    AfterModelResponseCompleted {
        #[serde(flatten)]
        event: HookEventAfterModelResponseCompleted,
    },
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use chrono::TimeZone;
    use chrono::Utc;
    use codex_protocol::ThreadId;
    use codex_protocol::models::SandboxPermissions;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::HookEvent;
    use super::HookEventAfterAgent;
    use super::HookEventAfterToolUse;
    use super::HookEventPlanFinalized;
    use super::HookEventPlanImplementationCompleted;
    use super::HookPayload;
    use super::HookToolInput;
    use super::HookToolInputLocalShell;
    use super::HookToolKind;

    #[test]
    fn hook_payload_serializes_stable_wire_shape() {
        let session_id = ThreadId::new();
        let thread_id = ThreadId::new();
        let payload = HookPayload {
            session_id,
            cwd: PathBuf::from("tmp"),
            client: None,
            triggered_at: Utc
                .with_ymd_and_hms(2025, 1, 1, 0, 0, 0)
                .single()
                .expect("valid timestamp"),
            hook_event: HookEvent::AfterAgent {
                event: HookEventAfterAgent {
                    thread_id,
                    turn_id: "turn-1".to_string(),
                    input_messages: vec!["hello".to_string()],
                    last_assistant_message: Some("hi".to_string()),
                    proposed_plan: None,
                },
            },
        };

        let actual = serde_json::to_value(payload).expect("serialize hook payload");
        let expected = json!({
            "session_id": session_id.to_string(),
            "cwd": "tmp",
            "triggered_at": "2025-01-01T00:00:00Z",
            "hook_event": {
                "event_type": "after_agent",
                "thread_id": thread_id.to_string(),
                "turn_id": "turn-1",
                "input_messages": ["hello"],
                "last_assistant_message": "hi",
            },
        });

        assert_eq!(actual, expected);
    }

    #[test]
    fn after_tool_use_payload_serializes_stable_wire_shape() {
        let session_id = ThreadId::new();
        let payload = HookPayload {
            session_id,
            cwd: PathBuf::from("tmp"),
            client: None,
            triggered_at: Utc
                .with_ymd_and_hms(2025, 1, 1, 0, 0, 0)
                .single()
                .expect("valid timestamp"),
            hook_event: HookEvent::AfterToolUse {
                event: HookEventAfterToolUse {
                    turn_id: "turn-2".to_string(),
                    call_id: "call-1".to_string(),
                    tool_name: "local_shell".to_string(),
                    tool_kind: HookToolKind::LocalShell,
                    tool_input: HookToolInput::LocalShell {
                        params: HookToolInputLocalShell {
                            command: vec!["cargo".to_string(), "fmt".to_string()],
                            workdir: Some("codex-rs".to_string()),
                            timeout_ms: Some(60_000),
                            sandbox_permissions: Some(SandboxPermissions::UseDefault),
                            justification: None,
                            prefix_rule: None,
                        },
                    },
                    executed: true,
                    success: true,
                    duration_ms: 42,
                    mutating: true,
                    sandbox: "none".to_string(),
                    sandbox_policy: "danger-full-access".to_string(),
                    output_preview: "ok".to_string(),
                },
            },
        };

        let actual = serde_json::to_value(payload).expect("serialize hook payload");
        let expected = json!({
            "session_id": session_id.to_string(),
            "cwd": "tmp",
            "triggered_at": "2025-01-01T00:00:00Z",
            "hook_event": {
                "event_type": "after_tool_use",
                "turn_id": "turn-2",
                "call_id": "call-1",
                "tool_name": "local_shell",
                "tool_kind": "local_shell",
                "tool_input": {
                    "input_type": "local_shell",
                    "params": {
                        "command": ["cargo", "fmt"],
                        "workdir": "codex-rs",
                        "timeout_ms": 60000,
                        "sandbox_permissions": "use_default",
                        "justification": null,
                        "prefix_rule": null,
                    },
                },
                "executed": true,
                "success": true,
                "duration_ms": 42,
                "mutating": true,
                "sandbox": "none",
                "sandbox_policy": "danger-full-access",
                "output_preview": "ok",
            },
        });

        assert_eq!(actual, expected);
    }

    #[test]
    fn plan_finalized_payload_serializes_stable_wire_shape() {
        let session_id = ThreadId::new();
        let thread_id =
            ThreadId::from_string("b5f6c1c2-1111-2222-3333-444455556666").expect("valid thread id");
        let payload = HookPayload {
            session_id,
            cwd: PathBuf::from("tmp"),
            client: Some("codex-tui".to_string()),
            triggered_at: Utc
                .with_ymd_and_hms(2025, 1, 1, 0, 0, 0)
                .single()
                .expect("valid timestamp"),
            hook_event: HookEvent::PlanFinalized {
                event: HookEventPlanFinalized {
                    thread_id,
                    turn_id: "turn-123".to_string(),
                    plan_id: format!("{thread_id}:turn-123"),
                    plan_text: "1. Add hooks\n2. Wire completion".to_string(),
                    parent_thread_id: Some(
                        ThreadId::from_string("11111111-2222-3333-4444-555555555555")
                            .expect("valid thread id"),
                    ),
                    original_user_request: Some("Add plan lifecycle hooks".to_string()),
                    plan_summary: Some("Add hooks".to_string()),
                },
            },
        };

        let actual = serde_json::to_value(payload).expect("serialize hook payload");
        let expected = json!({
            "session_id": session_id.to_string(),
            "cwd": "tmp",
            "client": "codex-tui",
            "triggered_at": "2025-01-01T00:00:00Z",
            "hook_event": {
                "event_type": "plan_finalized",
                "thread_id": thread_id.to_string(),
                "turn_id": "turn-123",
                "plan_id": format!("{thread_id}:turn-123"),
                "plan_text": "1. Add hooks\n2. Wire completion",
                "parent_thread_id": "11111111-2222-3333-4444-555555555555",
                "original_user_request": "Add plan lifecycle hooks",
                "plan_summary": "Add hooks",
            },
        });

        assert_eq!(actual, expected);
    }

    #[test]
    fn plan_implementation_completed_payload_serializes_stable_wire_shape() {
        let session_id = ThreadId::new();
        let thread_id =
            ThreadId::from_string("b5f6c1c2-1111-2222-3333-444455556666").expect("valid thread id");
        let parent_thread_id =
            ThreadId::from_string("11111111-2222-3333-4444-555555555555").expect("valid thread id");
        let payload = HookPayload {
            session_id,
            cwd: PathBuf::from("tmp"),
            client: Some("codex-tui".to_string()),
            triggered_at: Utc
                .with_ymd_and_hms(2025, 1, 1, 0, 0, 0)
                .single()
                .expect("valid timestamp"),
            hook_event: HookEvent::PlanImplementationCompleted {
                event: HookEventPlanImplementationCompleted {
                    thread_id,
                    parent_thread_id,
                    turn_id: "turn-456".to_string(),
                    last_agent_message: Some("Implemented and verified.".to_string()),
                },
            },
        };

        let actual = serde_json::to_value(payload).expect("serialize hook payload");
        let expected = json!({
            "session_id": session_id.to_string(),
            "cwd": "tmp",
            "client": "codex-tui",
            "triggered_at": "2025-01-01T00:00:00Z",
            "hook_event": {
                "event_type": "plan_implementation_completed",
                "thread_id": thread_id.to_string(),
                "parent_thread_id": parent_thread_id.to_string(),
                "turn_id": "turn-456",
                "last_agent_message": "Implemented and verified.",
            },
        });

        assert_eq!(actual, expected);
    }
}
