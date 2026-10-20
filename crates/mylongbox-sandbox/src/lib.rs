//! The app's small amount of system-level code, run at startup: a
//! seccomp filter that blocks internet sockets, Landlock rules that limit
//! file access, and pointing gettext at the app's translations.
//!
//! This is the only crate in the workspace that may use `unsafe`. Every
//! `unsafe` block must carry a `SAFETY:` comment.

#![deny(unsafe_op_in_unsafe_fn, clippy::undocumented_unsafe_blocks)]

pub mod locale;

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use landlock::{
    ABI, Access, AccessFs, AccessNet, Ruleset, RulesetAttr, RulesetCreatedAttr, RulesetStatus,
    path_beneath_rules,
};

use seccompiler::{
    BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter,
    SeccompRule, TargetArch,
};

/// Why the restrictions could not be applied.
#[derive(Debug)]
pub struct Error(pub(crate) String);

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

/// How much of the file-access limits the kernel enforces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Enforced {
    Fully,
    /// An older kernel enforces some of the limits.
    Partly,
    /// The kernel has no Landlock, or it is turned off.
    Not,
}

/// The rights limited: every way of changing files, as of Landlock ABI
/// 5. Later ABIs add checks on connecting to Unix sockets by path, which
/// would cut the app off from the display and D-Bus, so they are left to
/// the Flatpak.
fn write_access() -> landlock::BitFlags<AccessFs> {
    AccessFs::from_write(ABI::V5)
}

/// From now on, lets this thread and every thread it starts change
/// files only beneath `writable`, and refuses TCP connections and
/// listening outright, behind the socket filter. Reading is left to the
/// Flatpak, which shows the app only its own folders and the system
/// files GTK needs. Folders in `writable` that do not exist are skipped.
///
/// Call it before any other thread starts, since threads already running
/// keep their old rights. Like the socket filter, it cannot be undone.
pub fn limit_writes(writable: &[&Path]) -> Result<Enforced, Error> {
    let access = write_access();
    let fail = |e: landlock::RulesetError| error("the kernel refused the file limits", e);
    let existing = writable.iter().filter(|p| p.exists());
    let status = Ruleset::default()
        .handle_access(access)
        .map_err(fail)?
        .handle_access(AccessNet::from_all(ABI::V5))
        .map_err(fail)?
        .create()
        .map_err(fail)?
        .add_rules(path_beneath_rules(existing, access))
        .map_err(fail)?
        .restrict_self()
        .map_err(fail)?;
    Ok(match status.ruleset {
        RulesetStatus::FullyEnforced => Enforced::Fully,
        RulesetStatus::PartiallyEnforced => Enforced::Partly,
        RulesetStatus::NotEnforced => Enforced::Not,
    })
}

/// Stops the calling thread, and every thread it starts, from reaching
/// anything outside the process, for work on untrusted bytes already in
/// memory, such as decoding a photo. Files cannot be opened, read,
/// written, or created; no socket can be made or used; and nothing can
/// be written but to standard output and error. Like the other limits it
/// cannot be undone, so call it only on a thread made for the work.
///
/// The thread still shares memory with the rest of the process, so this
/// does not hold against code able to take over another thread too; it
/// stops what a payload in a decoder would try first.
pub fn confine_thread() -> Result<Enforced, Error> {
    let fail = |e: landlock::RulesetError| error("the kernel refused the file limits", e);
    let status = Ruleset::default()
        .handle_access(AccessFs::from_all(ABI::V5))
        .map_err(fail)?
        .handle_access(AccessNet::from_all(ABI::V5))
        .map_err(fail)?
        .create()
        .map_err(fail)?
        .restrict_self()
        .map_err(fail)?;
    let program = confining_filter()?;
    seccompiler::apply_filter(&program).map_err(|e| error("the kernel refused the filter", e))?;
    Ok(match status.ruleset {
        RulesetStatus::FullyEnforced => Enforced::Fully,
        RulesetStatus::PartiallyEnforced => Enforced::Partly,
        RulesetStatus::NotEnforced => Enforced::Not,
    })
}

/// The confined thread's filter, which also holds where the kernel has
/// no Landlock: every way of opening a file or making, using, or passing
/// a socket is refused, and writing is allowed only to descriptors 0 to
/// 2, which the thread may not close or replace by the usual calls, so
/// an open socket, such as the one to the session bus, cannot simply be
/// written to either. It does not refuse everything: `close_range`,
/// `pidfd_getfd`, and writing through `mmap` or asynchronous I/O to a
/// descriptor opened earlier still work, and without Landlock, files can
/// still be removed or renamed by path.
fn confining_filter() -> Result<BpfProgram, Error> {
    let rule = |conditions: Vec<SeccompCondition>| -> Result<Vec<SeccompRule>, Error> {
        let rule = SeccompRule::new(conditions).map_err(|e| error("building the filter", e))?;
        Ok(vec![rule])
    };
    let arg = |index: u8, op: SeccompCmpOp, value: u64| {
        SeccompCondition::new(index, SeccompCmpArgLen::Dword, op, value)
            .map_err(|e| error("building the filter", e))
    };
    let not_standard = || rule(vec![arg(0, SeccompCmpOp::Gt, 2)?]);
    let standard = || rule(vec![arg(0, SeccompCmpOp::Le, 2)?]);
    let mut rules: BTreeMap<i64, Vec<SeccompRule>> = BTreeMap::new();
    // No conditions: always refused.
    for call in [
        libc::SYS_openat,
        libc::SYS_openat2,
        libc::SYS_open_by_handle_at,
        libc::SYS_socket,
        libc::SYS_socketpair,
        libc::SYS_connect,
        libc::SYS_bind,
        libc::SYS_listen,
        libc::SYS_accept,
        libc::SYS_accept4,
        libc::SYS_sendto,
        libc::SYS_sendmsg,
        libc::SYS_sendmmsg,
        libc::SYS_sendfile,
        libc::SYS_splice,
        libc::SYS_tee,
        libc::SYS_vmsplice,
        libc::SYS_copy_file_range,
        libc::SYS_io_uring_setup,
        libc::SYS_execve,
        libc::SYS_execveat,
        libc::SYS_dup,
        libc::SYS_dup3,
    ] {
        rules.insert(call, Vec::new());
    }
    #[cfg(target_arch = "x86_64")]
    for call in [libc::SYS_open, libc::SYS_creat, libc::SYS_dup2] {
        rules.insert(call, Vec::new());
    }
    for call in [
        libc::SYS_write,
        libc::SYS_writev,
        libc::SYS_pwrite64,
        libc::SYS_pwritev,
        libc::SYS_pwritev2,
    ] {
        rules.insert(call, not_standard()?);
    }
    rules.insert(libc::SYS_close, standard()?);
    // Duplicating a descriptor through fcntl, the one way left to put a
    // socket in place of standard output.
    rules.insert(
        libc::SYS_fcntl,
        vec![
            SeccompRule::new(vec![arg(1, SeccompCmpOp::Eq, libc::F_DUPFD as u64)?])
                .map_err(|e| error("building the filter", e))?,
            SeccompRule::new(vec![arg(
                1,
                SeccompCmpOp::Eq,
                libc::F_DUPFD_CLOEXEC as u64,
            )?])
            .map_err(|e| error("building the filter", e))?,
        ],
    );
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
