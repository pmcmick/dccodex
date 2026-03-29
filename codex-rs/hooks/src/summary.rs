use codex_protocol::protocol::HookEventName;

use crate::HooksConfig;
use crate::engine::discovery;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookConfigurationSource {
    HooksJson,
    ConfigToml,
}

impl HookConfigurationSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::HooksJson => "hooks.json",
            Self::ConfigToml => "config.toml",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfiguredHookGroupSummary {
    pub source: HookConfigurationSource,
    pub label: &'static str,
    pub description: &'static str,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HookConfigurationSummary {
    pub groups: Vec<ConfiguredHookGroupSummary>,
    pub warnings: Vec<String>,
}

impl HookConfigurationSummary {
    pub fn total_hook_count(&self) -> usize {
        self.groups.iter().map(|group| group.count).sum()
    }
}

pub fn summarize_configured_hooks(config: &HooksConfig) -> HookConfigurationSummary {
    let mut warnings = Vec::new();

    let discovered = discovery::discover_handlers(config.config_layer_stack.as_ref());
    warnings.extend(discovered.warnings);

    let mut groups = summarize_hooks_json_groups(&discovered.handlers);
    groups.extend(summarize_config_toml_groups(config));

    let has_lifecycle_hooks = groups
        .iter()
        .any(|group| group.source == HookConfigurationSource::HooksJson);

    if has_lifecycle_hooks && !config.feature_enabled {
        warnings.push(
            "hooks.json lifecycle hooks are configured, but the `codex_hooks` feature is disabled for this session."
                .to_string(),
        );
    }

    if has_lifecycle_hooks && cfg!(windows) {
        warnings.push(
            "Disabled `codex_hooks` for this session because `hooks.json` lifecycle hooks are not supported on Windows yet."
                .to_string(),
        );
    }

    HookConfigurationSummary { groups, warnings }
}

fn summarize_hooks_json_groups(
    handlers: &[super::engine::ConfiguredHandler],
) -> Vec<ConfiguredHookGroupSummary> {
    [
        HookEventName::PreToolUse,
        HookEventName::PostToolUse,
        HookEventName::SessionStart,
        HookEventName::UserPromptSubmit,
        HookEventName::Stop,
    ]
    .into_iter()
    .filter_map(|event_name| {
        let count = handlers
            .iter()
            .filter(|handler| handler.event_name == event_name)
            .count();
        (count > 0).then(|| {
            let (label, description) = lifecycle_event_metadata(event_name);
            ConfiguredHookGroupSummary {
                source: HookConfigurationSource::HooksJson,
                label,
                description,
                count,
            }
        })
    })
    .collect()
}

fn summarize_config_toml_groups(config: &HooksConfig) -> Vec<ConfiguredHookGroupSummary> {
    [
        (
            "Notification",
            "When notifications are sent",
            count_commands(config.legacy_notify_argv.as_ref()),
        ),
        (
            "UserPromptSubmit",
            "When the user submits a prompt",
            count_commands(config.after_user_prompt_submit_argv.as_ref()),
        ),
        (
            "BeforeModelRequest",
            "Before a model request is issued",
            count_commands(config.before_model_request_argv.as_ref()),
        ),
        (
            "ModelResponseCreated",
            "When a response object is created",
            count_commands(config.after_model_response_created_argv.as_ref()),
        ),
        (
            "ModelResponseCompleted",
            "After a response completes",
            count_commands(config.after_model_response_completed_argv.as_ref()),
        ),
        (
            "PlanFinalized",
            "When a plan is finalized",
            count_commands(config.plan_finalized_argv.as_ref()),
        ),
        (
            "TurnStarted",
            "When a turn begins",
            count_commands(config.turn_started_argv.as_ref()),
        ),
        (
            "TurnCompleted",
            "When a turn completes successfully",
            count_commands(config.turn_completed_argv.as_ref()),
        ),
        (
            "PlanImplementationCompleted",
            "When a plan implementation completes",
            count_commands(config.plan_implementation_completed_argv.as_ref()),
        ),
        (
            "TurnAborted",
            "When a turn aborts",
            count_commands(config.turn_aborted_argv.as_ref()),
        ),
        (
            "SessionStart",
            "When a session is configured",
            count_commands(config.session_start_argv.as_ref()),
        ),
        (
            "SessionShutdown",
            "When the session shuts down",
            count_commands(config.session_shutdown_argv.as_ref()),
        ),
        (
            "Compaction",
            "At compaction lifecycle boundaries",
            count_commands(config.compaction_argv.as_ref()),
        ),
        (
            "AfterToolUse",
            "After a tool completes",
            count_commands(config.after_tool_use_argv.as_ref()),
        ),
        (
            "PreToolUse",
            "Before tool execution",
            count_commands(config.pre_tool_use_argv.as_ref()),
        ),
        (
            "ToolFailure",
            "When a tool fails",
            count_commands(config.tool_failure_argv.as_ref()),
        ),
        (
            "PostToolUseSuccess",
            "After a tool succeeds",
            count_commands(config.post_tool_use_success_argv.as_ref()),
        ),
    ]
    .into_iter()
    .filter_map(|(label, description, count)| {
        (count > 0).then_some(ConfiguredHookGroupSummary {
            source: HookConfigurationSource::ConfigToml,
            label,
            description,
            count,
        })
    })
    .collect()
}

