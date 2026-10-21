# Known limitations

Items here are assumptions that could not be verified against an official
source. Each one is revisited when the related feature is implemented. The
reference is the [NextDNS API documentation](https://nextdns.github.io/api/),
which is labeled beta.

## NextDNS API key format

The custom gitleaks rule in `.gitleaks.toml` assumes keys are 40 lowercase
hexadecimal characters. The API documentation shows an example key in that
format, but does not specify it. If real keys differ, secret scanning still
relies on the default gitleaks rules and on the generic `api_key = ...`
patterns.

## Endpoints

- `GET /profiles` (list profiles) is not described in the documentation text,
  which only covers `POST /profiles` and `GET`/`PATCH`/`DELETE` on
  `/profiles/:profile`. It is widely used by other NextDNS tools and is
  assumed to return `{ id, name }` entries, paginated like other arrays.
- Nested arrays inside `GET /profiles/:profile` are assumed to be complete;
  only array child endpoints (for example `/denylist`) are paginated.
- `PUT` on an array endpoint is assumed to take the full JSON array as body,
  and `POST` a single item object.

## Responses

- The documentation describes response bodies for `200` and `400` only. Other
  statuses are handled by code: `401`/`403` are authentication errors, `404`
  is "not found", `429` and `5xx` are retried. Any `errors` array is reported
  when present.
- Rate limits are not documented. The client retries `429` and `5xx` with
  exponential backoff and full jitter, honoring `Retry-After`.
- List items with an `active` flag are assumed to be active when the flag is
  missing.

## Configuration

- NextDNS is assumed to ignore list order, so lists are compared and
  rendered sorted by ID.
- Domains are validated locally (lowercase punycode, at least two labels, no
  wildcards). NextDNS may accept or reject a slightly different set.
- Blocklist, native, service and category IDs are not checked against a
  catalog: the catalog endpoints are not documented.
- There are two JSON Schemas (root file and profile/template files) instead
  of one, because the two kinds of files have different shapes.

## Authentication

- The API documentation describes one key per account and no scopes; the
  threat model assumes a key grants full account access.
- On a machine without a keyring service (servers, CI), a `keyring:` source
  is treated as missing rather than failed, so the default chain keeps
  working there. A locked keyring is a failure.
- `env:` variables are not removed from the process environment after being
  read: doing so requires `unsafe` in Rust 2024, and `ndac` starts children
  only with a cleared environment.
- The `age:` source shells out to the `age` binary instead of using the
  `age` crate, so identities from plugins work the same way as with the
  command-line tool.
- The per-user configuration file under `XDG_CONFIG_HOME` is not
  implemented yet; `ndac.local.toml` covers machine-specific settings.

## Fake API

`ndac-testkit` follows the documented semantics. Where the documentation is
silent, it assumes the following, and tests depend on these assumptions:

- Successful `PATCH`, `PUT`, `DELETE` and item `POST` return `204` with no
  body.
- Adding an item that already exists is a user error (`200` with
  `errors[].code = "duplicate"`).
- `PATCH` with a key that does not exist in the target object is rejected
  with `400`.
- Domains in the denylist and allowlist must be lowercase ASCII (punycode)
  with at least two labels.
