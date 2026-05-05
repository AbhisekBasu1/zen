# Contributing

Zen is a calm, repo-native Markdown workspace derived from Zed. Contributions should preserve that narrow scope.

## Product scope

Accepted work should support one of these areas:

- Reading and editing Markdown.
- Markdown preview.
- File and project search.
- Reading and editing code files.
- Git and GitHub visualization.
- Stability, packaging, performance, or documentation for the focused app.

Avoid reintroducing removed product areas unless they are explicitly approved:

- AI assistants.
- Collaboration.
- Remote development.
- Extension marketplace functionality.
- Terminal-first workflows.
- Debugger UI.
- Accounts, telemetry, or cloud services.

## Development guidelines

- Prefer simple, explicit changes over broad abstractions.
- Preserve the editor core where possible rather than rebuilding existing behavior.
- Avoid adding new crates or files unless there is a clear component boundary.
- Propagate errors instead of panicking or silently discarding failures.
- Do not use `unwrap()` in new Rust code unless there is a narrow, justified reason.
- Use full words for variable names.
- Use `./script/clippy` instead of `cargo clippy`.

## Before opening a pull request

Run the smallest checks that are relevant to your change. For most Rust changes, start with:

```sh
cargo check -p zen
```

For documentation-only changes, a code build is usually unnecessary.

## Pull request format

Use a clear, imperative title without conventional commit prefixes. Include release notes at the end of the pull request body:

```text
Release Notes:

- Fixed ...
```

Use `- N/A` for docs-only or internal-only changes.
