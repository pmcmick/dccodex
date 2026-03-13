# Config JSON Schema

We generate a JSON Schema for `~/.codex/config.toml` from the `ConfigToml` type
and commit it at `codex-rs/core/config.schema.json` for editor integration.

When you change any fields included in `ConfigToml` (or nested config types),
regenerate the schema:

```
just write-config-schema
```

Plan lifecycle hook settings are part of this generated schema. The most
important plan-specific notifier keys are:

- `notify_on_plan_finalized` for the canonical finalized plan text
- `notify_on_plan_implementation_completed` for completion of the initial
  implementation task in a clean child thread derived from that plan

These keys are documented in `codex-rs/core/config.schema.json` via the
`ConfigToml` field comments in `core/src/config/mod.rs`.
