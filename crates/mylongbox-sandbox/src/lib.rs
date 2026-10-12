//! Runtime self-restrictions applied at startup: a seccomp filter that
//! blocks internet sockets, and Landlock rules that limit file access.
//!
//! This is the only crate in the workspace that may use `unsafe`. Every
//! `unsafe` block must carry a `SAFETY:` comment.

#![deny(unsafe_op_in_unsafe_fn, clippy::undocumented_unsafe_blocks)]

use std::collections::BTreeMap;
use std::fmt;

use seccompiler::{
    BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter,
    SeccompRule, TargetArch,
};

/// Why the restrictions could not be applied.
#[derive(Debug)]
pub struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

fn error(what: &str, e: impl fmt::Display) -> Error {
    Error(format!("{what}: {e}"))
}

/// The filter: every socket but a local (Unix) one is refused with
/// `EACCES`, which callers treat as "not permitted", and so is setting up
/// io_uring, which can open sockets without the `socket` call. Everything
/// else is allowed; the Flatpak sandbox already limits the rest.
///
/// A call made through another architecture's system call table, such as
/// the 32-bit one on x86_64, ends the process, since the rules above
/// would not see it.
fn filter() -> Result<BpfProgram, Error> {
    let not_local = || -> Result<Vec<SeccompRule>, Error> {
        let condition = SeccompCondition::new(
            0,
            SeccompCmpArgLen::Dword,
            SeccompCmpOp::Ne,
            libc::AF_UNIX as u64,
        )
        .map_err(|e| error("building the socket rule", e))?;
        let rule =
            SeccompRule::new(vec![condition]).map_err(|e| error("building the socket rule", e))?;
        Ok(vec![rule])
    };
    let rules: BTreeMap<i64, Vec<SeccompRule>> = [
        (libc::SYS_socket, not_local()?),
        (libc::SYS_socketpair, not_local()?),
        // No conditions: always refused.
        (libc::SYS_io_uring_setup, Vec::new()),
    ]
    .into_iter()
    .collect();
    let arch = TargetArch::try_from(std::env::consts::ARCH)
        .map_err(|e| error("this processor is not supported", e))?;
    let filter = SeccompFilter::new(
        rules,
        SeccompAction::Allow,
        SeccompAction::Errno(libc::EACCES as u32),
        arch,
    )
    .map_err(|e| error("building the filter", e))?;
    filter
        .try_into()
        .map_err(|e: seccompiler::BackendError| error("compiling the filter", e))
}

/// Stops every thread of the process, now and from now on, from opening
/// any socket but a local one. It cannot be undone: the kernel keeps the
/// filter until the process ends, and also sets "no new privileges", so
/// nothing the process starts can shed it.
pub fn block_internet() -> Result<(), Error> {
    let program = filter()?;
    seccompiler::apply_filter_all_threads(&program)
        .map_err(|e| error("the kernel refused the filter", e))
}
