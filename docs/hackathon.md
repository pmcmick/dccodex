# DCCodex Hackathon Guide

This guide is the short version for event deployments: what users need to get
running quickly, where to look when something fails, and which docs to read
next.

## User Quickstart

Install DCCodex:

```bash
npm install -g @pmcmick/dccodex
```

Then start it:

```bash
dccodex
```

If you are distributing binaries directly instead of npm packages, use the
release artifact that matches the target platform. For older Linux machines,
prefer the musl release artifact.

## First Things To Configure

1. Sign in or provide credentials:
   [Authentication](authentication.md)
2. Review the core config options:
   [Configuration](config.md)
3. Learn the basic workflow:
   [Getting Started](getting-started.md)

## Recommended Docs For Users

- [Getting Started](getting-started.md)
- [Prompts](prompts.md)
- [Slash Commands](slash_commands.md)
- [Configuration](config.md)
- [Hooks](hooks.md)

## Good Defaults For Event Machines

- Prefer the npm install path when you want the easiest user experience.
- Prefer the musl Linux build when you need portability across older Linux
  distributions.
- Keep a sample config ready for participants:
  [Example Config](example-config.md)
- If you use hooks, document exactly which hooks are enabled and why:
  [Hooks](hooks.md)

## Troubleshooting

- Install/build issues:
  [Install](install.md)
- Auth problems:
  [Authentication](authentication.md)
- Sandbox or execution issues:
  [Sandbox](sandbox.md)
- Non-interactive command behavior:
  [Exec](exec.md)

## For Organizers

If you are preparing a local docs bundle:

```bash
mkdocs build
```

If you want a live local site for users on the same machine:

```bash
mkdocs serve --dev-addr 0.0.0.0:8000
```

If you are packaging DCCodex from local release artifacts, see the npm staging
steps in [Install](install.md).
