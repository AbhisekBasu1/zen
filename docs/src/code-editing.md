# Code editing

Zen keeps lightweight code reading and editing because repository Markdown often points directly at source code.

Supported expectations:

- Open and edit common source files.
- Syntax highlighting for popular languages where retained grammars are available.
- File navigation inside a project.
- Search across code and Markdown.
- Git diff context for code changes.

Non-goals:

- Full IDE behavior for every language.
- Extension-managed language tooling.
- Debugger workflows.
- Terminal-first development workflows.
- Cloud or remote development environments.

If a code feature does not directly support reading, editing, search, or Git/GitHub context, it should be treated as out of scope by default.
