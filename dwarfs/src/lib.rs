//! Safe Rust bindings to [DwarFS](https://github.com/tamatebako/dwarfs-t),
//! a fast high-compression read-only file system.
//!
//! This crate wraps the stable C ABI (`libdwarfs_c`) of the DwarFS reader
//! via the `dwarfs-t-sys` FFI crate. It exposes a small, idiomatic,
//! read-only API:
//!
//! - open an image from a file, from memory, or from a file region
//!   (offset + length, e.g. images embedded in self-extracting stubs)
//! - look up entries and read their metadata ([`Filesystem::stat`])
//! - read file contents at an offset ([`Filesystem::pread`])
//! - iterate directories ([`Filesystem::read_dir`])
//! - query image-level metadata as JSON ([`Filesystem::image_info_json`])
//! - create images fully in-process ([`Writer`]) — no `mkdwarfs`
//!   subprocess, no shell, no PATH dependency
//!
//! ```no_run
//! use dwarfs_t::{Filesystem, FileType};
//!
//! # fn main() -> Result<(), dwarfs_t::DwarfsError> {
//! let fs = Filesystem::open("image.dwarfs")?;
//!
//! let meta = fs.stat("format.sh")?;
//! assert_eq!(meta.file_type, FileType::Regular);
//!
//! let mut buf = vec![0u8; meta.size as usize];
//! let n = fs.pread("format.sh", &mut buf, 0)?;
//! buf.truncate(n);
//!
//! for entry in fs.read_dir("/")? {
//!     let entry = entry?;
//!     println!("{:?} {}", entry.file_type, entry.name);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Feature flags
//!
//! - `vendored` *(default)* — build and statically link the native
//!   `libdwarfs_c` (plus its dependency closure) from dwarfs-t sources via
//!   CMake/vcpkg. In a git checkout the sources are the pinned `dwarfs-t`
//!   submodule; when building the crates.io package, point
//!   `DWARFS_RS_DWARFS_T_SOURCE` at a dwarfs-t checkout (the sources are
//!   not bundled). Requires CMake, a C++20 compiler and vcpkg
//!   (`DWARFS_RS_VCPKG_ROOT`/`VCPKG_ROOT`).
//! - *no default features* — the pure-cargo **skeleton**: nothing native is
//!   built or linked, so the crate always compiles; every operation then
//!   fails at runtime with `ENOTSUP` ([`ErrorKind::NotSupported`]). This
//!   exists for consumers that gate the DwarFS backend behind their own
//!   optional feature and must build either way.
//!
//! # Errors
//!
//! All fallible operations return [`DwarfsError`], which carries the raw
//! errno value from the C ABI's thread-local error channel plus the
//! library's message; [`DwarfsError::kind`] maps it to an
//! [`ErrorKind`] classification.
//!
//! # License note
//!
//! The Rust sources of this crate are `MIT OR Apache-2.0`. The native
//! library linked through `dwarfs-t-sys` (dwarfs-t / DwarFS) is **GPL-3.0**:
//! binaries that statically link it are subject to GPL-3.0 terms. See the
//! repository README for details.

mod dir;
mod error;
mod fs;
mod metadata;
mod writer;

pub use dir::{DirEntry, ReadDir};
pub use error::{DwarfsError, ErrorKind};
pub use fs::{Filesystem, OFFSET_AUTO};
pub use metadata::{FileType, Metadata};
pub use writer::{Compression, Writer, WriterOptions};

/// Convert a filesystem path to a `CString` for the C ABI.
pub(crate) fn path_cstring(path: &std::path::Path) -> Result<std::ffi::CString, DwarfsError> {
    std::ffi::CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| DwarfsError::invalid_input("path contains an interior NUL byte"))
}
