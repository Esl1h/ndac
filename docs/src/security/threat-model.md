# Threat model

## Assets

- **The NextDNS API key.** It grants full access to the NextDNS account:
  every profile, its settings, and its query logs, which are a record of
  the browsing history of every device using those profiles. The API
  documentation describes a single key per account, shown on the account
  page, and no scopes or read-only keys. A leaked key must be treated as a
  full account compromise and rotated in the dashboard.
- **Snapshots** (`.ndac/snapshots/`): full profile JSON taken before each
  apply. They contain configuration, not logs, but reveal allow and deny
  lists.
- **Configuration**: the TOML files. Normally public-safe; they become
  sensitive only if they hold a literal key.

## Actors

- The local user running `ndac`.
- Other local users on the same machine.
- CI systems running `ndac plan`, `apply` or `drift`.
- Contributors and fork pull requests.
- The NextDNS API (trusted for correctness of its own data, not for
  shaping what `ndac` executes).

## Trust boundaries and mitigations

### Command line and process

| Threat | Mitigation |
|---|---|
| Key visible in `/proc/<pid>/cmdline`, `ps` and shell history | No `--api-key` flag; `--auth-source` accepts references only, and a value that looks like a pasted key is rejected without being echoed. |
| Key read from a core dump or by `ptrace` from a same-user process | `prctl(PR_SET_DUMPABLE, 0)` at startup on Linux. |
| Key in logs, errors, plan output or panic messages | The key type prints `****` plus at most the last four characters; it only reaches the `X-Api-Key` header, marked sensitive; error types never carry it; every resolved key is registered and masked by the log writer as a last line of defense. Release builds abort on panic. |
| Key forwarded to another host | The HTTP client refuses plain `http` (except loopback for tests) and never follows redirects. |
| `env:` variable inherited by child processes | `cmd:`, `age:` and `git` children run with a cleared environment plus a short allow list. The variable is not removed from `ndac`'s own environment: in Rust 2024 that requires `unsafe`, and `ndac` never spawns children that inherit its environment. |

### Key sources

| Threat | Mitigation |
|---|---|
| Literal key in a file readable by other users | Files holding a literal key (`ndac.local.toml`, `ndac.toml`, profile files, `.env`) must be mode `0600`-like and owned by the current user, or resolution fails with exit code 3. The override `allow_insecure_permissions` is only honored from `ndac.local.toml` or `NDAC_ALLOW_INSECURE_PERMISSIONS=1`, so a committed file cannot weaken it. |
| Literal key committed to git | A literal key in a file that git tracks, or that is in a work tree and not ignored, fails resolution. `--allow-tracked-secret` overrides it for one run, with a loud warning. `ndac init` and `ndac auth set --backend config` add `ndac.local.toml` and `.env` to `.gitignore`. gitleaks runs in pre-commit and CI. |
| Shell injection through `cmd:` sources | Commands are split with shell-words rules and executed directly, never through a shell. Shell syntax is passed as literal arguments. |
| A `cmd:` source hanging or prompting forever | `auth.cmd_timeout` (default 30s) kills the child. |
| `.env` parsing changing the process environment | `.env` files are parsed without being loaded into the environment. |
| A source failing silently and a different key being used | A source that exists but fails is an error by default; `on_source_error = "skip"` must be chosen explicitly, and never skips guardrail violations. |

### CI and repository

| Threat | Mitigation |
|---|---|
| Fork pull requests exfiltrating secrets | No `pull_request_target`; workflows triggered by pull requests get no secrets and read-only tokens. Live end-to-end tests run only from a protected environment with required reviewers. |
| Compromised third-party actions | Every action is pinned to a commit SHA, `persist-credentials: false`, minimal `permissions`, harden-runner egress auditing, and zizmor/actionlint in CI. |
| Malicious or vulnerable dependencies | `cargo deny` (RustSec advisories, license allow list, crates.io only, openssl banned), dependency review on pull requests, Dependabot with a cooldown, `Cargo.lock` committed and `--locked` builds. |
| Tampered release artifacts | Planned in the release phase: build provenance attestations, signed checksums, SBOMs. |

### The NextDNS API

| Threat | Mitigation |
|---|---|
| API returns unexpected fields | Unknown fields are preserved and ignored, never interpreted. |
| API reports errors with HTTP 200 | Any response with an `errors` array is a failure. |
| Partial apply after a failure | A snapshot is taken before every apply and the result is verified afterwards (apply phase). |

## Out of scope

- A compromised local user account: it can read the keyring session, the
  private files and the process memory of its own processes.
- The security of the NextDNS service itself.
- Endpoint configuration that `ndac endpoint render` prints and the user
  installs manually.
