# Installation

Each GitHub release of `ndac` provides:

- archives for Linux (x86_64 and aarch64, glibc and static musl) and macOS
  (x86_64 and Apple silicon), with shell completions, the man page and, on
  Linux, the systemd units;
- `.deb` and `.rpm` packages of the static Linux binary;
- a container image, `ghcr.io/esl1h/ndac`;
- `SHA256SUMS`, signed with cosign, a CycloneDX SBOM, and build provenance
  attestations for every file.

## Archives

```sh
version=0.1.0
target=x86_64-unknown-linux-musl
curl -fsSLO "https://github.com/Esl1h/ndac/releases/download/v$version/ndac-$version-$target.tar.gz"
gh attestation verify "ndac-$version-$target.tar.gz" --repo Esl1h/ndac
tar -xzf "ndac-$version-$target.tar.gz"
install -m 0755 "ndac-$version-$target/ndac" ~/.local/bin/
```

The musl builds are static and run on any Linux distribution. See
[Shell completions and man page](completions.md) for the extra files.

## Packages

- Debian and Ubuntu: `sudo apt install ./ndac_0.1.0-1_amd64.deb`
- Fedora and RHEL: `sudo dnf install ./ndac-0.1.0-1.x86_64.rpm`, or from the
  COPR repository when it is enabled
- Arch Linux: the AUR package `ndac-bin`
- From source: `cargo install --locked ndac`

Packages install the binary, completions, the man page and the systemd
units; no unit is enabled.

## Container

```sh
docker run --rm -v "$PWD:/work" -e NEXTDNS_API_KEY ghcr.io/esl1h/ndac:v0.1.0 plan
```

The image contains only the static binary and CA certificates, and runs as
a non-root user in `/work`. Verify it with `gh attestation verify
oci://ghcr.io/esl1h/ndac:v0.1.0 --repo Esl1h/ndac` or with `cosign verify`
(see the [maintainer guide](maintainers.md#verifying-a-release)).

## Scheduled drift checks with systemd

`ndac-drift.service` and `ndac-drift.timer` run `ndac drift --format prom`
every hour and write `/var/lib/ndac/ndac_drift.prom` for the
`node_exporter` textfile collector (`--collector.textfile.directory`). The
service runs as a dynamic user with a strict sandbox and reads the API key
from an encrypted systemd credential:

```sh
sudo install -d /etc/credstore.encrypted
printf '%s' "$KEY" | sudo systemd-creds encrypt --name=nextdns-api-key - \
  /etc/credstore.encrypted/nextdns-api-key
sudo systemctl enable --now ndac-drift.timer
```

It expects the configuration repository in `/etc/ndac`, readable by any
user. Drift is measured against the `post-apply` snapshots, so the snapshot
directory must hold the snapshots of the machine or pipeline that applies
the configuration, readable by the service.
