default:
    @just --list

# Format all code
fmt:
    cargo fmt --all

# Run clippy auto-fix on a specific crate
fix crate="":
    @if [ -z "{{crate}}" ]; then \
        cargo clippy --fix --all-features --tests --allow-dirty; \
    else \
        cargo clippy --fix --all-features --tests --allow-dirty -p {{crate}}; \
    fi

# Run all tests
test:
    cargo test --all

# Build debug
build:
    cargo build

# Build release
release:
    cargo build --release

# Run format check + clippy (what pre-commit does)
check:
    cargo fmt --check
    cargo clippy -- -D warnings

# Generate code coverage report (HTML)
coverage:
    cargo llvm-cov --workspace --html
    @echo "Report: target/llvm-cov/html/index.html"

# Generate code coverage summary (text)
coverage-summary:
    cargo llvm-cov --workspace

# Run the binary
run *args="":
    cargo run -- {{args}}

# Clean cargo target dirs (main + worktrees)
clean:
    cargo clean
    rm -rf .claude/worktrees/*/target

# Wipe all config/data so you can re-run onboarding from scratch
reset:
    rm -rf ~/.borg
