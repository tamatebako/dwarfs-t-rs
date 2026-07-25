//! Raw FFI bindings to `libdwarfs_c`, the stable C ABI of the DwarFS reader.
//!
//! These bindings mirror `dwarfs_c.h` from
//! [dwarfs-t](https://github.com/tamatebako/dwarfs-t) exactly. They are
//! **hand-written on purpose**: the C surface is small (16 functions, two
//! POD structs, one enum) and is a deliberately frozen ABI, so pulling in
//! `bindgen` (and its libclang requirement) would cost consumers more than
//! it buys. Correctness of the Rust-side declarations against the C header
//! is enforced at build time by `abi_check.c`, which contains
//! `_Static_assert`s over every struct offset, struct size, enum value and
//! constant used below; a mismatch fails the build.
//!
//! Everything in this crate is `unsafe` to call. You almost certainly want
//! the safe [`dwarfs`](https://docs.rs/dwarfs) wrapper instead.
//!
//! # Linking
//!
//! With the default `vendored` feature, `build.rs` compiles `libdwarfs_c`
//! and its entire static dependency closure from the vendored `dwarfs-t`
//! git submodule (CMake + vcpkg). See the repository README for the
//! environment knobs (`DWARFS_RS_VCPKG_ROOT`, `DWARFS_RS_VCPKG_TRIPLET`,
//! `DWARFS_RS_DWARFS_T_SOURCE`, ...).

#![allow(non_camel_case_types)]
#![allow(clippy::missing_safety_doc)]

use core::ffi::{c_char, c_int, c_void};

/// Opaque filesystem handle (`struct dwarfs_c_filesystem`).
///
/// Owned by the caller; release with [`dwarfs_c_close`].
pub enum dwarfs_c_filesystem {}

/// Opaque directory iterator handle (`struct dwarfs_c_dir`).
///
/// Owned by the caller; release with [`dwarfs_c_closedir`].
pub enum dwarfs_c_dir {}

/// `dwarfs_c_file_type`: type could not be determined.
pub const DWARFS_C_FILE_UNKNOWN: c_int = 0;
/// `dwarfs_c_file_type`: regular file.
pub const DWARFS_C_FILE_REGULAR: c_int = 1;
/// `dwarfs_c_file_type`: directory.
pub const DWARFS_C_FILE_DIRECTORY: c_int = 2;
/// `dwarfs_c_file_type`: symbolic link.
pub const DWARFS_C_FILE_SYMLINK: c_int = 3;
/// `dwarfs_c_file_type`: device, fifo, socket, ...
pub const DWARFS_C_FILE_OTHER: c_int = 4;

/// Pass as offset to [`dwarfs_c_open_region`] to auto-detect the image start.
pub const DWARFS_C_OFFSET_AUTO: i64 = -1;

/// Stat-equivalent information for a filesystem entry (`struct dwarfs_c_stat`).
///
/// Layout is pinned by `_Static_assert`s in `abi_check.c`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct dwarfs_c_stat {
    /// File size in bytes (regular files).
    pub size: i64,
    /// Modification time, seconds since the epoch.
    pub mtime: i64,
    /// Modification time, sub-second nanoseconds.
    pub mtime_nsec: i32,
    /// POSIX `st_mode` value (type and permission bits).
    pub mode: u32,
    /// Owner user id.
    pub uid: u32,
    /// Owner group id.
    pub gid: u32,
    /// Number of hard links.
    pub nlink: u32,
    /// One of the `DWARFS_C_FILE_*` constants.
    pub r#type: i32,
}

/// Directory entry produced by [`dwarfs_c_readdir`] (`struct dwarfs_c_dirent`).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct dwarfs_c_dirent {
    /// Entry name; owned by the iterator, valid until the next
    /// `dwarfs_c_readdir`/`dwarfs_c_closedir` on that iterator.
    pub name: *const c_char,
    /// One of the `DWARFS_C_FILE_*` constants.
    pub r#type: i32,
}

extern "C" {
    /// Thread-local error code of the last failed call on this thread
    /// (0 if the last call succeeded). Values are `errno.h` codes.
    pub fn dwarfs_c_errno() -> c_int;

    /// Thread-local, human-readable message of the last failed call.
    /// Never NULL. Borrowed; valid until the next call on this thread.
    pub fn dwarfs_c_error_message() -> *const c_char;

    /// Static, borrowed string describing an errno-style error code.
    pub fn dwarfs_c_strerror(err: c_int) -> *const c_char;

    /// Library version as major * 10000 + minor * 100 + patch.
    pub fn dwarfs_c_version() -> c_int;

    /// Borrowed, static version string (e.g. the git description).
    pub fn dwarfs_c_version_string() -> *const c_char;

    /// Open a DwarFS image from a file. NULL on error.
    pub fn dwarfs_c_open(path: *const c_char) -> *mut dwarfs_c_filesystem;

    /// Open a DwarFS image from a region of a file. NULL on error.
    pub fn dwarfs_c_open_region(
        path: *const c_char,
        offset: i64,
        length: i64,
    ) -> *mut dwarfs_c_filesystem;

    /// Open a DwarFS image from a memory buffer. The buffer is borrowed,
    /// NOT copied, and must remain valid until `dwarfs_c_close`.
    /// NULL on error.
    pub fn dwarfs_c_open_memory(data: *const c_void, size: usize) -> *mut dwarfs_c_filesystem;

    /// Close a filesystem handle. Safe to call with NULL.
    pub fn dwarfs_c_close(fs: *mut dwarfs_c_filesystem);

    /// Look up an entry by path (lstat semantics; leading `/` accepted).
    /// 0 on success, -1 on error (`ENOENT` if missing).
    pub fn dwarfs_c_stat(
        fs: *mut dwarfs_c_filesystem,
        path: *const c_char,
        st: *mut dwarfs_c_stat,
    ) -> c_int;

    /// Read from a regular file at a given offset (pread primitive).
    /// Returns the number of bytes read (>= 0) or -1 on error.
    pub fn dwarfs_c_pread(
        fs: *mut dwarfs_c_filesystem,
        path: *const c_char,
        buf: *mut c_void,
        count: usize,
        offset: i64,
    ) -> i64;

    /// Open a directory for iteration. NULL on error (`ENOENT`/`ENOTDIR`).
    pub fn dwarfs_c_opendir(fs: *mut dwarfs_c_filesystem, path: *const c_char)
        -> *mut dwarfs_c_dir;

    /// Fetch the next directory entry (`.`/`..` are never returned).
    /// 1 = entry returned, 0 = end of directory, -1 = error.
    pub fn dwarfs_c_readdir(dir: *mut dwarfs_c_dir, out: *mut dwarfs_c_dirent) -> c_int;

    /// Close a directory iterator. Safe to call with NULL.
    pub fn dwarfs_c_closedir(dir: *mut dwarfs_c_dir);

    /// Image-level metadata as a heap-allocated JSON string (release with
    /// [`dwarfs_c_free`]). NULL on error.
    pub fn dwarfs_c_image_info_json(fs: *mut dwarfs_c_filesystem) -> *mut c_char;

    /// Free a pointer returned by this library.
    pub fn dwarfs_c_free(ptr: *mut c_void);
}
