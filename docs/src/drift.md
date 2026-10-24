# Drift and reconcile

Drift is a change made outside `ndac`, for example in the NextDNS dashboard,
since the last apply. It is found by comparing each difference with the
`post-apply` snapshot (see [Plan and drift](plan.md#drift)). Profiles
without a snapshot, profiles to be created and removed profiles have no
drift.

## `ndac drift`

```sh
ndac drift                          # text, like `plan` but drift only
ndac drift --format json            # the plan JSON format, drift only
ndac drift --format github          # workflow annotations
ndac drift --format prom --textfile-dir /var/lib/node_exporter/textfile
```

`drift` exits with `0` when there is no drift and `2` when there is, so a
scheduled job can open an issue or alert on exit code 2. Changes made only in
the configuration (pending) are not drift and are not reported.

`--format github` prints one `::warning` per drifted section, pointing at
the file and line that set it: the profile, or the last template in
`extends` that sets it. Paths are relative to the working directory, so run
it from the repository root.

`--format prom` prints, or with `--textfile-dir` writes atomically to
`ndac_drift.prom` for the `node_exporter` textfile collector:

```text
# HELP ndac_drift Section changed outside ndac since the last apply (1).
# TYPE ndac_drift gauge
ndac_drift{profile="casa",section="denylist"} 1
# HELP ndac_drift_last_run_timestamp_seconds When the drift check last completed.
# TYPE ndac_drift_last_run_timestamp_seconds gauge
ndac_drift_last_run_timestamp_seconds 1792600000
```

Only drifted sections get a series. Alert on `ndac_drift == 1`, and on an
old `ndac_drift_last_run_timestamp_seconds` to catch a check that stopped
running.

## `ndac reconcile`

`reconcile` shows the drift and asks, item by item:

```text
profile "casa": denylist: bar.example is in NextDNS, not in the configuration [drift]
[o]verwrite live  [i]mport into code  [s]kip  [O]/[I] all remaining:
```

- **overwrite live** applies the configuration's value to NextDNS, with the
  same snapshot and verification steps as `apply`;
- **import into code** writes the live value into the configuration;
- **skip** leaves both sides as they are (an empty answer also skips).

`O` and `I` apply the same answer to every remaining item.

For automation, `--non-interactive --prefer live` imports everything and
`--non-interactive --prefer code` overwrites everything. Without a terminal,
`--prefer` is required. `--target` limits reconcile like `plan`.

### Writing into the configuration

Imports edit one key, array element or item at a time with `toml_edit`, so
comments, key order and whitespace are kept. A file that `ndac fmt` had
already formatted is formatted again after the edit, so `ndac fmt --check`
keeps passing; other files keep their layout. Each edited line gets a trailing
comment such as `# imported 2026-10-21 (drift)`, unless the line already has
a comment or `--no-comment` is given. Comments cannot go inside inline
tables, so values there are written without one.

When the value comes from a template, interactive runs ask where to write
it: into the profile, as an override (the default), or into the template,
which also changes every profile listed as using it. Non-interactive runs
always write into the profile.

How each kind of change is written into the profile:

- a field: the live value replaces the configured one (durations are written
  as strings such as `"30d"`);
- an item added in NextDNS: added to the item map, or to the ID array
  (as `["$inherit", "id"]` when the array is otherwise inherited);
- an item removed in NextDNS: removed from the profile; if a template still
  provides it, item maps get `"id" = "$remove"` and ID arrays are written out
  in full without `$inherit`;
- an item's `active` flag: the live flag is written.

After writing, the configuration is loaded again; if it no longer loads,
the files are restored and the error is shown. Right after
`reconcile --non-interactive --prefer live`, `plan` shows no changes.
