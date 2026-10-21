//! Process hardening applied before anything else runs.

/// Marks the process non-dumpable on Linux: no core dumps and no ptrace
/// attach by other processes of the same user, so the in-memory key cannot
/// be read from a dump or a debugger. Failure is not fatal.
pub(crate) fn apply() {
    #[cfg(target_os = "linux")]
    if let Err(e) = nix::sys::prctl::set_dumpable(false) {
        // Logging is not set up yet; this is advisory hardening only.
        let _ = e;
    }
}
