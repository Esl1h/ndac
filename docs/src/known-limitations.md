# Known limitations

Items here are assumptions that could not be verified against an official
source. Each one is revisited when the related feature is implemented.

## NextDNS API key format

The custom gitleaks rule in `.gitleaks.toml` assumes keys are 40 lowercase
hexadecimal characters, based on keys shown in the NextDNS dashboard. NextDNS
does not document the format. If the format differs, secret scanning still
relies on the default gitleaks rules and on the generic `api_key = ...`
patterns.
