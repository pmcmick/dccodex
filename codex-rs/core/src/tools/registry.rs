use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use crate::client_common::tools::ToolSpec;
use crate::features::Feature;
use crate::function_tool::FunctionCallError;
use crate::memories::usage::emit_metric_for_tool_read;
use crate::protocol::SandboxPolicy;
use crate::sandbox_tags::sandbox_tag;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use async_trait::async_trait;
use codex_hooks::HookEvent;
use codex_hooks::HookEventAfterToolUse;
use codex_hooks::HookEventPostToolUseSuccess;
use codex_hooks::HookEventPreToolUse;
use codex_hooks::HookEventToolFailure;
use codex_hooks::HookPayload;
use codex_hooks::HookPreToolUseDecision;
use codex_hooks::HookResponse;
use codex_hooks::HookResult;
use codex_hooks::HookToolInput;
use codex_hooks::HookToolInputLocalShell;
use codex_hooks::HookToolKind;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::ResponseInputItem;
use codex_utils_readiness::Readiness;
use tracing::debug;
use tracing::warn;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToolKind {
    Function,
    Mcp,
}

#[async_trait]
pub trait ToolHandler: Send + Sync {
    fn kind(&self) -> ToolKind;

    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(
            (self.kind(), payload),
            (ToolKind::Function, ToolPayload::Function { .. })
                | (ToolKind::Mcp, ToolPayload::Mcp { .. })
        )
    }

    /// Returns `true` if the [ToolInvocation] *might* mutate the environment of the
    /// user (through file system, OS operations, ...).
    /// This function must remains defensive and return `true` if a doubt exist on the
    /// exact effect of a ToolInvocation.
    async fn is_mutating(&self, _invocation: &ToolInvocation) -> bool {
        false
    }

    /// Perform the actual [ToolInvocation] and returns a [ToolOutput] containing
    /// the final output to return to the model.
    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, FunctionCallError>;
}

pub struct ToolRegistry {
    handlers: HashMap<String, Arc<dyn ToolHandler>>,
}

impl ToolRegistry {
    pub fn new(handlers: HashMap<String, Arc<dyn ToolHandler>>) -> Self {
        Self { handlers }
    }

    pub fn handler(&self, name: &str) -> Option<Arc<dyn ToolHandler>> {
        self.handlers.get(name).map(Arc::clone)
    }

    // TODO(jif) for dynamic tools.
    // pub fn register(&mut self, name: impl Into<String>, handler: Arc<dyn ToolHandler>) {
    //     let name = name.into();
    //     if self.handlers.insert(name.clone(), handler).is_some() {
    //         warn!("overwriting handler for tool {name}");
    //     }
    // }

