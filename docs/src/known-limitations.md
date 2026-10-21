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
