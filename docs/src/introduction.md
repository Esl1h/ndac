# Introduction

`ndac` (NextDNS-as-Code) manages NextDNS profiles declaratively. Desired state
lives in TOML files, live state comes from the NextDNS API, and the difference
between the two is the plan.

Start with [Installation](installation.md), then
[Configuration](configuration.md). The
[examples](https://github.com/Esl1h/ndac/tree/main/examples) directory has
complete configuration repositories: `minimal` (one profile), `multi-profile`
(templates shared by several profiles) and `paranoid` (strict protection,
no logs), plus the GitHub Actions workflow described in
[CI recipes](ci.md).