    pub async fn dispatch(
        &self,
        invocation: ToolInvocation,
    ) -> Result<ResponseInputItem, FunctionCallError> {
        let tool_name = invocation.tool_name.clone();
        let call_id_owned = invocation.call_id.clone();
        let otel = invocation.turn.otel_manager.clone();
        let payload_for_response = invocation.payload.clone();
        let log_payload = payload_for_response.log_payload();
        let metric_tags = [
            (
                "sandbox",
                sandbox_tag(
                    &invocation.turn.sandbox_policy,
                    invocation.turn.windows_sandbox_level,
                    invocation
                        .turn
                        .features
                        .enabled(Feature::UseLinuxSandboxBwrap),
                ),
            ),
            (
                "sandbox_policy",
                sandbox_policy_tag(&invocation.turn.sandbox_policy),
            ),
        ];
        let (mcp_server, mcp_server_origin) = match &invocation.payload {
            ToolPayload::Mcp { server, .. } => {
                let manager = invocation
                    .session
                    .services
                    .mcp_connection_manager
                    .read()
                    .await;
                let origin = manager.server_origin(server).map(str::to_owned);
                (Some(server.clone()), origin)
            }
            _ => (None, None),
        };
        let mcp_server_ref = mcp_server.as_deref();
        let mcp_server_origin_ref = mcp_server_origin.as_deref();

        {
            let mut active = invocation.session.active_turn.lock().await;
            if let Some(active_turn) = active.as_mut() {
                let mut turn_state = active_turn.turn_state.lock().await;
                turn_state.tool_calls = turn_state.tool_calls.saturating_add(1);
            }
        }

        let handler = match self.handler(tool_name.as_ref()) {
            Some(handler) => handler,
            None => {
                let message =
                    unsupported_tool_call_message(&invocation.payload, tool_name.as_ref());
                otel.tool_result_with_tags(
                    tool_name.as_ref(),
                    &call_id_owned,
                    log_payload.as_ref(),
                    Duration::ZERO,
                    false,
                    &message,
                    &metric_tags,
                    mcp_server_ref,
                    mcp_server_origin_ref,
                );
                return Err(FunctionCallError::RespondToModel(message));
            }
        };

        if !handler.matches_kind(&invocation.payload) {
            let message = format!("tool {tool_name} invoked with incompatible payload");
            otel.tool_result_with_tags(
                tool_name.as_ref(),
                &call_id_owned,
                log_payload.as_ref(),
                Duration::ZERO,
                false,
                &message,
                &metric_tags,
                mcp_server_ref,
                mcp_server_origin_ref,
            );
            return Err(FunctionCallError::Fatal(message));
        }

        let is_mutating = handler.is_mutating(&invocation).await;
        let hook_context = ToolHookContext {
            turn_id: invocation.turn.sub_id.clone(),
            call_id: invocation.call_id.clone(),
            tool_name: invocation.tool_name.clone(),
            tool_input: HookToolInput::from(&invocation.payload),
            mutating: is_mutating,
            sandbox: sandbox_tag(
                &invocation.turn.sandbox_policy,
                invocation.turn.windows_sandbox_level,
                invocation
                    .turn
                    .features
                    .enabled(Feature::UseLinuxSandboxBwrap),
            )
            .to_string(),
            sandbox_policy: sandbox_policy_tag(&invocation.turn.sandbox_policy).to_string(),
        };
        if let Some(short_circuit) =
            dispatch_pre_tool_use_hook(&invocation, &hook_context, &payload_for_response).await?
        {
            otel.tool_result_with_tags(
                tool_name.as_ref(),
                &call_id_owned,
                log_payload.as_ref(),
                Duration::ZERO,
                short_circuit.success,
                &short_circuit.output_preview,
                &metric_tags,
                mcp_server_ref,
                mcp_server_origin_ref,
            );
            let hook_abort_error = dispatch_tool_hook_event(
                &invocation,
                "after_tool_use",
                HookEvent::AfterToolUse {
                    event: HookEventAfterToolUse {
                        turn_id: hook_context.turn_id.clone(),
                        call_id: hook_context.call_id.clone(),
                        tool_name: hook_context.tool_name.clone(),
                        tool_kind: hook_context.tool_kind(),
                        tool_input: hook_context.tool_input.clone(),
                        executed: false,
                        success: short_circuit.success,
                        duration_ms: 0,
                        mutating: hook_context.mutating,
                        sandbox: hook_context.sandbox.clone(),
                        sandbox_policy: hook_context.sandbox_policy.clone(),
                        output_preview: short_circuit.output_preview.clone(),
                    },
                },
            )
            .await;
            if let Some(err) = hook_abort_error {
                return Err(err);
            }
            return Ok(short_circuit.response);
        }
        let output_cell = tokio::sync::Mutex::new(None);
        let invocation_for_tool = invocation.clone();

        let started = Instant::now();
        let result = otel
            .log_tool_result_with_tags(
                tool_name.as_ref(),
                &call_id_owned,
                log_payload.as_ref(),
                &metric_tags,
                mcp_server_ref,
                mcp_server_origin_ref,
                || {
                    let handler = handler.clone();
                    let output_cell = &output_cell;
                    async move {
                        if is_mutating {
                            tracing::trace!("waiting for tool gate");
                            invocation_for_tool.turn.tool_call_gate.wait_ready().await;
                            tracing::trace!("tool gate released");
                        }
                        match handler.handle(invocation_for_tool).await {
                            Ok(output) => {
                                let preview = output.log_preview();
                                let success = output.success_for_logging();
                                let mut guard = output_cell.lock().await;
                                *guard = Some(output);
                                Ok((preview, success))
                            }
                            Err(err) => Err(err),
                        }
                    }
                },
            )
            .await;
        let duration = started.elapsed();
        let (output_preview, success) = match &result {
            Ok((preview, success)) => (preview.clone(), *success),
            Err(err) => (err.to_string(), false),
        };
        emit_metric_for_tool_read(&invocation, success).await;
        let duration_ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
        let hook_abort_error = if success {
            dispatch_tool_hook_event(
                &invocation,
                "post_tool_use_success",
                HookEvent::PostToolUseSuccess {
                    event: HookEventPostToolUseSuccess {
                        turn_id: hook_context.turn_id.clone(),
                        call_id: hook_context.call_id.clone(),
                        tool_name: hook_context.tool_name.clone(),
                        tool_kind: hook_context.tool_kind(),
                        tool_input: hook_context.tool_input.clone(),
                        duration_ms,
                        mutating: hook_context.mutating,
                        sandbox: hook_context.sandbox.clone(),
                        sandbox_policy: hook_context.sandbox_policy.clone(),
                        output_preview: output_preview.clone(),
                    },
                },
            )
            .await
        } else {
            dispatch_tool_hook_event(
                &invocation,
                "tool_failure",
                HookEvent::ToolFailure {
                    event: HookEventToolFailure {
                        turn_id: hook_context.turn_id.clone(),
                        call_id: hook_context.call_id.clone(),
                        tool_name: hook_context.tool_name.clone(),
                        tool_kind: hook_context.tool_kind(),
                        tool_input: hook_context.tool_input.clone(),
                        duration_ms,
                        mutating: hook_context.mutating,
                        sandbox: hook_context.sandbox.clone(),
                        sandbox_policy: hook_context.sandbox_policy.clone(),
                        error_preview: output_preview.clone(),
                    },
                },
            )
            .await
        };
        if let Some(err) = hook_abort_error {
            return Err(err);
        }
        let hook_abort_error = dispatch_tool_hook_event(
            &invocation,
            "after_tool_use",
            HookEvent::AfterToolUse {
                event: HookEventAfterToolUse {
                    turn_id: hook_context.turn_id.clone(),
                    call_id: hook_context.call_id.clone(),
                    tool_name: hook_context.tool_name.clone(),
                    tool_kind: hook_context.tool_kind(),
                    tool_input: hook_context.tool_input.clone(),
                    executed: true,
                    success,
                    duration_ms,
                    mutating: hook_context.mutating,
                    sandbox: hook_context.sandbox.clone(),
                    sandbox_policy: hook_context.sandbox_policy.clone(),
                    output_preview,
                },
            },
        )
        .await;

        if let Some(err) = hook_abort_error {
            return Err(err);
        }

        match result {
            Ok(_) => {
                let mut guard = output_cell.lock().await;
                let output = guard.take().ok_or_else(|| {
                    FunctionCallError::Fatal("tool produced no output".to_string())
                })?;
                Ok(output.into_response(&call_id_owned, &payload_for_response))
            }
            Err(err) => Err(err),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConfiguredToolSpec {
    pub spec: ToolSpec,
    pub supports_parallel_tool_calls: bool,
}

impl ConfiguredToolSpec {
    pub fn new(spec: ToolSpec, supports_parallel_tool_calls: bool) -> Self {
        Self {
            spec,
            supports_parallel_tool_calls,
        }
    }
}

pub struct ToolRegistryBuilder {
    handlers: HashMap<String, Arc<dyn ToolHandler>>,
    specs: Vec<ConfiguredToolSpec>,
}

impl ToolRegistryBuilder {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            specs: Vec::new(),
        }
    }

    pub fn push_spec(&mut self, spec: ToolSpec) {
        self.push_spec_with_parallel_support(spec, false);
    }

    pub fn push_spec_with_parallel_support(
        &mut self,
        spec: ToolSpec,
        supports_parallel_tool_calls: bool,
    ) {
        self.specs
            .push(ConfiguredToolSpec::new(spec, supports_parallel_tool_calls));
    }

    pub fn register_handler(&mut self, name: impl Into<String>, handler: Arc<dyn ToolHandler>) {
        let name = name.into();
        if self
            .handlers
            .insert(name.clone(), handler.clone())
            .is_some()
        {
            warn!("overwriting handler for tool {name}");
        }
    }

    // TODO(jif) for dynamic tools.
    // pub fn register_many<I>(&mut self, names: I, handler: Arc<dyn ToolHandler>)
    // where
    //     I: IntoIterator,
    //     I::Item: Into<String>,
    // {
    //     for name in names {
    //         let name = name.into();
    //         if self
    //             .handlers
    //             .insert(name.clone(), handler.clone())
    //             .is_some()
    //         {
    //             warn!("overwriting handler for tool {name}");
    //         }
    //     }
    // }

    pub fn build(self) -> (Vec<ConfiguredToolSpec>, ToolRegistry) {
        let registry = ToolRegistry::new(self.handlers);
        (self.specs, registry)
    }
}

fn unsupported_tool_call_message(payload: &ToolPayload, tool_name: &str) -> String {
    match payload {
        ToolPayload::Custom { .. } => format!("unsupported custom tool call: {tool_name}"),
        _ => format!("unsupported call: {tool_name}"),
    }
}

fn sandbox_policy_tag(policy: &SandboxPolicy) -> &'static str {
    match policy {
        SandboxPolicy::ReadOnly { .. } => "read-only",
        SandboxPolicy::WorkspaceWrite { .. } => "workspace-write",
        SandboxPolicy::DangerFullAccess => "danger-full-access",
        SandboxPolicy::ExternalSandbox { .. } => "external-sandbox",
    }
}

