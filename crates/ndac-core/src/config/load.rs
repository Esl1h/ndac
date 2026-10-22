//! Discovery and loading of a configuration directory.

use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::{Component, Path, PathBuf},
};

use globset::{Glob, GlobSetBuilder};

use super::{
    check,
    desired::{DesiredProfile, desired, prenormalize},
    error::{ConfigError, ConfigErrors},
    merge::merge,
    schema::{SchemaKind, json_schema},
    source::{Seg, SourceFile},
    types::{
        AuthConfig, ColorChoice, LocalFile, OutputConfig, ProfileFile, ProviderConfig, RootFile,
        UserFile,
    },
    values::suggest,
};

/// Root file name.
pub const ROOT_FILE: &str = "ndac.toml";
/// Local, git-ignored override file, next to the root file.
pub const LOCAL_FILE: &str = "ndac.local.toml";
/// Directory whose files are templates; other included files are profiles.
pub const TEMPLATES_DIR: &str = "templates";

/// The per-user configuration file: `$XDG_CONFIG_HOME/ndac/config.toml`, or
/// `~/.config/ndac/config.toml` when `XDG_CONFIG_HOME` is unset or not
/// absolute, as the XDG Base Directory specification requires. The same
/// path is used on every platform. `None` without a usable home directory.
#[must_use]
pub fn user_config_path(
    xdg_config_home: Option<OsString>,
    home: Option<OsString>,
) -> Option<PathBuf> {
    let base = xdg_config_home
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            home.map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .map(|h| h.join(".config"))
        })?;
    Some(base.join("ndac").join("config.toml"))
}

/// The `[output].color` preference, read without validating anything else
/// so that it can apply before a command loads its configuration: the local
/// override, then the root file, then the user file. Unreadable or invalid
/// files count as unset; commands that load them report the problem.
#[must_use]
pub fn color_preference(root_path: Option<&Path>, user_path: Option<&Path>) -> Option<ColorChoice> {
    #[derive(serde::Deserialize)]
    struct OutputOnly {
        #[serde(default)]
        output: OutputConfig,
    }
    let color = |p: &Path| {
        let text = fs::read_to_string(p).ok()?;
        toml::from_str::<OutputOnly>(&text).ok()?.output.color
    };
    let local = root_path.and_then(Path::parent).map(|d| d.join(LOCAL_FILE));
    local
        .as_deref()
        .and_then(color)
        .or_else(|| root_path.and_then(color))
        .or_else(|| user_path.and_then(color))
}

/// Finds the root file: `explicit` if given, else the nearest `ndac.toml`
/// in `cwd` or a parent directory.
///
/// # Errors
///
/// [`ConfigError::NotFound`] or [`ConfigError::Io`].
pub fn discover(explicit: Option<&Path>, cwd: &Path) -> Result<PathBuf, ConfigError> {
    if let Some(p) = explicit {
        return if p.is_file() {
            Ok(p.to_path_buf())
        } else {
            Err(ConfigError::Io {
                path: p.to_path_buf(),
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "file not found"),
            })
        };
    }
    for dir in cwd.ancestors() {
        let candidate = dir.join(ROOT_FILE);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(ConfigError::NotFound {
        start: cwd.to_path_buf(),
    })
}

/// Merges `layers` in order onto an empty file.
#[must_use]
pub fn fold_layers(layers: &[&Entry]) -> ProfileFile {
    layers.iter().fold(ProfileFile::default(), |acc, e| {
        let mut own = e.file.clone();
        prenormalize(&mut own);
        merge(acc, own)
    })
}

/// A template or profile file.
#[derive(Debug)]
pub struct Entry {
    /// The source.
    pub src: SourceFile,
    /// Its parsed content.
    pub file: ProfileFile,
}

/// A loaded and validated configuration directory.
#[derive(Debug)]
pub struct Workspace {
    /// Directory of the root file.
    pub root_dir: PathBuf,
    /// The root file.
    pub root_src: SourceFile,
    /// Its content.
    pub root: RootFile,
    /// The local override, if present.
    pub local: Option<(SourceFile, LocalFile)>,
    /// The per-user file, if loaded with [`Workspace::load_user`].
    pub user: Option<(SourceFile, UserFile)>,
    /// Templates by key.
    pub templates: BTreeMap<String, Entry>,
    /// Profiles by key.
    pub profiles: BTreeMap<String, Entry>,
}

