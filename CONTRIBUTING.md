# Contributing to nextform

Thanks for your interest. Please read the [Code of Conduct](CODE_OF_CONDUCT.md)
first. For anything larger than a small fix, open an issue to discuss the
change before writing code.

## Development setup

Required:

- Rust stable via [rustup](https://rustup.rs). The toolchain is pinned by
  `rust-toolchain.toml`; the minimum supported Rust version (MSRV) is the
  `rust-version` in `Cargo.toml`.
- [just](https://github.com/casey/just) to run the recipes below.

Used by the recipes and pre-commit hooks:

- [cargo-nextest](https://nexte.st), [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov),
  [cargo-deny](https://github.com/EmbarkStudios/cargo-deny)
- [taplo](https://taplo.tamasfe.dev), [typos](https://github.com/crate-ci/typos),
  [gitleaks](https://github.com/gitleaks/gitleaks)
- [mdBook](https://rust-lang.github.io/mdBook/)
- [actionlint](https://github.com/rhysd/actionlint) and
  [zizmor](https://docs.zizmor.sh) when editing workflows
- [pre-commit](https://pre-commit.com), then run `pre-commit install`

## Recipes

- `just fmt`: format Rust and TOML files.
- `just lint`: formatting check, clippy with warnings as errors, typos and
  `cargo deny check`.
- `just test`: all tests and doctests.
- `just cov`: HTML coverage report.
- `just docs`: rustdoc with warnings as errors, and the mdBook.
- `just lint-actions`: actionlint and zizmor on `.github/`.

`just lint test` must pass before you open a pull request.

## Formatting

`rustfmt.toml` only uses stable options. The nightly-only
`imports_granularity = "Crate"` and `group_imports = "StdExternalCrate"` are
the preferred import style; you can apply them occasionally with
`cargo +nightly fmt -- --config imports_granularity=Crate,group_imports=StdExternalCrate`,
but CI does not enforce them.

## Commits

- Follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/):
  `type(scope): subject`, imperative mood, subject at most 50 characters,
  body wrapped at 72 characters. The changelog is generated from these
  messages.
- One logical change per commit; keep refactors separate from behavior
  changes.
- Sign off every commit to certify the
  [Developer Certificate of Origin](https://developercertificate.org/):
  `git commit -s`.

## Tests

- New behavior needs tests; bug fixes need a test that fails without the fix.
- Tests never touch the network. Use the fake API in `nextform-testkit`.
- Snapshot tests use [insta](https://insta.rs); review changes with
  `cargo insta review` and commit the updated snapshots.
- If you change configuration types, regenerate the JSON Schema and commit it.

## Never commit real API keys

Use obviously fake keys such as `test-key-0000000000000000000000000000000000`
in tests, fixtures and examples. Never paste a real key, profile log or
unredacted plan into an issue or pull request. The pre-commit hooks and CI run
gitleaks; if a real key ever reaches a commit, revoke it in the NextDNS
dashboard immediately, then tell the maintainer privately.
