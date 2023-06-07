//! A basic implementation of `git blame`-like functionality.
//!
//! This crate provides `git blame`-like functionality. It does
//! not use Git (or libgit2 or ...) under the hood but rather [gitoxide]
//! which is therefore also a build dependency.
//!
//! Currently it is very simple and is lacking in features. It does not handle
//! renamed files and presumably does poorly with parallel histories. It assumes
//! all revisions of the blamed file can be interpreted as UTF-8. The only
//! interface to this crate's functionality is [`blame_file`].
//!
//! [gitoxide]: https://github.com/Byron/gitoxide

mod blame;
pub use blame::*;

mod error;
pub use error::*;

mod collector;
