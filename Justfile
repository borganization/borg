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

# Run the binary
run *args="":
    cargo run -- {{args}}