impl Workspace {
    /// Loads and validates every file reachable from `root_path`.
    ///
    /// # Errors
    ///
    /// All problems found, across files.
    pub fn load(root_path: &Path) -> Result<Self, ConfigErrors> {
        let root_dir = root_path
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        let mut errors = Vec::new();

        let root_src = read(root_path, &root_dir)?;
        let root: RootFile =
            parse_checked(&root_src, SchemaKind::Root, &mut errors).ok_or_else(|| {
                ConfigErrors {
                    errors: std::mem::take(&mut errors),
                }
            })?;
        errors.extend(check::root_file(&root_src, &root));

        let local_path = root_dir.join(LOCAL_FILE);
        let local = if local_path.is_file() {
            let src = read(&local_path, &root_dir)?;
            parse_checked::<LocalFile>(&src, SchemaKind::Local, &mut errors).map(|l| {
                check::provider(&src, &l.provider, &mut errors);
                check::auth(&src, &l.auth, &[Seg::Key("auth")], &mut errors);
                (src, l)
            })
        } else {
            None
        };

        let mut templates = BTreeMap::new();
        let mut profiles = BTreeMap::new();
        match include_files(&root_dir, &root.include) {
            Ok(files) => {
                for path in files {
                    let src = match read(&path, &root_dir) {
                        Ok(s) => s,
                        Err(e) => {
                            errors.extend(e.errors);
                            continue;
                        }
                    };
                    let Some(file) =
                        parse_checked::<ProfileFile>(&src, SchemaKind::Profile, &mut errors)
                    else {
                        continue;
                    };
                    let is_template = is_template(&root_dir, &path);
                    errors.extend(check::profile_file(&src, &file, is_template));
                    let key = path
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let target = if is_template {
                        &mut templates
                    } else {
                        &mut profiles
                    };
                    if let Some(first) = target.get(&key) {
                        let first: &Entry = first;
                        errors.push(src.invalid(
                            Some(0..0),
                            format!(
                                "duplicate {} `{key}`, also defined in {}",
                                if is_template { "template" } else { "profile" },
                                first.src.display
                            ),
                            "file stems must be unique",
                            None,
                        ));
                        continue;
                    }
                    target.insert(key, Entry { src, file });
                }
            }
            Err(e) => errors.extend(e),
        }

        let ws = Self {
            root_dir,
            root_src,
            root,
            local,
            user: None,
            templates,
            profiles,
        };
        errors.extend(ws.check_extends());
        if errors.is_empty() {
            Ok(ws)
        } else {
            Err(ConfigErrors { errors })
        }
    }

    fn check_extends(&self) -> Vec<ConfigError> {
        let mut out = Vec::new();
        for entry in self.templates.values().chain(self.profiles.values()) {
            for (i, name) in entry.file.extends.iter().enumerate() {
                if !self.templates.contains_key(name) {
                    out.push(
                        entry.src.invalid(
                            entry.src.value_span(&[Seg::Key("extends"), Seg::Index(i)]),
                            format!("unknown template `{name}`"),
                            "not found",
                            suggest(name, self.templates.keys().map(String::as_str))
                                .map(|s| format!("did you mean `{s}`?"))
                                .or_else(|| {
                                    Some(format!("templates are files under {TEMPLATES_DIR}/"))
                                }),
                        ),
                    );
                }
            }
        }
        out
    }

