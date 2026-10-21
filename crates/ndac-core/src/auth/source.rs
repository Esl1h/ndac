//! Auth source references, e.g. `env:NEXTDNS_API_KEY`.

use std::{fmt, path::PathBuf, str::FromStr};

use super::AuthError;

/// Where to look for the API key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceRef {
    /// Literal `api_key` in `ndac.local.toml`, `ndac.toml`, or the profile.
    Config,
    /// An environment variable.
    Env(String),
    /// A file whose path is in an environment variable (Docker/Kubernetes
    /// secrets convention).
    EnvFile(String),
    /// A file.
    File(PathBuf),
    /// `NEXTDNS_API_KEY` in a `.env` file: the given one, or the first of
    /// `./.env` and `<root>/.env`.
    Dotenv(Option<PathBuf>),
    /// OS keyring entry.
    Keyring {
        /// Service name.
        service: String,
        /// User name.
        user: String,
    },
    /// Output of a command, run without a shell.
    Cmd(String),
    /// `$CREDENTIALS_DIRECTORY/NAME` from systemd `LoadCredential`.
    SystemdCred(String),
    /// A file decrypted with `age` (feature `age`).
    Age(PathBuf),
}

impl FromStr for SourceRef {
    type Err = AuthError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let invalid = |reason: &str| AuthError::InvalidRef {
            input: input.to_owned(),
            reason: reason.to_owned(),
        };
        let (kind, arg) = match input.split_once(':') {
            Some((k, a)) => (k, Some(a)),
            None => (input, None),
        };
        let need = |a: Option<&str>| -> Result<String, AuthError> {
            match a.map(str::trim) {
                Some(v) if !v.is_empty() => Ok(v.to_owned()),
                _ => Err(invalid("missing argument after `:`")),
            }
        };
        match kind.trim() {
            "config" if arg.is_none() => Ok(Self::Config),
            "dotenv" => Ok(Self::Dotenv(match arg {
                None => None,
                a => Some(PathBuf::from(need(a)?)),
            })),
            "env" => Ok(Self::Env(need(arg)?)),
            "env-file" => Ok(Self::EnvFile(need(arg)?)),
            "file" => Ok(Self::File(PathBuf::from(need(arg)?))),
            "cmd" => Ok(Self::Cmd(need(arg)?)),
            "systemd-cred" => {
                let name = need(arg)?;
                if name.contains('/') || name == "." || name == ".." {
                    return Err(invalid("credential name must not contain `/`"));
                }
                Ok(Self::SystemdCred(name))
            }
            "age" => Ok(Self::Age(PathBuf::from(need(arg)?))),
            "keyring" => {
                let v = need(arg)?;
                match v.split_once('/') {
                    Some((s, u)) if !s.is_empty() && !u.is_empty() => Ok(Self::Keyring {
                        service: s.to_owned(),
                        user: u.to_owned(),
                    }),
                    _ => Err(invalid("expected keyring:SERVICE/USER")),
                }
            }
            "config" => Err(invalid("`config` takes no argument")),
            // Without a `:` this may be a key pasted by mistake: never echo it.
            _ if arg.is_none() => Err(AuthError::InvalidRef {
                input: "<hidden>".into(),
                reason: "not a source reference; never pass an API key on the command line".into(),
            }),
            _ => Err(invalid("unknown source kind")),
        }
    }
}

impl fmt::Display for SourceRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config => f.write_str("config"),
            Self::Env(v) => write!(f, "env:{v}"),
            Self::EnvFile(v) => write!(f, "env-file:{v}"),
            Self::File(p) => write!(f, "file:{}", p.display()),
            Self::Dotenv(None) => f.write_str("dotenv"),
            Self::Dotenv(Some(p)) => write!(f, "dotenv:{}", p.display()),
            Self::Keyring { service, user } => write!(f, "keyring:{service}/{user}"),
            Self::Cmd(c) => write!(f, "cmd:{c}"),
            Self::SystemdCred(n) => write!(f, "systemd-cred:{n}"),
            Self::Age(p) => write!(f, "age:{}", p.display()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        for s in [
            "config",
            "env:NEXTDNS_API_KEY",
            "env-file:NEXTDNS_API_KEY_FILE",
            "file:/run/secrets/nextdns",
            "dotenv",
            "dotenv:/etc/ndac/.env",
            "keyring:nextdns/default",
            "cmd:pass show nextdns/api-key",
            "systemd-cred:nextdns-api-key",
            "age:/home/me/key.age",
        ] {
            let parsed: SourceRef = s.parse().unwrap();
            assert_eq!(parsed.to_string(), s);
        }
    }

    #[test]
    fn rejects_malformed() {
        for s in [
            "",
            "env",
            "env:",
            "keyring:nextdns",
            "keyring:/x",
            "config:x",
            "systemd-cred:../x",
            "vault:x",
            "test-key-0000000000000000000000000000000000",
        ] {
            assert!(s.parse::<SourceRef>().is_err(), "{s}");
        }
    }

    #[test]
    fn a_pasted_key_is_never_echoed() {
        let key = "0123456789abcdef0123456789abcdef01234567";
        let err = key.parse::<SourceRef>().unwrap_err();
        assert!(!err.to_string().contains(key), "{err}");
    }
}
