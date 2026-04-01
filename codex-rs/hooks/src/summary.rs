use std::ffi::OsStr;

use codex_config::ConfigLayerStackOrdering;
use codex_protocol::protocol::HookEventName;

use crate::HooksConfig;
use crate::engine::ConfiguredHandler;
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
pub struct ConfiguredHookSummary {
    pub source: HookConfigurationSource,
    pub label: String,
    pub description: String,
    pub command_preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HookConfigurationSummary {
    pub hooks: Vec<ConfiguredHookSummary>,
    pub warnings: Vec<String>,
}

impl HookConfigurationSummary {
    pub fn total_hook_count(&self) -> usize {
        self.hooks.len()
    }
}

pub fn summarize_configured_hooks(config: &HooksConfig) -> HookConfigurationSummary {
    let discovered = discovery::discover_handlers(config.config_layer_stack.as_ref());
    let has_hooks_json = hooks_json_present(config);
    let mut warnings = discovered.warnings;

    if has_hooks_json && !config.feature_enabled {
        warnings.push(
            "hooks.json lifecycle hooks are configured, but the `codex_hooks` feature is disabled for this session."
                .to_string(),
        );
    }

    if has_hooks_json && cfg!(windows) {
        warnings.push(
            "Disabled `codex_hooks` for this session because `hooks.json` lifecycle hooks are not supported on Windows yet."
                .to_string(),
        );
    }

    let mut hooks = Vec::new();
    if config.feature_enabled && !cfg!(windows) {
        hooks.extend(summarize_discovered_handlers(&discovered.handlers));
    }
    if let Some(legacy_notify) = summarize_legacy_notify(config) {
        hooks.push(legacy_notify);
    }

    HookConfigurationSummary { hooks, warnings }
}

fn hooks_json_present(config: &HooksConfig) -> bool {
    config
        .config_layer_stack
        .as_ref()
        .map(|stack| {
            stack
                .get_layers(
                    ConfigLayerStackOrdering::LowestPrecedenceFirst,
                    /*include_disabled*/ false,
                )
                .into_iter()
                .filter_map(|layer| layer.config_folder())
                .any(|folder| {
                    folder
                        .join("hooks.json")
                        .map(|path| path.as_path().is_file())
                        .unwrap_or(false)
                })
        })
        .unwrap_or(false)
}

fn summarize_discovered_handlers(handlers: &[ConfiguredHandler]) -> Vec<ConfiguredHookSummary> {
    handlers
        .iter()
        .map(|handler| {
            let matcher_description = handler.matcher.as_deref().map(matcher_summary);
            let label = match matcher_description.as_deref() {
                Some(matcher) => format!("{} · {matcher}", event_label(handler.event_name)),
                None => event_label(handler.event_name).to_string(),
            };

            let command_preview = shell_preview(&handler.command);
            let mut description = format!(
                "Command hook from {}",
                handler.source_path.display()
            );
            if let Some(matcher) = matcher_description {
                description.push_str(&format!("; matcher: {matcher}"));
            }
            description.push_str(&format!("; timeout: {}s", handler.timeout_sec));
            if let Some(status_message) = handler.status_message.as_deref() {
                description.push_str(&format!("; status: {status_message}"));
            }

            ConfiguredHookSummary {
                source: HookConfigurationSource::HooksJson,
                label,
                description,
                command_preview,
            }
        })
        .collect()
}

fn summarize_legacy_notify(config: &HooksConfig) -> Option<ConfiguredHookSummary> {
    let argv = config.legacy_notify_argv.as_ref()?;
    let (program, args) = argv.split_first()?;
    if program.is_empty() {
        return None;
    }

    let mut description = "Legacy notify command after each completed turn".to_string();
    if !config.feature_enabled {
        description.push_str("; available even when `codex_hooks` is disabled");
    }

    Some(ConfiguredHookSummary {
        source: HookConfigurationSource::ConfigToml,
        label: "Notification".to_string(),
        description,
        command_preview: shell_preview(
            &std::iter::once(program.clone())
                .chain(args.iter().cloned())
                .collect::<Vec<_>>()
                .join(" "),
        ),
    })
}

fn event_label(event_name: HookEventName) -> &'static str {
    match event_name {
        HookEventName::PreToolUse => "PreToolUse",
        HookEventName::PostToolUse => "PostToolUse",
        HookEventName::SessionStart => "SessionStart",
        HookEventName::UserPromptSubmit => "UserPromptSubmit",
        HookEventName::Stop => "Stop",
    }
}