// Hooks use a separate wire-facing input type so hook payload JSON stays stable
// and decoupled from core's internal tool runtime representation.
impl From<&ToolPayload> for HookToolInput {
    fn from(payload: &ToolPayload) -> Self {
        match payload {
            ToolPayload::Function { arguments } => HookToolInput::Function {
                arguments: arguments.clone(),
            },
            ToolPayload::Custom { input } => HookToolInput::Custom {
                input: input.clone(),
            },
            ToolPayload::LocalShell { params } => HookToolInput::LocalShell {
                params: HookToolInputLocalShell {
                    command: params.command.clone(),
                    workdir: params.workdir.clone(),
                    timeout_ms: params.timeout_ms,
                    sandbox_permissions: params.sandbox_permissions,
                    prefix_rule: params.prefix_rule.clone(),
                    justification: params.justification.clone(),
                },
            },
            ToolPayload::Mcp {
                server,
                tool,
                raw_arguments,
            } => HookToolInput::Mcp {
                server: server.clone(),
                tool: tool.clone(),
                arguments: raw_arguments.clone(),
            },
        }
    }
}

fn hook_tool_kind(tool_input: &HookToolInput) -> HookToolKind {
    match tool_input {
        HookToolInput::Function { .. } => HookToolKind::Function,
        HookToolInput::Custom { .. } => HookToolKind::Custom,
        HookToolInput::LocalShell { .. } => HookToolKind::LocalShell,
        HookToolInput::Mcp { .. } => HookToolKind::Mcp,
    }
}

