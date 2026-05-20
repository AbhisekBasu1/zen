<p align="center">
  <img src="crates/zen/resources/app-icon.png" alt="Zen app icon" width="112" height="112">
</p>

# Zen

Zen is a calm, repo-native Markdown workspace for humans and AI agents. It helps you read, edit, preview, search, and review the Markdown that runs a repository: README files, agent instructions, plans, specs, runbooks, changelogs, and project docs.

Zen began as a fork of Zed. The name is intentional: keep the speed and editor craft, remove the noise, and make the repository's context layer easier to understand.

The public product name, Cargo package, app bundle, URL scheme, and executable are now Zen. Zen is independently maintained and is not affiliated with Zed Industries.

## Benchmark snapshot

Zen keeps the core editing experience while cutting away a large amount of surrounding surface area. In local benchmarks, that translated to roughly 2.2x lower idle RSS, a 6.8x smaller debug binary, and a 3.9x faster cold Rust check compared with the original Zed baseline before the fork.

## Product thesis

Markdown is no longer just documentation. In AI-era software work, Markdown is also operational context:

- Humans use it to understand the project.
- AI agents use it to follow instructions, plans, constraints, and workflows.
- Reviewers use it to understand what changed and why.

Zen is built around that layer.

## Current scope

Included:

- Open files and folders.
- Read and edit Markdown.
- Preview Markdown documents.
- Find and maintain agent instruction files such as `AGENTS.md`, `CLAUDE.md`, and `.github/copilot-instructions.md`.
- Read and edit common code files with syntax highlighting.
- Search inside files and across a project.
- View Git changes and GitHub-related project context.

Out of scope:

- Built-in AI assistants.
- Multiplayer collaboration.
- Remote development.
- Extensions marketplace.
- Terminal workflows.
- Debugger workflows.
- Language-server management as a primary product feature.
- Accounts, telemetry, and cloud services.

## Development

Build a debug macOS app bundle:

```sh
./script/bundle-mac -d
```

The debug app is produced at:

```text
target/aarch64-apple-darwin/debug/bundle/osx/Zen.app
```

Run a fast compile check:

```sh
cargo check -p zen
```

Use `./script/clippy` instead of `cargo clippy` when running clippy.

## Documentation

The documentation lives in `docs/` and is intentionally small. It describes only the retained Markdown, agent-context, search, editor, and Git/GitHub workflows.

## Upstream attribution

Zen is a fork of Zed, substantially simplified and rebranded as a Markdown-first repository workspace. Portions of this repository remain derived from Zed and retain their original copyright and license notices.

See `NOTICE.md` for attribution details.

## License

Zen inherits the upstream Zed licensing structure:

- Editor/application code is licensed under `GPL-3.0-or-later`.
- Server-side components inherited from upstream, if present, are licensed under `AGPL-3.0-or-later`.
- GPUI and related permissively licensed components are licensed under `Apache-2.0`.

Review `LICENSE-GPL`, `LICENSE-AGPL`, `LICENSE-APACHE`, and `NOTICE.md` before distributing modified builds.
