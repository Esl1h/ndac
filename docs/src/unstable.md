# Undocumented endpoints

The NextDNS API documentation does not describe DNS rewrites or the setup
information of a profile (its IPv6 addresses). `ndac` supports both behind
the `unstable-endpoints` build feature, which release binaries and packages
do not include:

```sh
cargo install --locked ndac --features unstable-endpoints
```

The paths and shapes follow the
[`nextdns-go`](https://github.com/amalucelli/nextdns-go) client
(`rewrites.go`, `setup.go`), which the Terraform provider uses, not the
API documentation. NextDNS can change or remove them without notice, and
the live end-to-end test does not cover them.

## Rewrites

```toml
[rewrites]
mode = "authoritative"        # or "merge", as for denylist

[rewrites.items]
"router.lan" = "192.168.1.1"  # an IP address...
"nas.lan"    = "nas.example"  # ...or a hostname
```

- Names are domains, normalized like denylist entries. Answers are IP
  addresses or hostnames.
- Templates merge `rewrites.items` by name; `"$remove"` as the answer drops
  an inherited rewrite. `rewrites.mode` is profile-only.
- A rewrite is identified by name and answer, so plans show a changed
  answer as a removal and an addition (`- router.lan → 192.168.1.1`,
  `+ router.lan → 192.168.1.2`). `apply` deletes before it adds, because
  the API keeps one rewrite per name.
- `import` writes `[rewrites]` in authoritative mode, and drift covers
  rewrites like any list. `reconcile --prefer live` cannot write rewrites
  back into the files; edit `[rewrites.items]` by hand.
- A new profile is created first, then its rewrites are added.
- `rewrites` can be used in `ignore` and `--target`.

## IPv6 servers

```sh
ndac endpoint render casa --target resolved --api-ipv6
ndac endpoint render casa --target networkmanager --api-ipv6
```

`--api-ipv6` reads the profile's IPv6 addresses from the setup endpoint and
adds them to the `resolved` and `networkmanager` output, which otherwise
only have the IPv4 anycast addresses. It needs the API key. Each address is
parsed, and anything that is not an IPv6 address is an error, because the
`networkmanager` output is a shell script.

## Not supported

- Linked IP: its settings include an update token, which is a credential;
  `ndac` drops it from the setup response instead of storing it.
- Catalogs of blocklists, natives, services and categories: no source
  describes them.
