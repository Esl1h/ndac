# FAQ and troubleshooting

## FAQ

**Is there a state file?** No. The desired state is in the TOML files and
the live state comes from the API on every run. Snapshots under `.ndac/`
are only used to tell drift from pending changes and to roll back.

**Will `ndac` change settings I did not write down?** No. Fields that are
not set in configuration are not managed. Lists are different: in
`authoritative` mode (the default, and what `import` writes) live items
that are not configured are removed; use `mode = "merge"` to leave them
alone, or list the path in `ignore`.

**Can I start from the profiles I already have?** Yes: `ndac import --all`
writes one file per profile, and `ndac plan` right after it is empty.

**Can one repository manage profiles in several NextDNS accounts?** Yes. A
profile can set its own `[auth].sources`; see
[Authentication](authentication.md#resolution-order).

**Why is there no `--api-key` flag?** Command-line arguments are visible to
other users through the process list and stay in shell history. See
[Authentication](authentication.md#why-there-is-no---api-key).

**Does `ndac` configure my devices?** No. `ndac endpoint render` prints
configuration for the NextDNS CLI, systemd-resolved, NetworkManager and
Android, and never installs anything.

**Is this an official NextDNS tool?** No. `ndac` is an independent project
that uses the public NextDNS API.

## Troubleshooting

Exit codes say which part failed; see [Exit codes](apply.md#exit-codes).
`-v`, `-vv` or `NDAC_LOG=debug` add logging on stderr, and
`--log-format json` makes it machine-readable. Logs never contain the key.

### Authentication (exit code 3)

- `ndac auth sources` shows the source chain in use, where it came from,
  and the status of each source; `ndac auth check` shows which source
  answered. Neither prints the key.
- "readable by other users": a file with a literal key must be private.
  Run `chmod 600` on it.
- "is tracked by git" or "is not ignored by git": move the key out of the
  repository, or add the file to `.gitignore`. `ndac init` already ignores
  `ndac.local.toml` and `.env`.
- "keyring is locked or access was denied": unlock the keyring, or use
  another source on machines without a desktop session (servers, CI).

### Configuration (exit code 4)

`ndac validate` lists every problem with file, line and a hint. Unknown
keys are usually typos; the error suggests the closest valid name. The
JSON Schemas in `schema/` give the same checks in editors (`ndac init`
writes a `.taplo.toml` for them).

### API errors (exit code 5)

The client retries rate limits and server errors with backoff
(`provider.retries`, default 5). `-v` logs each retry and its reason, `-vv`
every API call with its method, endpoint template, status and duration.

### A plan that never becomes empty

After `apply`, `ndac` reads the profile back and checks that the plan is
empty; a field that NextDNS changes or rejects silently fails that check.
Add the path to `ignore` and open an issue with `ndac plan --format json`
output.

### `ndac doctor` says queries do not reach NextDNS

This device does not use NextDNS for DNS, or uses it without a profile.
Use `ndac endpoint render` for the device, then run `ndac doctor` again;
see [Endpoints and doctor](endpoint.md).
