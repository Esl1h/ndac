# Developer tasks. Run `just --list` for a summary.
# Recipes for fuzz, e2e and release-dry are added in the phases that
# introduce those features.

set shell := ["bash", "-euo", "pipefail", "-c"]

default:
    @just --list

# Format Rust and TOML sources in place
fmt:
    cargo fmt --all
    taplo fmt

# Check formatting, lints, spelling and supply-chain policy
lint:
    cargo fmt --all --check
    taplo fmt --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    typos
    cargo deny check

# Run all tests, including doctests
test:
    cargo nextest run --workspace --all-features
    cargo test --doc --workspace --all-features

# Line coverage report in target/llvm-cov/html
cov:
    cargo llvm-cov nextest --workspace --all-features --html

# Build API docs and the mdBook
docs:
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace --all-features
    mdbook build docs

# Regenerate the JSON Schemas under schema/
schema:
    UPDATE_SCHEMA=1 cargo test -p ndac-core --lib schema_files_are_up_to_date

# Lint GitHub Actions workflows
lint-actions:
    actionlint
    zizmor --offline .github
