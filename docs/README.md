# Zen documentation

This directory contains the user and developer documentation for Zen: a calm, repo-native Markdown workspace for humans and AI agents.

The docs intentionally cover only the retained product surface:

- Markdown reading, editing, and preview.
- Agent instruction files and repo context documents.
- File and project search.
- Code reading and editing.
- Git and GitHub visualization.
- Development and packaging notes for this fork.

The old upstream Zed docs covered AI assistants, collaboration, remote development, extensions, terminal workflows, debugger workflows, accounts, telemetry, and language-server customization. Those areas are no longer part of Zen's product direction and should not be reintroduced into the docs unless the product scope changes.

## Build locally

Install mdBook if needed, then run:

```sh
mdbook serve docs
```

The docs use plain mdBook configuration. They do not depend on the former Zed docs preprocessor.
