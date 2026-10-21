# Configuration

## Files

- `ndac.toml` is the root file. `ndac` uses `--config PATH`, then
  `NDAC_CONFIG`, then the nearest `ndac.toml` in the working directory or a
  parent directory.
- `include` lists globs, relative to the root directory, of template and
  profile files. Matches are sorted. Absolute patterns, `..` and files or
  symlinks resolving outside the root directory are rejected. Hidden
  directories and `target/` are skipped.
- Files under `templates/` are templates; every other included file is a
  profile. The file stem is the key: `profiles/casa.toml` is profile `casa`.
  Keys must be unique among templates and among profiles.
- `ndac.local.toml`, next to `ndac.toml`, is loaded automatically when it
  exists. It may only contain `[provider]`, `[auth]` and `[output]`, which
  override the root file. Keep it out of git; `ndac init` adds it to
  `.gitignore`.

Editors get completion and validation from the JSON Schemas in `schema/`
(`ndac.schema.json` for the root file, `ndac-profile.schema.json` for
profiles and templates). `ndac schema --kind root|local|profile` prints them.

## Profiles and templates

A profile or template file is the profile itself, using the API's camelCase
names:

```toml
id      = "abc123"            # empty or absent: created on apply
name    = "Casa"
extends = ["base", "paranoid"]
ignore  = ["parentalControl.services"]

[settings.logs]
retention = "90d"

[parentalControl]
safeSearch = true
categories = { gambling = true, porn = true }

[denylist]
mode = "authoritative"

[denylist.items]
"tiktokv.com" = true
"foo.example" = false   # present but inactive
```

Fields left out are not managed: `ndac` never changes them.

`id`, `name`, `ignore`, `auth` and list `mode` are profile-only; a template
that sets them is rejected.

## How `extends` merges

A profile resolves by applying layers, in order, onto an empty profile:

1. each template listed in `extends`, where a template's own `extends` is
   applied just before it (recursively);
2. the profile itself.

Each layer merges onto the result of the layers before it:

- **Tables** merge recursively.
- **Scalars**: the last layer that sets a value wins.
- **ID arrays** (`security.tlds`, `privacy.blocklists`, `privacy.natives`)
  replace the inherited array. An entry `"$inherit"` expands to the
  inherited array at that position. Duplicates keep their first occurrence.
- **Item maps** (`denylist.items`, `allowlist.items`,
  `parentalControl.services`, `parentalControl.categories`) merge by key. The
  value `"$remove"` deletes an inherited key.
- **`id`, `name`, `ignore`, `auth` and list `mode`** are never inherited.

Because templates are layers, a template can use `"$inherit"` or
`"$remove"` against whatever is stacked before it. With
`extends = ["base", "paranoid"]` and `blocklists = ["$inherit",
"hagezi-pro"]` in `paranoid`, the result is base's blocklists plus
`hagezi-pro`. A profile without `extends` cannot use the markers.

Cycles (`a` extends `b` extends `a`) and unknown templates are errors.

## Normalization

`ndac render <profile>` prints the canonical desired state that `plan`
compares with the live profile:

- durations become seconds (`"30d"` is `2592000`; units `s`, `m`, `h`, `d`,
  `w` and `y` = 365 days, combinable as in `"1h30m"`);
- domains become lowercase punycode without a trailing dot, so
  `Example.com.` and `example.com` are the same entry (and a duplicate if
  both are listed);
- ID arrays become `[{ id }]` and item maps `[{ id, active }]`, sorted by ID.
  List order is assumed not to matter to NextDNS.

## List modes

`denylist` and `allowlist` take a `mode`:

- `authoritative` (default): the configured items are the whole list; live
  items that are not configured are removed.
- `merge`: only the configured items are managed; other live items are left
  alone.

ID arrays and the parental control maps are always authoritative. Use
`ignore` to leave one of them unmanaged.

## Formatting

`ndac fmt` sorts ID arrays (with `"$inherit"` first) and item maps, aligns
`=` within each table, and ends files with one newline. It never changes
values or drops comments, and keeps file permissions. `ndac fmt --check`
lists files that need formatting and exits with 1.
