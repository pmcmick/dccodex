use codex_features::Feature;
use codex_hooks::ConfiguredHookGroupSummary;
use codex_hooks::HooksConfig;
use codex_hooks::summarize_configured_hooks;
use ratatui::style::Stylize;
use ratatui::text::Line;

use super::ChatWidget;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;

const HOOKS_SELECTION_VIEW_ID: &str = "hooks-selection";

impl ChatWidget {
    pub(crate) fn add_hooks_output(&mut self) {
        self.bottom_pane
            .show_selection_view(self.hooks_popup_params());
        self.request_redraw();
    }

    fn hooks_popup_params(&self) -> SelectionViewParams {
        let summary = summarize_configured_hooks(&HooksConfig {
            legacy_notify_argv: self.config.notify.clone(),
            after_user_prompt_submit_argv: self.config.notify_on_user_prompt_submit.clone(),
            before_model_request_argv: self.config.notify_on_before_model_request.clone(),
            after_model_response_created_argv: self.config.notify_on_model_response_created.clone(),
            plan_finalized_argv: self.config.notify_on_plan_finalized.clone(),
            turn_started_argv: self.config.notify_on_turn_started.clone(),
            turn_completed_argv: self.config.notify_on_turn_completed.clone(),
            plan_implementation_completed_argv: self
                .config
                .notify_on_plan_implementation_completed
                .clone(),
            turn_aborted_argv: self.config.notify_on_turn_aborted.clone(),
            session_start_argv: self.config.notify_on_session_start.clone(),
            session_shutdown_argv: self.config.notify_on_session_shutdown.clone(),
            compaction_argv: self.config.notify_on_compaction.clone(),
            after_tool_use_argv: self.config.notify_on_after_tool_use.clone(),
            pre_tool_use_argv: self.config.notify_on_pre_tool_use.clone(),
            tool_failure_argv: self.config.notify_on_tool_failure.clone(),
            post_tool_use_success_argv: self.config.notify_on_post_tool_use_success.clone(),
            after_model_response_completed_argv: self
                .config
                .notify_on_model_response_completed
                .clone(),
            feature_enabled: self.config.features.enabled(Feature::CodexHooks),
            config_layer_stack: Some(self.config.config_layer_stack.clone()),
            shell_program: None,
            shell_args: Vec::new(),
        });
        let total_hook_count = summary.total_hook_count();
        let has_warnings = !summary.warnings.is_empty();
        let item_count = summary.groups.len();

        SelectionViewParams {
            view_id: Some(HOOKS_SELECTION_VIEW_ID),
            title: Some("Hooks".to_string()),
            subtitle: Some(match total_hook_count {
                0 => "No hooks configured".to_string(),
                1 => "1 hook configured".to_string(),
                count => format!("{count} hooks configured"),
            }),
            footer_note: Some(Line::from(
                if has_warnings {
                    "This menu is read-only. To add or modify hooks, edit hooks.json or config.toml directly. Some hooks may be inactive in this session."
                } else {
                    "This menu is read-only. To add or modify hooks, edit hooks.json or config.toml directly."
                }
                .dim(),
            )),
            items: if summary.groups.is_empty() {
                vec![SelectionItem {
                    name: "No hooks configured".to_string(),
                    description: Some(
                        "Edit hooks.json or config.toml directly to add hooks.".to_string(),
                    ),
                    is_disabled: true,
                    ..SelectionItem::default()
                }]
            } else {
                summary
                    .groups
                    .into_iter()
                    .map(hooks_selection_item)
                    .collect()
            },
            initial_selected_idx: (item_count > 0).then_some(0),
            ..SelectionViewParams::default()
        }
    }
}

fn hooks_selection_item(group: ConfiguredHookGroupSummary) -> SelectionItem {
    SelectionItem {
        name: format!("{} ({})", group.label, group.count),
        name_prefix_spans: vec![format!("{} · ", group.source.label()).dim()],
        description: Some(group.description.to_string()),
        selected_description: Some(format!(
            "{} Configured via {}.",
            group.description,
            group.source.label()
        )),
        ..SelectionItem::default()
    }
}
