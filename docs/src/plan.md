# Plan and drift

`ndac plan` compares the desired state of each profile with its live state
and prints what `ndac apply` would change. It never writes anything.

```text
profile "casa" (abc123)
  ~ privacy.blocklists
      + hagezi-pro
  ~ settings.logs.retention: 7776000 → 2592000 (90d → 30d)
  ! denylist                                     [drift]
      - foo.example   (live only; authoritative → will be removed)

profile "trabalho" (def456): no changes

Plan: 1 to add, 1 to change, 1 to remove across 1 profile. Drift in 1 section.
```

Symbols: `+` add, `-` remove, `~` change, `!` a section changed outside
`ndac`.

## What is compared

- Only fields set in configuration are managed; others are left alone.
- Paths in `ignore` are skipped on both sides.
- Lists are compared by ID. In `authoritative` lists, live items that are
  not configured are removed; in `merge` lists they are left alone.
- A profile without `id` is shown as `(to be created)`.

## Drift

After every verified apply, `ndac` stores a `post-apply` snapshot of the
live profile. `plan` compares each change against it:

- **pending**: the configuration changed since the last apply;
- **drift**: the live profile changed (for example in the dashboard);
- **conflict**: both changed the same field or item.

Without a snapshot every change is pending. `ndac drift` reports drift for
scheduled checks, and `ndac reconcile` resolves it; see
[Drift and reconcile](drift.md).

## Options

- `--target PATH` limits the plan to a profile (`casa`), a section
  (`casa.denylist`) or a section in every profile (`*.privacy.blocklists`).
  Repeatable.
- `--detailed-exitcode` exits with 2 when there are changes, 0 when none.
- `--format text|json|markdown`.

A profile whose file was removed but that has snapshots is reported; only
`ndac apply --allow-destroy` deletes it.

## JSON format

`--format json` prints a stable document. Fields are only added within a
`format_version`; any other change bumps it.

```json
{
  "format_version": 1,
  "summary": { "add": 1, "change": 1, "remove": 1, "profiles": 1, "drift_sections": 1 },
  "profiles": [
    {
      "key": "casa",
      "id": "abc123",
      "create": false,
      "list_modes": { "denylist": "authoritative" },
      "changes": [
        { "op": "add_item", "list": "privacy.blocklists", "id": "hagezi-pro", "active": null, "origin": "pending" },
        { "op": "set_field", "path": "settings.logs.retention", "from": 7776000, "to": 2592000, "origin": "pending" },
        { "op": "remove_item", "list": "denylist", "id": "foo.example", "origin": "drift" }
      ]
    }
  ],
  "removed": []
}
```

- `op` is one of `set_field`, `add_item`, `remove_item`, `set_item_active`.
- `origin` is `pending`, `drift` or `conflict`.
- `active` is `null` for lists without an active flag (TLDs, blocklists,
  natives).

## Markdown format

`--format markdown` is meant for pull request comments: a summary line and
one collapsible block per profile, truncated at 60000 characters.

## Snapshots

Snapshots live in `.ndac/snapshots/<profile>/` (configurable with
`provider.snapshot_dir`), one JSON file per snapshot, mode `0600`, and the
newest `provider.snapshot_keep` (default 50) are kept per profile.

- `ndac snapshot list [--profile P]`
- `ndac snapshot show <id>`: the full JSON. IDs can be shortened to a
  unique prefix.
- `ndac snapshot diff <a> <b>`: the changes that turn snapshot `a` into `b`.