    /// Loads the per-user file at `path`, if it exists, below the repository
    /// files.
    ///
    /// # Errors
    ///
    /// Unreadable or invalid file.
    pub fn load_user(&mut self, path: &Path) -> Result<(), ConfigErrors> {
        if !path.is_file() {
            return Ok(());
        }
        let src = read(path, &self.root_dir)?;
        let mut errors = Vec::new();
        if let Some(u) = parse_checked::<UserFile>(&src, SchemaKind::User, &mut errors) {
            check::auth(&src, &u.auth, &[Seg::Key("auth")], &mut errors);
            if errors.is_empty() {
                self.user = Some((src, u));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(ConfigErrors { errors })
        }
    }

    /// Effective `[provider]`: root merged with the local override.
    #[must_use]
    pub fn provider(&self) -> ProviderConfig {
        let root = self.root.provider.clone();
        let Some((_, local)) = &self.local else {
            return root;
        };
        let l = local.provider.clone();
        ProviderConfig {
            api_url: l.api_url.or(root.api_url),
            timeout: l.timeout.or(root.timeout),
            retries: l.retries.or(root.retries),
            concurrency: l.concurrency.or(root.concurrency),
            snapshot_dir: l.snapshot_dir.or(root.snapshot_dir),
            snapshot_keep: l.snapshot_keep.or(root.snapshot_keep),
        }
    }

    /// Effective root `[auth]`: the local override, then the root file,
    /// then the user file.
    #[must_use]
    pub fn auth(&self) -> AuthConfig {
        let local = self
            .local
            .as_ref()
            .map(|(_, l)| l.auth.clone())
            .unwrap_or_default();
        let repo = or_auth(local, &self.root.auth);
        match &self.user {
            Some((_, u)) => or_auth(repo, &u.auth),
            None => repo,
        }
    }

    /// Effective `[output]`.
    #[must_use]
    pub fn output(&self) -> OutputConfig {
        let local = self.local.as_ref().and_then(|(_, l)| l.output.color);
        let user = self.user.as_ref().and_then(|(_, u)| u.output.color);
        OutputConfig {
            color: local.or(self.root.output.color).or(user),
        }
    }

    /// Resolves one profile.
    ///
    /// # Errors
    ///
    /// Unknown profile or an `extends` cycle.
    pub fn resolve(&self, key: &str) -> Result<DesiredProfile, ConfigErrors> {
        let Some(entry) = self.profiles.get(key) else {
            let hint = suggest(key, self.profiles.keys().map(String::as_str))
                .map(|s| format!("did you mean `{s}`?"));
            return Err(self
                .root_src
                .invalid(None, format!("unknown profile `{key}`"), "", hint)
                .into());
        };
        let mut layers = Vec::new();
        self.layers(
            &entry.src,
            &entry.file.extends,
            &mut layers,
            &mut Vec::new(),
        )?;
        layers.push(entry);
        Ok(desired(key, fold_layers(&layers), entry.src.path.clone()))
    }

    /// The files a profile is built from, in the order they are applied:
    /// templates (each one's own `extends` first), then the profile.
    ///
    /// # Errors
    ///
    /// Unknown profile or an `extends` cycle.
    pub fn layer_entries(&self, key: &str) -> Result<Vec<&Entry>, ConfigErrors> {
        let entry = self.profiles.get(key).ok_or_else(|| {
            ConfigErrors::from(self.root_src.invalid(
                None,
                format!("unknown profile `{key}`"),
                "",
                None,
            ))
        })?;
        let mut layers = Vec::new();
        self.layers(
            &entry.src,
            &entry.file.extends,
            &mut layers,
            &mut Vec::new(),
        )?;
        layers.push(entry);
        Ok(layers)
    }

    /// Resolves every profile, in key order.
    ///
    /// # Errors
    ///
    /// All resolution problems.
    pub fn resolve_all(&self) -> Result<Vec<DesiredProfile>, ConfigErrors> {
        let mut out = Vec::new();
        let mut errors = Vec::new();
        for key in self.profiles.keys() {
            match self.resolve(key) {
                Ok(p) => out.push(p),
                Err(e) => errors.extend(e.errors),
            }
        }
        if errors.is_empty() {
            Ok(out)
        } else {
            Err(ConfigErrors { errors })
        }
    }

    /// Appends the layers of `extends`, depth first: each template's own
    /// `extends`, then the template itself. Layers are applied in order, so a
    /// `$inherit` or `$remove` in a template refers to everything stacked
    /// before it.
    fn layers<'s>(
        &'s self,
        from: &SourceFile,
        extends: &[String],
        out: &mut Vec<&'s Entry>,
        stack: &mut Vec<String>,
    ) -> Result<(), ConfigError> {
        for (i, name) in extends.iter().enumerate() {
            if let Some(pos) = stack.iter().position(|s| s == name) {
                let mut chain: Vec<&str> = stack[pos..].iter().map(String::as_str).collect();
                chain.push(name);
                return Err(from.invalid(
                    from.value_span(&[Seg::Key("extends"), Seg::Index(i)]),
                    format!("template cycle: {}", chain.join(" -> ")),
                    "closes the cycle",
                    None,
                ));
            }
            // Unknown templates are reported by check_extends during load.
            let Some(t) = self.templates.get(name) else {
                continue;
            };
            stack.push(name.clone());
            self.layers(&t.src, &t.file.extends, out, stack)?;
            stack.pop();
            out.push(t);
        }
        Ok(())
    }

    /// Every file `fmt` should format: root, local override, includes.
    #[must_use]
    pub fn files(&self) -> Vec<&SourceFile> {
        let mut out = vec![&self.root_src];
        out.extend(self.local.as_ref().map(|(s, _)| s));
        out.extend(self.templates.values().map(|e| &e.src));
        out.extend(self.profiles.values().map(|e| &e.src));
        out
    }
}

