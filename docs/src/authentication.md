# Authentication

`ndac` needs a NextDNS API key, found at the bottom of the
[account page](https://my.nextdns.io/account). The key gives access to the
whole account, including DNS logs, so `ndac` never takes it as a
command-line argument and never prints it.

## Sources

`[auth].sources` lists where to look, in order; the first source that
yields a key wins.

| Reference | Reads | Notes |
|---|---|---|
| `systemd-cred:NAME` | `$CREDENTIALS_DIRECTORY/NAME` | Best for services: `LoadCredentialEncrypted=` keeps the key encrypted at rest. |
| `keyring:SERVICE/USER` | OS keyring | Secret Service (GNOME Keyring, KWallet) or macOS Keychain. Store with `ndac auth set --backend keyring`. |
| `cmd:PROGRAM ARGS` | first line of the program's output | For password managers: `cmd:pass show nextdns/api-key`, `cmd:gopass show -o nextdns`. Runs without a shell, with a cleared environment, inherited stdin (for pinentry or a security-key touch), and a timeout. |
| `age:PATH` | file decrypted with `age` | Build feature `age`. Uses the `age` binary with `NDAC_AGE_IDENTITY`, so plugins such as `age-plugin-yubikey` work. |
| `env-file:VAR` | file named by `$VAR` | Docker and Kubernetes secrets convention. |
| `file:PATH` | a file | Trailing newline trimmed. |
| `env:VAR` | an environment variable | Convenient in CI; visible to anything that can read the environment. |
| `dotenv[:PATH]` | `NEXTDNS_API_KEY` in a `.env` file | Default search: `./.env`, then next to `ndac.toml`. Guardrails apply. |
| `config` | `api_key` in `[auth]` | `ndac.local.toml` first, then `ndac.toml`, then the profile file, then the user file. Guardrails apply. |

The table is roughly ordered from most to least protected at rest.

### Resolution order

1. `--auth-source REF` (repeatable or comma-separated), or
   `NDAC_AUTH_SOURCE`.
2. The profile's `[auth].sources`, for commands that act on that profile
   (for example a profile in a separate NextDNS account).
3. `[auth].sources` from `ndac.local.toml`, then `ndac.toml`.
4. `[auth].sources` from the user file (see
   [Configuration](configuration.md#files)).
5. The default chain: `systemd-cred:nextdns-api-key`,
   `env-file:NEXTDNS_API_KEY_FILE`, `env:NEXTDNS_API_KEY`, `dotenv`,
   `keyring:nextdns/default`, `config`.

A source that is not present (unset variable, absent file, no keyring
entry, no keyring service on the machine) is skipped. A source that exists
but fails (unreadable file, command exiting non-zero, locked keyring) stops
resolution with exit code 3, unless `on_source_error = "skip"`.

### Literal keys

A literal key in `ndac.local.toml`, `ndac.toml`, a profile file, the user
file or `.env` is allowed, with guardrails:

- the file must be readable only by its owner, who must be the current
  user. Fix with `chmod 600`. `allow_insecure_permissions = true` in
  `ndac.local.toml` or the user file (or `NDAC_ALLOW_INSECURE_PERMISSIONS=1`)
  turns the error into a warning;
- the file must not be tracked by git, and inside a work tree it must be
  ignored. `--allow-tracked-secret` overrides this for one run;
- a warning names the file on every run; `quiet_literal = true` silences
  it.

## Commands

- `ndac auth check [--profile P]` resolves the key, then lists the profiles
  it can reach. It prints the source, never the key.
- `ndac auth sources [--profile P]` shows the chain, where it came from, and
  the status of each source.
- `ndac auth set --backend keyring|config|file` stores a key typed at a
  hidden prompt, or read from stdin when it is not a terminal. `config`
  writes `ndac.local.toml` with mode `0600` and adds it to `.gitignore`.

## Why there is no `--api-key`

Command-line arguments of every process are readable by other users through
`/proc/<pid>/cmdline` and `ps`, and are saved in shell history. Environment
variables and files with private permissions do not have these problems.
