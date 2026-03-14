# Contributing

Contributions to Tamagotchi are welcome. This document covers the guidelines for contributing to the project.

## Getting started

1. Fork the repository
2. Clone your fork and create a feature branch
3. Make your changes
4. Ensure all checks pass (see below)
5. Submit a pull request

## Development setup

See [Development](development.md) for build instructions, project structure, and testing.

## Before submitting

Run all checks:

```sh
cargo build
cargo test
cargo fmt --check
cargo clippy -- -D warnings
```

All four must pass. CI will run these same checks on your PR.

## Code style

- Follow standard Rust conventions and `rustfmt` formatting
- Keep functions focused and small
- Use `anyhow::Result` for error propagation in application code
- Use `thiserror` for library error types if adding a new public error enum
- Prefer `tracing` macros (`debug!`, `info!`, `warn!`) over `println!` for diagnostic output

## Commit messages

- Use the imperative mood ("Add feature" not "Added feature")
- Keep the first line under 72 characters
- Reference issues when applicable

## What to contribute

Good areas for contribution:

- **Tests**: Several crates have limited test coverage (see [Development](development.md))
- **Documentation**: Improvements to these docs or inline code documentation
- **Skills**: New built-in skills for popular CLI tools
- **Bug fixes**: Check the issue tracker
- **Platform support**: Windows sandboxing, additional runtime support

## Architecture decisions

If your change affects the architecture (new crates, new tool types, changes to the agent loop), please open an issue first to discuss the approach.

## License

By contributing, you agree that your contributions will be licensed under the same license as the project.