/// Paths of every configuration file, without validating their content.
/// Used by `fmt`, which must work on files that do not validate yet.
///
/// # Errors
///
/// Unreadable root file or unsafe include patterns.
pub fn config_files(root_path: &Path) -> Result<Vec<PathBuf>, ConfigErrors> {
    let root_dir = root_path
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let src = read(root_path, &root_dir)?;
    let include: Vec<String> = src
        .doc()
        .get("include")
        .and_then(|i| i.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let mut out = vec![root_path.to_path_buf()];
    let local = root_dir.join(LOCAL_FILE);
    if local.is_file() {
        out.push(local);
    }
    out.extend(include_files(&root_dir, &include).map_err(|errors| ConfigErrors { errors })?);
    Ok(out)
}

/// `top`, with unset fields taken from `below`.
fn or_auth(top: AuthConfig, below: &AuthConfig) -> AuthConfig {
    let b = below.clone();
    AuthConfig {
        sources: top.sources.or(b.sources),
        api_key: top.api_key.or(b.api_key),
        on_source_error: top.on_source_error.or(b.on_source_error),
        allow_insecure_permissions: top
            .allow_insecure_permissions
            .or(b.allow_insecure_permissions),
        quiet_literal: top.quiet_literal.or(b.quiet_literal),
        cmd_timeout: top.cmd_timeout.or(b.cmd_timeout),
    }
}

fn read(path: &Path, root_dir: &Path) -> Result<SourceFile, ConfigErrors> {
    let text = fs::read_to_string(path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let display = path
        .strip_prefix(root_dir)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned();
    Ok(SourceFile::parse(path, display, text)?)
}

fn parse_checked<T: serde::de::DeserializeOwned>(
    src: &SourceFile,
    kind: SchemaKind,
    errors: &mut Vec<ConfigError>,
) -> Option<T> {
    let unknown = check::unknown_keys(src, &json_schema(kind));
    if !unknown.is_empty() {
        errors.extend(unknown);
        return None;
    }
    match check::deserialize(src) {
        Ok(v) => Some(v),
        Err(e) => {
            errors.push(e);
            None
        }
    }
}

fn is_template(root_dir: &Path, path: &Path) -> bool {
    path.strip_prefix(root_dir)
        .ok()
        .and_then(|rel| rel.components().next())
        .is_some_and(|c| c.as_os_str() == TEMPLATES_DIR)
}

/// Expands `include` globs relative to `root_dir`, sorted. Rejects absolute
/// patterns, `..`, and files or symlinks resolving outside the root.
///
/// # Errors
///
/// Every rejected pattern or escaping file.
pub fn include_files(
    root_dir: &Path,
    patterns: &[String],
) -> Result<Vec<PathBuf>, Vec<ConfigError>> {
    let mut errors = Vec::new();
    let mut builder = GlobSetBuilder::new();
    for p in patterns {
        let as_path = Path::new(p);
        if as_path.is_absolute() || p.starts_with('/') || p.starts_with('\\') {
            errors.push(ConfigError::Include {
                pattern: p.clone(),
                reason: "must be relative to the directory of ndac.toml".into(),
            });
            continue;
        }
        if as_path
            .components()
            .any(|c| matches!(c, Component::ParentDir))
        {
            errors.push(ConfigError::Include {
                pattern: p.clone(),
                reason: "`..` is not allowed".into(),
            });
            continue;
        }
        match Glob::new(p) {
            Ok(g) => {
                builder.add(g);
            }
            Err(e) => errors.push(ConfigError::Include {
                pattern: p.clone(),
                reason: e.kind().to_string(),
            }),
        }
    }
    let set = match builder.build() {
        Ok(s) => s,
        Err(e) => {
            errors.push(ConfigError::Include {
                pattern: patterns.join(", "),
                reason: e.to_string(),
            });
            return Err(errors);
        }
    };

    let canonical_root = fs::canonicalize(root_dir).unwrap_or_else(|_| root_dir.to_path_buf());
    let mut out = Vec::new();
    let mut stack = vec![root_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let Ok(ft) = entry.file_type() else { continue };
            // Symlinked directories are not followed.
            if ft.is_symlink() && path.is_dir() {
                continue;
            }
            if ft.is_dir() {
                // Hidden directories hold VCS data and snapshots; target/ is
                // build output.
                if !name.to_string_lossy().starts_with('.') && name != "target" {
                    stack.push(path);
                }
                continue;
            }
            let Ok(rel) = path.strip_prefix(root_dir) else {
                continue;
            };
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            if rel_str == ROOT_FILE || rel_str == LOCAL_FILE || !set.is_match(&rel_str) {
                continue;
            }
            match fs::canonicalize(&path) {
                Ok(target) if target.starts_with(&canonical_root) && target.is_file() => {
                    out.push(path);
                }
                Ok(_) => errors.push(ConfigError::Escape { path }),
                Err(source) => errors.push(ConfigError::Io { path, source }),
            }
        }
    }
    out.sort();
    if errors.is_empty() {
        Ok(out)
    } else {
        Err(errors)
    }
}
