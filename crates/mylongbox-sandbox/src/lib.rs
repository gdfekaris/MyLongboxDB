//! Runtime self-restrictions applied at startup: a seccomp filter that
//! blocks internet sockets, and Landlock rules that limit file access.
//!
//! This is the only crate in the workspace that may use `unsafe`. Every
//! `unsafe` block must carry a `SAFETY:` comment.

#![deny(unsafe_op_in_unsafe_fn, clippy::undocumented_unsafe_blocks)]
