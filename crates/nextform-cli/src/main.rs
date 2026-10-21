//! `nextform`: NextDNS profiles as code.

// Not `forbid`: the Linux `prctl(PR_SET_DUMPABLE)` hardening call will live in
// a single module of this crate with a scoped `allow`.
#![deny(unsafe_code)]

use std::process::ExitCode;

fn main() -> ExitCode {
    ExitCode::SUCCESS
}