fn count_commands(commands: Option<&Vec<Vec<String>>>) -> usize {
    commands
        .into_iter()
        .flatten()
        .filter(|argv| !argv.is_empty() && !argv[0].is_empty())
        .count()
}

fn lifecycle_event_metadata(event_name: HookEventName) -> (&'static str, &'static str) {
    match event_name {
        HookEventName::PreToolUse => ("PreToolUse", "Before tool execution"),
        HookEventName::PostToolUse => ("PostToolUse", "After tool execution"),
        HookEventName::SessionStart => ("SessionStart", "When the session starts"),
        HookEventName::UserPromptSubmit => ("UserPromptSubmit", "When the user submits a prompt"),
        HookEventName::Stop => ("Stop", "When Codex stops"),
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    use codex_protocol::protocol::HookEventName;

    use super::HookConfigurationSource;
    use super::summarize_config_toml_groups;
    use super::summarize_configured_hooks;
    use super::summarize_hooks_json_groups;
    use crate::HooksConfig;
    use crate::engine::ConfiguredHandler;

    #[test]
    fn summarize_hooks_json_groups_includes_supported_events_in_order() {
        assert_eq!(
            summarize_hooks_json_groups(&[
                ConfiguredHandler {
                    event_name: HookEventName::PostToolUse,
                    matcher: None,
                    command: "echo after".to_string(),
                    timeout_sec: 600,
                    status_message: None,
                    source_path: PathBuf::from("/tmp/hooks.json"),
                    display_order: 2,
                },
                ConfiguredHandler {
                    event_name: HookEventName::PreToolUse,
                    matcher: None,
                    command: "echo before-1".to_string(),
                    timeout_sec: 600,
                    status_message: None,
                    source_path: PathBuf::from("/tmp/hooks.json"),
                    display_order: 0,
                },
                ConfiguredHandler {
                    event_name: HookEventName::PreToolUse,
                    matcher: Some("*".to_string()),
                    command: "echo before-2".to_string(),
                    timeout_sec: 600,
                    status_message: None,
                    source_path: PathBuf::from("/tmp/hooks.json"),
                    display_order: 1,
                },
                ConfiguredHandler {
                    event_name: HookEventName::UserPromptSubmit,
                    matcher: None,
                    command: "echo prompt".to_string(),
                    timeout_sec: 600,
                    status_message: None,
                    source_path: PathBuf::from("/tmp/hooks.json"),
                    display_order: 3,
                },
            ]),
            vec![
                super::ConfiguredHookGroupSummary {
                    source: HookConfigurationSource::HooksJson,
                    label: "PreToolUse",
                    description: "Before tool execution",
                    count: 2,
                },
                super::ConfiguredHookGroupSummary {
                    source: HookConfigurationSource::HooksJson,
                    label: "PostToolUse",
                    description: "After tool execution",
                    count: 1,
                },
                super::ConfiguredHookGroupSummary {
                    source: HookConfigurationSource::HooksJson,
                    label: "UserPromptSubmit",
                    description: "When the user submits a prompt",
                    count: 1,
                },
            ]
        );
    }

    #[test]
    fn summarize_config_toml_groups_filters_out_empty_commands() {
        assert_eq!(
            summarize_config_toml_groups(&HooksConfig {
                legacy_notify_argv: Some(vec![
                    vec!["python3".to_string(), "notify.py".to_string()],
                    vec!["".to_string()],
                ]),
                pre_tool_use_argv: Some(vec![Vec::new()]),
                tool_failure_argv: Some(vec![vec![
                    "python3".to_string(),
                    "failure.py".to_string(),
                ]]),
                ..HooksConfig::default()
            }),
            vec![
                super::ConfiguredHookGroupSummary {
                    source: HookConfigurationSource::ConfigToml,
                    label: "Notification",
                    description: "When notifications are sent",
                    count: 1,
                },
                super::ConfiguredHookGroupSummary {
                    source: HookConfigurationSource::ConfigToml,
                    label: "ToolFailure",
                    description: "When a tool fails",
                    count: 1,
                },
            ]
        );
    }

    #[test]
    fn summarize_configured_hooks_counts_toml_hooks_without_lifecycle_config() {
        let summary = summarize_configured_hooks(&HooksConfig {
            legacy_notify_argv: Some(vec![vec!["python3".to_string(), "notify.py".to_string()]]),
            pre_tool_use_argv: Some(vec![vec!["python3".to_string(), "pre.py".to_string()]]),
            ..HooksConfig::default()
        });

        assert_eq!(summary.total_hook_count(), 2);
        assert_eq!(summary.warnings, Vec::<String>::new());
    }
}
