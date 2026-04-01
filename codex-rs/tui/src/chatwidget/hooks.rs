use codex_hooks::ConfiguredHookSummary;
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
            feature_enabled: self.config.features.enabled(codex_features::Feature::CodexHooks),
            config_layer_stack: Some(self.config.config_layer_stack.clone()),
            shell_program: None,
            shell_args: Vec::new(),
        });
        let total_hook_count = summary.total_hook_count();
        let has_warnings = !summary.warnings.is_empty();
        let item_count = summary.hooks.len();

        SelectionViewParams {
            view_id: Some(HOOKS_SELECTION_VIEW_ID),
            title: Some("Hooks".to_string()),
            subtitle: Some(match total_hook_count {
                0 => "No active hooks discovered".to_string(),
                1 => "1 hook discovered".to_string(),
                count => format!("{count} hooks discovered"),
            }),
            footer_note: Some(Line::from(
                if has_warnings {
                    "This menu is read-only. It shows the hooks discovered for this session. Some configured hooks may be inactive or skipped."
                } else {
                    "This menu is read-only. It shows the hooks discovered for this session."
                }
                .dim(),
            )),
            items: if summary.hooks.is_empty() {
                vec![SelectionItem {
                    name: "No active hooks discovered".to_string(),
                    description: summary
                        .warnings
                        .first()
                        .cloned()
                        .or_else(|| Some("Edit hooks.json or config.toml directly to add hooks.".to_string())),
                    is_disabled: true,
                    ..SelectionItem::default()
                }]
            } else {
                summary
                    .hooks
                    .into_iter()
                    .map(hooks_selection_item)
                    .collect()
            },
            initial_selected_idx: (item_count > 0).then_some(0),
            ..SelectionViewParams::default()
        }
    }
}

fn hooks_selection_item(hook: ConfiguredHookSummary) -> SelectionItem {
    SelectionItem {
        name: hook.label,
        name_prefix_spans: vec![format!("{} · ", hook.source.label()).dim().into()],
        description: Some(hook.command_preview.clone()),
        selected_description: Some(format!("{}\nCommand: {}", hook.description, hook.command_preview)),
        ..SelectionItem::default()
    }
}