struct ToolHookContext {
    turn_id: String,
    call_id: String,
    tool_name: String,
    tool_input: HookToolInput,
    mutating: bool,
    sandbox: String,
    sandbox_policy: String,
}

impl ToolHookContext {
    fn tool_kind(&self) -> HookToolKind {
        hook_tool_kind(&self.tool_input)
    }
}

struct PreToolHookShortCircuit {
    response: ResponseInputItem,
    success: bool,
    output_preview: String,
}

async fn dispatch_pre_tool_use_hook(
    invocation: &ToolInvocation,
    hook_context: &ToolHookContext,
    payload_for_response: &ToolPayload,
) -> Result<Option<PreToolHookShortCircuit>, FunctionCallError> {
    let hook_outcomes = dispatch_tool_hook_payload(
        invocation,
        "pre_tool_use",
        HookEvent::PreToolUse {
            event: HookEventPreToolUse {
                turn_id: hook_context.turn_id.clone(),
                call_id: hook_context.call_id.clone(),
                tool_name: hook_context.tool_name.clone(),
                tool_kind: hook_context.tool_kind(),
                tool_input: hook_context.tool_input.clone(),
                mutating: hook_context.mutating,
                sandbox: hook_context.sandbox.clone(),
                sandbox_policy: hook_context.sandbox_policy.clone(),
            },
        },
    )
    .await;
    let turn = invocation.turn.as_ref();

    for hook_outcome in hook_outcomes {
        let hook_name = hook_outcome.hook_name;
        match hook_outcome.result {
            HookResult::Success => {
                debug!(
                    turn_id = %turn.sub_id,
                    call_id = %invocation.call_id,
                    tool_name = %invocation.tool_name,
                    hook_event = "pre_tool_use",
                    hook_name = %hook_name,
                    "hook completed"
                );
            }
            HookResult::SuccessWithPreToolUseDecision(decision) => match decision {
                HookPreToolUseDecision::Deny { message } => {
                    warn!(
                        turn_id = %turn.sub_id,
                        call_id = %invocation.call_id,
                        tool_name = %invocation.tool_name,
                        hook_event = "pre_tool_use",
                        hook_name = %hook_name,
                        "hook denied tool execution"
                    );
                    let output = ToolOutput::Function {
                        body: FunctionCallOutputBody::Text(message),
                        success: Some(false),
                    };
                    let output_preview = output.log_preview();
                    let response =
                        output.into_response(invocation.call_id.as_str(), payload_for_response);
                    return Ok(Some(PreToolHookShortCircuit {
                        response,
                        success: false,
                        output_preview,
                    }));
                }
                HookPreToolUseDecision::Replace { output, success } => {
                    debug!(
                        turn_id = %turn.sub_id,
                        call_id = %invocation.call_id,
                        tool_name = %invocation.tool_name,
                        hook_event = "pre_tool_use",
                        hook_name = %hook_name,
                        replacement_chars = output.len(),
                        replacement_success = success,
                        "hook replaced tool execution"
                    );
                    let output = ToolOutput::Function {
                        body: FunctionCallOutputBody::Text(output),
                        success: Some(success),
                    };
                    let output_preview = output.log_preview();
                    let response =
                        output.into_response(invocation.call_id.as_str(), payload_for_response);
                    return Ok(Some(PreToolHookShortCircuit {
                        response,
                        success,
                        output_preview,
                    }));
                }
            },
            HookResult::SuccessWithAppendedUserPrompt(text) => {
                debug!(
                    turn_id = %turn.sub_id,
                    call_id = %invocation.call_id,
                    tool_name = %invocation.tool_name,
                    hook_event = "pre_tool_use",
                    hook_name = %hook_name,
                    appended_chars = text.len(),
                    "hook completed with appended prompt text"
                );
            }
            HookResult::SuccessWithPromptAugmentation {
                append_prompt_text,
                switch_to_plan_mode,
            } => {
                debug!(
                    turn_id = %turn.sub_id,
                    call_id = %invocation.call_id,
                    tool_name = %invocation.tool_name,
                    hook_event = "pre_tool_use",
                    hook_name = %hook_name,
                    switch_to_plan_mode,
                    appended_prompt_text_present = append_prompt_text.is_some(),
                    "hook completed with prompt augmentation"
                );
            }
            HookResult::FailedContinue(error) => {
                warn!(
                    call_id = %invocation.call_id,
                    tool_name = %invocation.tool_name,
                    hook_event = "pre_tool_use",
                    hook_name = %hook_name,
                    error = %error,
                    "pre_tool_use hook failed; continuing"
                );
            }
            HookResult::FailedAbort(error) => {
                warn!(
                    call_id = %invocation.call_id,
                    tool_name = %invocation.tool_name,
                    hook_event = "pre_tool_use",
                    hook_name = %hook_name,
                    error = %error,
                    "pre_tool_use hook failed; aborting operation"
                );
                return Err(FunctionCallError::Fatal(format!(
                    "pre_tool_use hook '{hook_name}' failed and aborted operation: {error}"
                )));
            }
        }
    }

    Ok(None)
}

