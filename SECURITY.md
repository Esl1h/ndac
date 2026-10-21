# Security policy

## Supported versions

Until 1.0, only the latest released minor version receives security fixes.
After 1.0, the latest minor version of the current major receives fixes, and
the previous major receives critical fixes for six months after a new major is
released.

## Reporting a vulnerability

Please report vulnerabilities privately through
[GitHub private vulnerability reporting](https://github.com/Esl1h/ndac/security/advisories/new).
Do not open a public issue, pull request or discussion.

Include the affected version, your platform, the steps to reproduce, and the
impact you expect. Never include a real NextDNS API key; redact keys, profile
IDs and log excerpts.

## Response targets

- Acknowledgement within 3 business days.
- Initial assessment within 7 days.
- A fix or mitigation for confirmed high or critical issues within 30 days,
  followed by a GitHub Security Advisory and, where applicable, a RustSec
  advisory.

This is a volunteer project, so these are targets and not guarantees.

## Scope

In scope:

- Handling of the NextDNS API key: resolution from every auth source, in-memory
  handling, redaction in logs, errors, plan output and snapshots.
- Execution of `cmd:` auth sources (argument parsing, environment, timeouts).
- File permissions and git-tracking guardrails for files holding secrets.
- Write-back to TOML files, snapshot files and lock files.
- The fake API in `ndac-testkit` only insofar as it could leak into release
  builds.
- Supply chain: release artifacts, signatures, attestations, SBOMs, container
  images and the CI workflows that produce them.

Out of scope:

- Vulnerabilities in the NextDNS service or API; report those to NextDNS.
- Attacks that require an already compromised local user account.
- Generated endpoint configuration that the user installs manually, unless the
  generated content itself is unsafe.
