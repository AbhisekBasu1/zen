# Development

## Build the macOS app

Build a debug app bundle with:

```sh
./script/bundle-mac -d
```

The current debug bundle path is:

```text
target/aarch64-apple-darwin/debug/bundle/osx/Zen Dev.app
```

The Cargo package, executable, app bundle, URL scheme, and user data paths are named Zen. Some inherited source-tree paths still contain `zed`.

## Check Rust changes

Use:

```sh
cargo check -p zen
```

Use `./script/clippy` instead of `cargo clippy` when running clippy.

## Documentation changes

The docs are plain mdBook docs. They should stay small and should describe only supported product workflows.

Build locally with:

```sh
mdbook serve docs
```

Do not reintroduce the former Zed docs preprocessor unless there is a specific need for generated documentation.
