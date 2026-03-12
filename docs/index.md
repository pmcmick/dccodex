# DCCodex Docs

These docs are intended to be served locally for DCCodex users, especially in
short-lived environments like hackathons where you want fast answers without
depending on upstream web documentation.

## Start Here

- New user: [Getting Started](getting-started.md)
- Installing DCCodex: [Install](install.md)
- Authentication and sign-in: [Authentication](authentication.md)
- Configuration reference: [Configuration](config.md)
- Hook setup and examples: [Hooks](hooks.md)
- Interactive commands: [Slash Commands](slash_commands.md)
- Prompting guidance: [Prompts](prompts.md)

## Hackathon Use

If you are deploying DCCodex for an event or workshop, start with the
[Hackathon Guide](hackathon.md). It pulls together the specific install,
authentication, config, and troubleshooting links people need most.

## Serve Locally

If you have MkDocs installed:

```bash
mkdocs serve
```

Then open `http://127.0.0.1:8000/`.

If you only need a static build:

```bash
mkdocs build
```

That writes the generated site to `site/`.
