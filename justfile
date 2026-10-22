# Developer tasks. Run `just --list` for a summary.
# Recipes for fuzz and e2e are added in the phase that introduces them.

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

# Build this host's release archive and SHA256SUMS into target/dist, as
# release.yml does, without publishing anything
release-dry:
    cargo build --release --locked -p ndac
    rm -rf target/assets target/dist
    packaging/scripts/assets.sh target/release/ndac target/assets
    packaging/scripts/archive.sh "$(cargo pkgid -p ndac | sed 's/.*[#@]//')" \
        "$(rustc -vV | sed -n 's/^host: //p')" target/release/ndac target/assets target/dist
    cd target/dist && sha256sum -- * > SHA256SUMS && cat SHA256SUMS

# Live test against a throwaway NextDNS profile; needs NEXTDNS_E2E_API_KEY
# and NEXTDNS_E2E_PROFILE_ID
e2e:
    cargo test -p ndac --features live-e2e --test live_e2e -- --ignored
