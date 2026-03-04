use anyhow::Context;
use anyhow::Result;
use codex_core::config::Config;
use codex_core::config::HookCommandConfigEntry;
use codex_utils_cli::CliConfigOverrides;

#[derive(Debug, clap::Parser)]
pub struct HooksCli {
    #[clap(flatten)]
    pub config_overrides: CliConfigOverrides,

    /// Output configured hooks as JSON.
    #[arg(long)]
    pub json: bool,
}

impl HooksCli {
    pub async fn run(self) -> Result<()> {
        let HooksCli {
            config_overrides,
            json,
        } = self;
        let overrides = config_overrides
            .parse_overrides()
            .map_err(anyhow::Error::msg)?;
        let config = Config::load_with_cli_overrides(overrides)
            .await
            .context("failed to load configuration")?;
        let entries = config.configured_hook_command_entries();

        if json {
            let json_entries: Vec<_> = entries
                .iter()
                .map(|entry| {
                    serde_json::json!({
                        "config_key": entry.config_key,
                        "event_type": entry.event_type,
                        "commands": entry.commands,
                    })
                })
                .collect();
            let output = serde_json::to_string_pretty(&json_entries)?;
            println!("{output}");
            return Ok(());
        }

        print_table(&entries);
        Ok(())
    }
}

fn print_table(entries: &[HookCommandConfigEntry<'_>]) {
    if entries.is_empty() {
        println!("No hooks configured. Add `notify`/`notify_on_*` entries in config.toml.");
        return;
    }

    let mut rows: Vec<[String; 3]> = Vec::new();
    for entry in entries {
        for command in entry.commands {
            rows.push([
                entry.event_type.to_string(),
                entry.config_key.to_string(),
                command.join(" "),
            ]);
        }
    }

    let mut widths = ["Event".len(), "Config Key".len(), "Command".len()];
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.len());
        }
    }

    println!(
        "{event:<event_w$}  {config_key:<key_w$}  {command:<command_w$}",
        event = "Event",
        config_key = "Config Key",
        command = "Command",
        event_w = widths[0],
        key_w = widths[1],
        command_w = widths[2],
    );
    for row in rows {
        println!(
            "{event:<event_w$}  {config_key:<key_w$}  {command:<command_w$}",
            event = row[0],
            config_key = row[1],
            command = row[2],
            event_w = widths[0],
            key_w = widths[1],
            command_w = widths[2],
        );
    }
}
