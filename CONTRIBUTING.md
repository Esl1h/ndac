# Contributing

Bug reports, fixes and documentation improvements are welcome. Open an
[issue](https://github.com/Esl1h/ndac/issues) for bugs and ideas, and a pull
request for changes. Report vulnerabilities privately as described in
[SECURITY.md](SECURITY.md), never in a public issue.

## Before you start

- Small fixes can go straight to a pull request.
- Changes to the configuration format, command-line interface, exit codes,
  JSON output or security model start as an issue labeled `design`, open
  for comments for at least 7 days; see [GOVERNANCE.md](GOVERNANCE.md).
- Never use a real NextDNS API key in tests, fixtures, logs or issues. The
  test suite runs against a fake API (`crates/ndac-testkit`).

## Requirements for a pull request

- **Tests.** Every behavior change or bug fix comes with a test that fails
  without it. Snapshot tests use [insta](https://insta.rs); review changed
  snapshots before accepting them.
- **Checks.** `just lint` and `just test` pass locally. CI runs the same
  checks plus the minimum supported Rust version (see `rust-version` in
  `Cargo.toml`), CodeQL, cargo-deny and workflow linting; all required
  checks must pass.
- **Formatting.** `just fmt` formats Rust and TOML. Comments explain why,
  not what.
- **Documentation.** User-visible changes update the book under `docs/src`
  and, when they change the output of `--help`, the doc comments in
  `crates/ndac-cli/src/cli.rs`. JSON Schemas are regenerated with
  `just schema`.
- **Commits.** Messages follow [Conventional Commits](https://www.conventionalcommits.org/)
  (`fix(core): ...`, `feat(cli): ...`, `docs: ...`), because release-plz
  builds `CHANGELOG.md` from them. One logical change per commit; keep
  refactors apart from behavior changes.
- **Review.** Pull requests are squash-merged into `main` after review by a
  code owner (`.github/CODEOWNERS`).

## Development setup

You need the stable Rust toolchain (`rust-toolchain.toml`), plus
[just](https://just.systems), [cargo-nextest](https://nexte.st),
[taplo](https://taplo.tamasfe.dev), [typos](https://github.com/crate-ci/typos)
and [cargo-deny](https://embarkstudios.github.io/cargo-deny/). `just --list`
shows every task:

```sh
just fmt      # format Rust and TOML
just lint     # fmt check, clippy -D warnings, typos, cargo deny
just test     # all tests and doctests
just docs     # API docs and the mdBook
```

Features behind the `unstable-endpoints` build feature are described in
[Undocumented endpoints](https://esl1h.github.io/ndac/unstable.html).

## License

By contributing, you agree that your contributions are dual licensed under
the [MIT](LICENSE-MIT) and [Apache-2.0](LICENSE-APACHE) licenses, as
described in the README.
