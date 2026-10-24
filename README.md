# ndac: NextDNS-as-Code

[![CI](https://github.com/Esl1h/ndac/actions/workflows/ci.yml/badge.svg)](https://github.com/Esl1h/ndac/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/ndac.svg)](https://crates.io/crates/ndac)
[![docs](https://img.shields.io/badge/docs-mdBook-blue.svg)](https://esl1h.github.io/ndac/)
[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/Esl1h/ndac/badge)](https://scorecard.dev/viewer/?uri=github.com/Esl1h/ndac)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![MSRV](https://img.shields.io/badge/rustc-1.89+-orange.svg)](Cargo.toml)

`ndac` keeps the desired state of your NextDNS profiles in TOML files,
reads the live state from the NextDNS API, and shows the difference as a
plan you can review in a pull request and apply, in the style of Terraform
and octoDNS but without a state file. Templates share settings between
profiles, every apply is verified and snapshotted first, and a scheduled
`ndac drift` tells you when someone changed a profile in the dashboard.

`ndac` is an independent project. It is not made, endorsed or supported by
NextDNS Inc.; "NextDNS" is used only to describe what the tool works with.

> **Status:** 0.1.0 is the first release. The configuration format and the
> crate APIs may still change before 1.0.

## Quickstart

```sh
ndac init my-nextdns && cd my-nextdns   # ndac.toml, templates/, .gitignore
ndac auth set --backend keyring         # API key from my.nextdns.io/account
ndac import --all                       # one TOML file per live profile
$EDITOR profiles/home.toml
ndac plan                               # review the changes
ndac apply                              # snapshot, apply, verify
```

Right after `import`, `ndac plan` is empty; after `apply`, it is empty
again. Both are checked by the test suite. See the
[examples](examples/) for complete configuration repositories.

## Features

| Feature | What it does |
| --- | --- |
| Plan and apply | Field-level diff with `+`, `-`, `~`; text, JSON and Markdown output. `apply` takes a snapshot first, then reads the profile back and checks the plan is empty. |
| Templates | `extends` stacks templates onto a profile; arrays keep inherited entries with `"$inherit"`, maps drop them with `"$remove"`. |
| List modes | `authoritative` lists remove live items that are not configured, `merge` lists leave them alone, and `ignore` skips paths. |
| Drift and reconcile | Changes made outside `ndac` are told apart from pending ones; `ndac reconcile` keeps the code or writes the live values back into the TOML files, comments preserved. |
| Import and rollback | Live profiles to TOML, and any snapshot back to the live profile. |
| Endpoints and doctor | Configuration for the NextDNS CLI, systemd-resolved, NetworkManager, Android and DoH URLs, and a check that this machine really uses the expected profile. |
| Automation | GitHub annotations and Prometheus metrics for drift, a GitHub Actions workflow (`ndac init --github-actions`), and a hardened systemd timer. |
| Editor support | JSON Schemas for completion and validation, and diagnostics with file, line and hints. |

## API key sources

The key can read and change the whole account, DNS logs included, so
`ndac` never takes it as a command-line argument and never prints it.
Sources, from most to least protected at rest:

| Source | Where the key lives |
| --- | --- |
| `systemd-cred:NAME` | systemd credentials, encrypted at rest |
| `keyring:SERVICE/USER` | the OS keyring (Secret Service, macOS Keychain) |
| `cmd:PROGRAM ARGS` | a password manager, such as `pass` or `gopass` |
| `age:PATH` | an `age`-encrypted file (build feature `age`) |
| `env-file:VAR`, `file:PATH` | a file, such as a container secret |
| `env:VAR` | an environment variable, convenient in CI |
| `dotenv`, `config` | a literal key in `.env` or `ndac.local.toml`, allowed only in files that are private and not tracked by git |

See [Authentication](https://esl1h.github.io/ndac/authentication.html).

## Installation

Release archives for Linux and macOS, `.deb` and `.rpm` packages, and the
container image `ghcr.io/esl1h/ndac` are listed in
[Installation](https://esl1h.github.io/ndac/installation.html). From
source: `cargo install --locked ndac`.

Every release file has build provenance, and `SHA256SUMS` is signed with
cosign:

```sh
gh attestation verify ndac-0.1.0-x86_64-unknown-linux-musl.tar.gz --repo Esl1h/ndac
cosign verify-blob SHA256SUMS --bundle SHA256SUMS.sigstore.json \
  --certificate-identity "https://github.com/Esl1h/ndac/.github/workflows/release.yml@refs/tags/v0.1.0" \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
sha256sum -c --ignore-missing SHA256SUMS
```

## Related tools

- The **NextDNS dashboard** is the reference for every setting and the
  quickest way to change one profile by hand. `ndac` uses the same API and
  works alongside it: dashboard changes show up as drift.
- **[terraform-provider-nextdns](https://github.com/amalucelli/terraform-provider-nextdns)**
  manages NextDNS from Terraform. It fits teams that already run Terraform
  and want NextDNS in the same state; `ndac` needs no state file and focuses
  on NextDNS alone.
- **[nextdnsctl](https://github.com/danielmeint/nextdnsctl)** (Python) is a
  command-line tool for bulk-editing, importing and backing up denylists
  and allowlists, and for managing profiles from a file. It suits quick
  scripted list changes; `ndac` adds plans, templates and drift detection
  for whole profiles.

## Documentation

The [book](https://esl1h.github.io/ndac/) covers configuration, merge
rules, plans, drift, CI recipes, endpoints and troubleshooting. `ndac
--help` and `man ndac` describe every command.

## Security

See [SECURITY.md](SECURITY.md) to report a vulnerability, and the
[threat model](https://esl1h.github.io/ndac/security/threat-model.html).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this project by you, as defined in the Apache-2.0 license,
shall be dual licensed as above, without any additional terms or conditions.
