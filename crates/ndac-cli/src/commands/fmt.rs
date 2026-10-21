//! `ndac fmt`.

use std::{
    fs,
    path::{Path, PathBuf},
};

use miette::{NamedSource, Report};
use ndac_core::config::{ConfigError, ConfigErrors, config_files, format_toml};

use super::{CmdResult, Failure, root_path};
use crate::output::{Exit, Output};

pub(crate) fn run(config: Option<&Path>, check: bool, out: Output) -> CmdResult {
    let root = root_path(config)?;
    let root_dir = root
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let files = config_files(&root)?;

    let mut errors = Vec::new();
    let mut changed = Vec::new();
    for path in files {
        let display = path
            .strip_prefix(&root_dir)
            .unwrap_or(&path)
            .display()
            .to_string();
        let text = fs::read_to_string(&path).map_err(|source| {
            ConfigErrors::from(ConfigError::Io {
                path: path.clone(),
                source,
            })
        })?;
        let formatted = match format_toml(&text) {
            Ok(f) => f,
            Err(e) => {
                errors.push(ConfigError::Syntax {
                    message: e.message().to_owned(),
                    span: e.span().map(Into::into),
                    src: NamedSource::new(display, text),
                });
                continue;
            }
        };
        if formatted == text {
            continue;
        }
        if !check {
            write_atomic(&path, &formatted)
                .map_err(|e| Failure::Other(Report::msg(format!("cannot write {display}: {e}"))))?;
        }
        changed.push(display);
    }
    if !errors.is_empty() {
        return Err(Failure::Config(ConfigErrors { errors }));
    }

    for file in &changed {
        out.status(&if check {
            format!("would reformat {file}")
        } else {
            format!("reformatted {file}")
        });
    }
    if check && !changed.is_empty() {
        return Ok(Exit::Error);
    }
    if changed.is_empty() {
        out.status("all files are formatted");
    }
    Ok(Exit::Success)
}

/// Replaces `path` without leaving a truncated file behind on failure and
/// keeps its permissions (files may hold a literal key and be mode 0600).
fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let perms = fs::metadata(path)?.permissions();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.ndac-fmt.tmp"));
    fs::write(&tmp, text)?;
    fs::set_permissions(&tmp, perms)?;
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}
