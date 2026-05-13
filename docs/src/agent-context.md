# Agent context

AI coding agents increasingly depend on Markdown files for durable project context.

Zen's product focus is not to be another AI assistant. Its focus is to help humans inspect, edit, preview, search, and review the Markdown that agents rely on before they act.

## Important files

Common agent-facing files include:

- `AGENTS.md` for repository instructions used by agentic coding tools.
- `CLAUDE.md` for Claude Code project memory and instructions.
- `.github/copilot-instructions.md` for GitHub Copilot repository instructions.
- Planning and implementation notes that describe current work.
- Architecture docs and runbooks that explain project constraints.

## Why this matters

Changing these files can change how future agent sessions behave. A small wording change in an instruction file can affect edits, tests, pull request descriptions, and implementation choices.

That makes these files worth treating as first-class project artifacts.

## Zen's role

Zen should make this workflow calm and legible:

- Find the instruction files in a repository.
- Read them with Markdown preview available.
- Edit them as plain text.
- Search for related project rules and plans.
- Review changes through Git context.
- Keep code nearby when the document refers to implementation details.

Zen should not hide these files behind a chat interface. The source of truth is still Markdown in the repository.

## Human authorship

Zen can show which text was typed by a human in Zen after authorship tracking was enabled.

Use the human-authorship button in the editor toolbar to highlight human-authored ranges. Keyboard and IME input are tracked as human-authored. Paste, programmatic edits, formatter-style edits, and external file changes are treated as agent-authored unless the user manually marks a selection otherwise.

The editor context menu includes:

- `Paste as Agent`
- `Mark Selection as Human`
- `Mark Selection as Agent`

Authorship metadata is stored locally in Zen's data directory, not in the repository. It is saved after edits, on file save, and when an editor is closed. Existing text from before tracking was enabled cannot be classified retroactively.
