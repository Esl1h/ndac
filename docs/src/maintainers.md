# Maintainer guide

## Repository settings checklist

These settings cannot be expressed as files in the repository and must be
configured in the GitHub UI or API.

- [ ] Branch ruleset on `main`: require a pull request, 1 approving review
      from CODEOWNERS, required status checks (all `ci.yml` and `security.yml`
      jobs), linear history, signed commits, block force pushes and deletion.
- [ ] Tag ruleset for `v*`: only maintainers and the release GitHub App
      (see below) may create tags; block updates and deletion.
- [ ] Secret scanning and push protection enabled.
- [ ] Private vulnerability reporting enabled (used by `SECURITY.md`).
- [ ] Dependabot alerts and security updates enabled.
- [ ] Code scanning enabled (receives CodeQL and Scorecard SARIF).
- [ ] Environment `release` with required reviewers, deployments limited
      to `main` (crates.io publishing) and tags `v*` (binaries, image,
      packages).
- [ ] Environment `release-plz` without reviewers, deployments limited to
      `main` (opens the release pull request).
- [ ] Environment `nextdns-e2e` with required reviewers, holding
      `NEXTDNS_E2E_API_KEY` and `NEXTDNS_E2E_PROFILE_ID`.
- [ ] GitHub Pages source set to GitHub Actions.
- [ ] Immutable releases enabled, if available for the account.
- [ ] Codecov: repository activated, OIDC upload allowed.
- [ ] Actions settings: "Read repository contents" as default `GITHUB_TOKEN`
      permission; Actions may not create or approve pull requests (release-plz
      uses its GitHub App instead).

## Releases

Releases are cut by merging the pull request that release-plz keeps open on
`main` (`chore: release vX.Y.Z`):

1. `release-plz.yml` opens or updates that pull request on every push to
   `main`: version bump of `ndac`, `ndac-core` and `ndac-api` (one version
   for all three) and the new `CHANGELOG.md` section.
2. Merging it runs the `publish` job in the `release` environment: it
   publishes the three crates to crates.io with trusted publishing, creates
   the tag `vX.Y.Z` and a draft GitHub release.
3. The tag starts `release.yml`: binaries for six targets built with
   `cargo auditable`, archives with completions, the man page and the systemd
   units, `.deb` and `.rpm` packages, a CycloneDX SBOM, `SHA256SUMS` signed
   with cosign, build provenance attestations, and the image
   `ghcr.io/esl1h/ndac` (signed, with SBOM and provenance). The draft release
   is published once every asset is uploaded, so it also works with immutable
   releases.
4. Publishing the release starts `packaging.yml`, which updates the AUR
   package `ndac-bin` and triggers the COPR build, when enabled.

`just release-dry` builds this host's archive and `SHA256SUMS` into
`target/dist` the same way, without publishing anything.

### One-time setup

- [ ] **GitHub App** for release-plz, installed on this repository only, with
      repository permissions Contents and Pull requests set to read and write.
      Pull requests and tags created with `GITHUB_TOKEN` would not start
      workflows. Store its client ID as the variable `RELEASE_APP_CLIENT_ID`
      and a private key as the secret `RELEASE_APP_PRIVATE_KEY` in both the
      `release-plz` and `release` environments.
- [ ] **crates.io**: trusted publishing cannot create a crate, so the first
      version is published by hand. Set the workspace version (for example
      `0.1.0`), then `cargo publish -p ndac-api`, `-p ndac-core` and `-p ndac`
      with a short-lived token, in that order. Then, for each crate, add a
      trusted publisher: repository `Esl1h/ndac`, workflow `release-plz.yml`,
      environment `release`. Revoke the token. Push the tag `v0.1.0` by hand
      for the first binary release.
- [ ] **GHCR**: after the first image push, make the package
      `ghcr.io/esl1h/ndac` public and link it to this repository.
- [ ] **AUR** (optional): create the package `ndac-bin` on the AUR with a
      dedicated SSH key. Store the private key as the secret
      `AUR_SSH_PRIVATE_KEY` in the `release` environment, the commit identity
      as the variables `AUR_GIT_NAME` and `AUR_GIT_EMAIL`, and set the
      variable `AUR_ENABLED` to `true`.
- [ ] **COPR** (optional): create a COPR project with a package `ndac` using
      the SCM source, the "make srpm" method, this repository and spec
      `packaging/rpm/ndac.spec`; chroots for Fedora on x86_64 and aarch64.
      Store its custom webhook URL as the secret `COPR_WEBHOOK_URL` in the
      `release` environment and set the variable `COPR_ENABLED` to `true`.

## Verifying a release

```sh
gh attestation verify ndac-0.1.0-x86_64-unknown-linux-musl.tar.gz --repo Esl1h/ndac
cosign verify-blob SHA256SUMS --bundle SHA256SUMS.sigstore.json \
  --certificate-identity "https://github.com/Esl1h/ndac/.github/workflows/release.yml@refs/tags/v0.1.0" \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
sha256sum -c --ignore-missing SHA256SUMS
cosign verify ghcr.io/esl1h/ndac:v0.1.0 \
  --certificate-identity "https://github.com/Esl1h/ndac/.github/workflows/release.yml@refs/tags/v0.1.0" \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
```
