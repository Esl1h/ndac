# CI recipes

A configuration repository usually runs three jobs:

- on a pull request, `ndac plan` posted as a comment, so reviewers see what
  the change does to the live profiles;
- on a merge to `main`, `ndac apply --auto-approve`, behind an approval;
- on a schedule, `ndac drift`, which opens an issue when someone changed a
  profile outside `ndac`.

## GitHub Actions

`ndac init --github-actions` writes `.github/workflows/nextdns-gitops.yml`,
the same file as
[`examples/github-actions/nextdns-gitops.yml`](https://github.com/Esl1h/ndac/blob/main/examples/github-actions/nextdns-gitops.yml).
It needs two environments, each with the secret `NEXTDNS_API_KEY`:

- `nextdns-plan`, for the plan and drift jobs. Fork pull requests never get
  the key and are not planned.
- `nextdns`, for the apply job. Add required reviewers so every apply is
  approved, and limit deployments to `main`.

The workflow downloads a pinned release (`NDAC_VERSION`), checks it against
`SHA256SUMS` and with `gh attestation verify` before running it.

## The API key in CI

The key can read and change the whole account, DNS logs included, so keep
it in environment secrets rather than repository secrets, and give it only
to the jobs that need it. `env:NEXTDNS_API_KEY` is in the default source
chain. To avoid a long-lived secret in GitHub, fetch the key at run time
with OIDC from a secrets manager and point `ndac` at the file:

```yaml
- uses: aws-actions/configure-aws-credentials@<sha> # pin it
  with:
    role-to-assume: arn:aws:iam::123456789012:role/nextdns-ci
    aws-region: eu-west-1
- run: |
    (umask 077 && aws secretsmanager get-secret-value --secret-id nextdns \
      --query SecretString --output text > "$RUNNER_TEMP/nextdns-key")
- run: ndac plan
  env:
    NDAC_AUTH_SOURCE: file:${{ runner.temp }}/nextdns-key
```

`hashicorp/vault-action` with JWT authentication works the same way.

## Snapshots in CI

Drift is found by comparing the live profile with the snapshot `ndac`
takes after each verified apply (see [Plan and drift](plan.md)). A CI runner
starts empty, so the workflow keeps `.ndac/snapshots` as the artifact
`ndac-snapshots` after each apply and downloads the newest one before the
next apply or drift check. Without a snapshot every difference is reported
as pending and `ndac drift` finds nothing.

## Exit codes in scripts

- `ndac plan --detailed-exitcode` exits 0 without changes and 2 with
  changes.
- `ndac drift` exits 0 without drift and 2 with drift.
- Anything else is an error; see [Exit codes](apply.md#exit-codes).

```sh
rc=0
ndac drift --format github || rc=$?
case $rc in
  0) echo "no drift" ;;
  2) echo "drift found" ;;
  *) exit "$rc" ;;
esac
```

`--format github` prints workflow annotations, `--format prom` writes a
Prometheus textfile; see [Drift and reconcile](drift.md).

## Without CI

On a server, the systemd timer shipped with the packages runs `ndac drift`
every hour and exports metrics for the node exporter; see
[Installation](installation.md#scheduled-drift-checks-with-systemd).
