# Governance

## Maintainer

The project is maintained by [@Esl1h](https://github.com/Esl1h), who has final
say on design, scope and releases. Additional maintainers may be added after a
sustained record of reviewed contributions; they are listed here and in
`.github/CODEOWNERS`.

## Decisions

- Small changes are decided in the pull request that proposes them.
- Changes to the configuration format, command-line interface, exit codes,
  JSON output format or security model start as an issue labeled `design` and
  are open for comments for at least 7 days before implementation.
- Security reports follow [SECURITY.md](SECURITY.md) and may be handled in
  private.

## Releases

Releases are cut from `main` by release-plz whenever there are user-facing
changes worth shipping, typically every few weeks. Security fixes are released
as soon as they are ready. Versions follow Semantic Versioning; before 1.0, a
minor version bump may contain breaking changes.