fn matcher_summary(matcher: &str) -> String {
    if matcher == "*" || matcher == "^.*$" {
        return "*".to_string();
    }

    matcher
        .strip_prefix('^')
        .and_then(|value| value.strip_suffix('$'))
        .unwrap_or(matcher)
        .to_string()
}

fn shell_preview(command: &str) -> String {
    const MAX_LEN: usize = 72;

    if command.len() <= MAX_LEN {
        return command.to_string();
    }

    let program = command
        .split_whitespace()
        .next()
        .and_then(|value| std::path::Path::new(value).file_name())
        .and_then(OsStr::to_str)
        .unwrap_or("command");
    format!("{program} ...")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use codex_config::ConfigLayerStack;
    use codex_utils_absolute_path::AbsolutePathBuf;
    use pretty_assertions::assert_eq;
    use tempfile::tempdir;
    use toml::Value as TomlValue;

    use super::HookConfigurationSource;
    use super::summarize_configured_hooks;
    use crate::HooksConfig;

    #[test]
    fn summarize_configured_hooks_lists_discovered_handlers_and_notify() {
        let temp = tempdir().expect("tempdir");
        fs::write(
            temp.path().join("hooks.json"),
            r#"{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Edit",
        "hooks": [{ "type": "command", "command": "python3 pre.py", "timeout": 5 }]
      }
    ],
    "UserPromptSubmit": [
      {
        "hooks": [{ "type": "command", "command": "python3 prompt.py" }]
      }
    ]
  }
}"#,
        )
        .expect("write hooks.json");

        let config_path = temp.path().join("config.toml");
        fs::write(&config_path, "").expect("write config.toml");

        let config_path =
            AbsolutePathBuf::from_absolute_path(config_path).expect("absolute config path");
        let stack = ConfigLayerStack::default()
            .with_user_config(&config_path, TomlValue::Table(Default::default()));
        let summary = summarize_configured_hooks(&HooksConfig {
            legacy_notify_argv: Some(vec!["notify-send".to_string(), "Codex".to_string()]),
            feature_enabled: true,
            config_layer_stack: Some(stack),
            shell_program: None,
            shell_args: Vec::new(),
        });

        assert_eq!(summary.total_hook_count(), 3);
        assert_eq!(summary.warnings, Vec::<String>::new());
        assert_eq!(
            summary.hooks[0].source,
            HookConfigurationSource::HooksJson
        );
        assert_eq!(summary.hooks[0].label, "PreToolUse · Edit");
        assert!(summary.hooks[0].description.contains("timeout: 5s"));
        assert_eq!(summary.hooks[1].label, "UserPromptSubmit");
        assert_eq!(summary.hooks[2].source, HookConfigurationSource::ConfigToml);
        assert_eq!(summary.hooks[2].label, "Notification");
    }

    #[test]
    fn summarize_configured_hooks_warns_when_hooks_are_present_but_disabled() {
        let temp = tempdir().expect("tempdir");
        fs::write(
            temp.path().join("hooks.json"),
            r#"{ "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "echo stop" }] }] } }"#,
        )
        .expect("write hooks.json");
        let config_path = temp.path().join("config.toml");
        fs::write(&config_path, "").expect("write config.toml");

        let config_path =
            AbsolutePathBuf::from_absolute_path(config_path).expect("absolute config path");
        let stack = ConfigLayerStack::default()
            .with_user_config(&config_path, TomlValue::Table(Default::default()));
        let summary = summarize_configured_hooks(&HooksConfig {
            legacy_notify_argv: None,
            feature_enabled: false,
            config_layer_stack: Some(stack),
            shell_program: None,
            shell_args: Vec::new(),
        });

        assert!(summary.hooks.is_empty());
        assert_eq!(summary.warnings.len(), 1);
        assert!(summary.warnings[0].contains("feature is disabled"));
    }
}