async fn dispatch_tool_hook_payload(
    invocation: &ToolInvocation,
    hook_event_name: &'static str,
    hook_event: HookEvent,
) -> Vec<HookResponse> {
    let session = invocation.session.as_ref();
    let turn = invocation.turn.as_ref();
    let hook_outcomes = session
        .hooks()
        .dispatch(HookPayload {
            session_id: session.conversation_id,
            cwd: turn.cwd.clone(),
            client: turn.app_server_client_name.clone(),
            triggered_at: chrono::Utc::now(),
            hook_event,
        })
        .await;
    debug!(
        turn_id = %turn.sub_id,
        call_id = %invocation.call_id,
        tool_name = %invocation.tool_name,
        hook_event = hook_event_name,
        hooks_executed = hook_outcomes.len(),
        "hook dispatch completed"
    );
    hook_outcomes
}

async fn dispatch_tool_hook_event(
    invocation: &ToolInvocation,
    hook_event_name: &'static str,
    hook_event: HookEvent,
) -> Option<FunctionCallError> {
    let turn = invocation.turn.as_ref();
    let hook_outcomes = dispatch_tool_hook_payload(invocation, hook_event_name, hook_event).await;

    for hook_outcome in hook_outcomes {
        let hook_name = hook_outcome.hook_name;
        match hook_outcome.result {
            HookResult::Success => {
                debug!(
                    turn_id = %turn.sub_id,
                    call_id = %invocation.call_id,
                    tool_name = %invocation.tool_name,
                    hook_event = hook_event_name,
                    hook_name = %hook_name,
                    "hook completed"
                );
            }
            HookResult::SuccessWithAppendedUserPrompt(text) => {
                debug!(
                    turn_id = %turn.sub_id,
                    call_id = %invocation.call_id,
                    tool_name = %invocation.tool_name,
                    hook_event = hook_event_name,
                    hook_name = %hook_name,
                    appended_chars = text.len(),
                    "hook completed with appended prompt text"
                );
            }
            HookResult::SuccessWithPromptAugmentation {
                append_prompt_text,
                switch_to_plan_mode,
            } => {
                debug!(
                    turn_id = %turn.sub_id,
                    call_id = %invocation.call_id,
                    tool_name = %invocation.tool_name,
                    hook_event = hook_event_name,
                    hook_name = %hook_name,
                    switch_to_plan_mode,
                    appended_prompt_text_present = append_prompt_text.is_some(),
                    "hook completed with prompt augmentation"
                );
            }
            HookResult::SuccessWithPreToolUseDecision(decision) => {
                debug!(
                    turn_id = %turn.sub_id,
                    call_id = %invocation.call_id,
                    tool_name = %invocation.tool_name,
                    hook_event = hook_event_name,
                    hook_name = %hook_name,
                    ?decision,
                    "hook completed with pre-tool decision"
                );
            }
            HookResult::FailedContinue(error) => {
                warn!(
                    call_id = %invocation.call_id,
                    tool_name = %invocation.tool_name,
                    hook_event = hook_event_name,
                    hook_name = %hook_name,
                    error = %error,
                    "{hook_event_name} hook failed; continuing"
                );
            }
            HookResult::FailedAbort(error) => {
                warn!(
                    call_id = %invocation.call_id,
                    tool_name = %invocation.tool_name,
                    hook_event = hook_event_name,
                    hook_name = %hook_name,
                    error = %error,
                    "{hook_event_name} hook failed; aborting operation"
                );
                return Some(FunctionCallError::Fatal(format!(
                    "{hook_event_name} hook '{hook_name}' failed and aborted operation: {error}"
                )));
            }
        }
    }

    None
}
