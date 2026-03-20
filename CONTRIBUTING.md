# Contributing to Borg

## Development Setup

1. Install [Rust 1.75+](https://rustup.rs/)
2. Clone the repository:
   ```sh
   git clone https://github.com/borganization/borg.git
   cd borg
   ```
3. Build the project:
   ```sh
   cargo build
   ```
4. Run locally:
   ```sh
   cargo run --bin borg
   ```
5. Auto-rebuild on file changes (install `cargo-watch` first with `cargo install cargo-watch --locked`):
   ```sh
   cargo watch -x 'build --bin borg'
   ```
6. Set up git hooks:
   ```sh
   git config core.hooksPath .githooks
   ```

## Running Tests

```sh
cargo test                            # all tests
cargo test -p borg-apply-patch  # patch DSL tests
cargo test -p borg-core         # config tests
```

## Code Style

- Run `cargo fmt` before committing
- Run `cargo clippy` and fix any warnings
- The pre-commit hook checks both automatically

## Pull Requests

1. Fork the repo and create a feature branch
2. Make your changes with clear, focused commits
3. Ensure `cargo test` and `cargo clippy` pass
4. Open a PR with a description of what changed and why

## Commit Messages

Use conventional prefixes: `fix:`, `feat:`, `refactor:`, `chore:`, `docs:`, `test:`

## Questions?

Open an [issue](https://github.com/borganization/borg/issues) or start a [discussion](https://github.com/borganization/borg/discussions).
