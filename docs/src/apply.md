# Apply, import and rollback

## Getting started

```sh
ndac init                     # ndac.toml, templates/base.toml, .taplo.toml, .gitignore
ndac auth set --backend keyring
ndac import --all             # one file per live profile under profiles/
ndac plan                     # empty right after an import
```

`ndac import` writes each profile in the canonical layout, with every
documented field and authoritative lists, so `plan` right after it shows
no changes. It never overwrites a file unless `--force` is given, and it
records the imported state as the drift baseline.

## Apply

`ndac apply` computes the plan, prints it, and asks for confirmation (type
`yes`). Without a terminal it refuses to run unless `--auto-approve` is
given.

For each profile with changes, it:

1. saves a `pre-apply` snapshot of the live profile;
2. sends the smallest set of calls, in this order: profile name, security,
   privacy, parental control, denylist, allowlist, settings. Field changes
   in one object become one `PATCH`; an authoritative list with more than
   one change becomes one `PUT` of the whole list; single item changes and
   merge-mode lists use per-item `POST`, `PATCH` and `DELETE`;
3. writes the new `id` into the profile file after creating a profile
   (comments and layout are kept);
4. reads the profile back and compares it with the configuration. A
   remaining difference is an error ("apply verification failed") and the
   state is saved as a `failed-apply` snapshot; otherwise it is saved as the
   `post-apply` snapshot that `plan` uses to detect drift.

Profiles are applied concurrently; calls within a profile are sequential.

### Safety

- **Destroy guard**: emptying an authoritative list, and deleting a profile
  whose file was removed, require `--allow-destroy`.
- **Lock**: `.ndac/snapshots/.lock` prevents two applies from the same
  checkout at once.
- **Ctrl-C**: the request in flight finishes, no new request starts, the
  report shows what was and was not applied, and the exit code is 130.
- **Retries**: only idempotent requests are retried blindly. Adding a list
  item re-reads the list before trying again.

## Rollback

```sh
ndac snapshot list --profile casa
ndac rollback 20261021T155535123Z-1a2b3c4d
```

`rollback` treats the snapshot as the complete desired state of its profile
(all lists authoritative), shows the plan, asks for confirmation and applies
it like `apply`. The configuration files are not changed, so the next
`plan` shows the configuration's changes as pending again.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success, or no changes |
| 1 | Error, cancelled confirmation, or verification failure |
| 2 | Changes pending (`plan --detailed-exitcode`, `drift`) |
| 3 | Authentication error |
| 4 | Configuration error |
| 5 | API error after retries |
| 130 | Interrupted by Ctrl-C |
