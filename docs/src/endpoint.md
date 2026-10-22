# Endpoints and doctor

## `ndac endpoint render`

```sh
ndac endpoint render casa --target nextdns-cli
ndac endpoint render casa --target resolved --device "Office PC"
ndac endpoint render casa --target networkmanager --device Laptop
ndac endpoint render casa --target android --device "Pixel 8"
ndac endpoint render casa --target doh-url --device "Pixel 8"
```

`endpoint render` prints configuration text for a profile to stdout. It never
writes system files and never runs commands: review the output, then install
it yourself. The profile needs an `id`, so apply new profiles first.

### Targets

- **`nextdns-cli`**: `/etc/nextdns.conf` for the
  [`nextdns` client](https://github.com/nextdns/nextdns), one `key value`
  line per option, in the format the client itself writes. Options come from
  the profile's `[endpoint]` table:

  ```toml
  [endpoint]
  cacheSize        = "10MB"   # cache-size
  reportClientInfo = true     # report-client-info

  [endpoint.conditional]       # one `profile <selector>=<id>` line each
  "10.0.3.0/24"       = "kids"     # a profile key...
  "00:1c:42:2e:60:4a" = "def456"   # ...or a NextDNS profile ID
  ```

  Selectors are what the client accepts before `=`: an IPv4 or IPv6 CIDR, a
  MAC address, an interface name or `@user`. Conditional lines come before the
  default profile because the client uses the first match. `--device` does
  not apply: the client sends device names itself when `reportClientInfo` is
  on.
- **`resolved`**: a systemd-resolved drop-in using DNS over TLS, with the
  NextDNS IPv4 anycast addresses `45.90.28.0` and `45.90.30.0` and the
  profile hostname as TLS server name. The profile's IPv6 addresses are
  only added with `--api-ipv6`, in builds with the `unstable-endpoints`
  feature (see [Undocumented endpoints](unstable.md)).
- **`networkmanager`**: a POSIX shell script with `nmcli` commands that set
  the same servers on one connection (`CONNECTION=... sh script.sh`). DNS over
  TLS in NetworkManager requires systemd-resolved as its DNS backend.
- **`android`**: the Private DNS hostname.
- **`doh-url`**: the DNS-over-HTTPS URL.

### Device names

With `--device`, queries appear under that name in the NextDNS logs:

- DNS over TLS (`resolved`, `networkmanager`, `android`) puts the name in
  the hostname: `<device>-<id>.dns.nextdns.io`, where spaces become `--`
  (`Office PC` gives `Office--PC-abc123.dns.nextdns.io`). Only ASCII
  letters, digits, `-` and spaces are allowed, and the label must fit in 63
  characters.
- DNS over HTTPS appends the URL-encoded name:
  `https://dns.nextdns.io/abc123/Pixel%208`.

## `ndac doctor`

```sh
ndac doctor
ndac doctor --expect-profile casa --expect-device Laptop
```

`doctor` asks <https://test.nextdns.io> how this host's queries reach
NextDNS and prints a checklist:

```text
✓ queries reach NextDNS (protocol DOT, server example-1)
✓ profile abc123
✓ device name "Laptop"
```

It exits with `0` when every check passes, `2` on a mismatch (queries not
reaching NextDNS, another profile or device name) and `5` when the test
service cannot be reached. Unencrypted queries (`UDP`) are reported with `!`
but do not fail the check.

`--expect-profile` takes a profile key from the configuration, if there is
one, or a NextDNS profile ID. The test service sometimes shows the profile as
a fingerprint (`fp…`) instead of its ID; then the profile cannot be compared
and is reported with `?`.

`doctor` does not use the API key and does not need a configuration file.
Each run asks a fresh random subdomain of `test.nextdns.io`, so the answer
cannot come from a DNS cache.
