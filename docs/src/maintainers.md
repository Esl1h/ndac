# Maintainer guide

## Repository settings checklist

These settings cannot be expressed as files in the repository and must be
configured in the GitHub UI or API.

- [ ] Branch ruleset on `main`: require a pull request, 1 approving review
      from CODEOWNERS, required status checks (all `ci.yml` and `security.yml`
      jobs), linear history, signed commits, block force pushes and deletion.
- [ ] Tag ruleset for `v*`: only maintainers and the release workflow may
      create tags; block updates and deletion.
- [ ] Secret scanning and push protection enabled.
- [ ] Private vulnerability reporting enabled (used by `SECURITY.md`).
- [ ] Dependabot alerts and security updates enabled.
- [ ] Code scanning enabled (receives CodeQL and Scorecard SARIF).
- [ ] Environment `release` with required reviewers, deployment limited to
      tags `v*`.
- [ ] Environment `nextdns-e2e` with required reviewers, holding
      `NEXTDNS_E2E_API_KEY` and `NEXTDNS_E2E_PROFILE_ID`.
- [ ] GitHub Pages source set to GitHub Actions.
- [ ] Immutable releases enabled, if available for the account.
- [ ] Codecov: repository activated, OIDC upload allowed.
- [ ] Actions settings: "Read repository contents" as default `GITHUB_TOKEN`
      permission; Actions may not create or approve pull requests except for
      release-plz.
