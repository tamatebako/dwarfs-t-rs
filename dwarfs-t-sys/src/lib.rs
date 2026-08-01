//! Raw FFI bindings to `libdwarfs_c`, the stable C ABI of DwarFS.
//!
//! These bindings mirror `dwarfs_c.h` from
//! [dwarfs-t](https://github.com/tamatebako/dwarfs-t) exactly. They are
//! **hand-written on purpose**: the C surface is small (16 reader + 6
//! writer functions, three POD structs, two enums) and is a deliberately
//! frozen ABI, so pulling in `bindgen` (and its libclang requirement)
//! would cost consumers more than it buys. Correctness of the Rust-side
//! declarations against the C header is enforced at build time by
//! `abi_check.c`, which contains `_Static_assert`s over every struct
//! offset, struct size, enum value and constant used below; a mismatch
//! fails the build.
//!
//! Everything in this crate is `unsafe` to call. You almost certainly want
//! the safe [`dwarfs-t`](https://docs.rs/dwarfs-t) wrapper instead.
//!
//! # Linking
//!
//! With the default `vendored` feature, `build.rs` compiles `libdwarfs_c`
//! and its entire static dependency closure from the vendored `dwarfs-t`
//! git submodule (CMake + vcpkg). See the repository README for the
//! environment knobs (`DWARFS_RS_VCPKG_ROOT`, `DWARFS_RS_VCPKG_TRIPLET`,
//! `DWARFS_RS_DWARFS_T_SOURCE`, ...). The crates.io package does not bundle
//! the dwarfs-t sources, so there `DWARFS_RS_DWARFS_T_SOURCE` is required.
//!
//! # Skeleton mode (`--no-default-features`)
//!
//! With `vendored` disabled, nothing native is built or linked and every
//! ABI entry point below is a Rust stub with the same signature: calls that
//! would fail report `ENOTSUP` through the thread-local error channel
//! ([`dwarfs_c_errno`]/[`dwarfs_c_error_message`]) and return the failure
//! value (NULL / -1); the `void` functions are no-ops. This lets dependent
//! crates compile and link without any native toolchain, surfacing
//! "operation not supported" at runtime instead. On docs.rs the native
//! build is skipped as well (`DOCS_RS` env), but the declarations stay
//! feature-complete because rustdoc never links.

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

/// The dwarfs_c ABI version these bindings pin (spec 18 C20): checked
/// against `dwarfs_c_abi_version()` at use; a mismatch is refused.
pub const DWARFS_C_ABI_VERSION: c_int = 1;

/// Opaque writer handle (`struct dwarfs_c_writer`).
///
/// Owned by the caller; release with [`dwarfs_c_writer_free`]. Not
/// thread-safe.
pub enum dwarfs_c_writer {}

/// `dwarfs_c_compression`: store blocks uncompressed ("null").
pub const DWARFS_C_COMPRESSION_NONE: c_int = 0;
/// `dwarfs_c_compression`: Zstandard (mkdwarfs default).
pub const DWARFS_C_COMPRESSION_ZSTD: c_int = 1;
/// `dwarfs_c_compression`: LZMA.
pub const DWARFS_C_COMPRESSION_LZMA: c_int = 2;
/// `dwarfs_c_compression`: Brotli.
pub const DWARFS_C_COMPRESSION_BROTLI: c_int = 3;

/// Current version of the [`dwarfs_c_writer_options`] layout.
pub const DWARFS_C_WRITER_OPTIONS_VERSION: u32 = 1;

/// Writer options (`struct dwarfs_c_writer_options`).
///
/// Always obtain defaults via [`dwarfs_c_writer_options_init`]
/// (struct_version-stamped); layout is pinned by `_Static_assert`s in
/// `abi_check.c`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct dwarfs_c_writer_options {
    /// `DWARFS_C_WRITER_OPTIONS_VERSION`.
    pub struct_version: u32,
    /// One of the `DWARFS_C_COMPRESSION_*` constants.
    pub compression: i32,
    /// Algorithm-native level; -1 = the mkdwarfs default per algorithm.
    pub compression_level: i32,
    /// log2 of the block size (10..30); 0 = mkdwarfs default (24).
    pub block_size_bits: u32,
    /// 0 = off (mkdwarfs default); 1 = the "pcmaudio" categorizer.
    pub enable_categorizer: i32,
    /// Worker threads for scanning and compression; 0 = one per CPU.
    pub num_workers: u32,
}

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

#[cfg(feature = "vendored")]
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

    /// The dwarfs_c ABI version (spec 18 C20): bumped on any
    /// ABI-breaking change; the bindings pin it and refuse mismatches.
    pub fn dwarfs_c_abi_version() -> c_int;

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

    /// Initialize a writer options struct to the mkdwarfs defaults profile
    /// and stamp its struct_version. NULL opts are ignored.
    pub fn dwarfs_c_writer_options_init(opts: *mut dwarfs_c_writer_options);

    /// Create a writer. NULL on error (EINVAL for a bad struct_version or
    /// out-of-range option values). NULL `opts` means "all defaults".
    pub fn dwarfs_c_writer_create(opts: *const dwarfs_c_writer_options) -> *mut dwarfs_c_writer;

    /// Add a whole directory tree to the image (the mkdwarfs -i equivalent):
    /// the directory's content lands at the image root. `image_prefix` must
    /// be NULL, "" or "/". 0 on success, -1 on error (EINVAL/ENOENT/ENOTDIR/
    /// EALREADY).
    pub fn dwarfs_c_writer_add_tree(
        w: *mut dwarfs_c_writer,
        host_path: *const c_char,
        image_prefix: *const c_char,
    ) -> c_int;

    /// Add a single file at the image root; `image_path` must equal
    /// basename(host_path) and all files must share one directory.
    /// 0 on success, -1 on error (EINVAL/ENOENT/EALREADY).
    pub fn dwarfs_c_writer_add_file(
        w: *mut dwarfs_c_writer,
        host_path: *const c_char,
        image_path: *const c_char,
    ) -> c_int;

    /// Write the image to `out_path` (must not exist; the writer never
    /// overwrites). This is where all scanning and compression happens.
    /// 0 on success, -1 on error (EINVAL/EEXIST/EIO).
    pub fn dwarfs_c_writer_write(w: *mut dwarfs_c_writer, out_path: *const c_char) -> c_int;

    /// Release a writer handle. Safe to call with NULL.
    pub fn dwarfs_c_writer_free(w: *mut dwarfs_c_writer);
}

// ---------------------------------------------------------------------------
// Skeleton mode (`--no-default-features`)
//
// No native library is built or linked (build.rs returns early), so the
// whole ABI surface is provided as Rust stubs with the exact same names and
// signatures. Every operation that can fail sets the thread-local error
// channel to ENOTSUP (with a fixed explanatory message) and returns the
// failure value of the real function (NULL / -1); the `void` functions are
// no-ops. The stubs are plain Rust functions — they introduce no foreign
// symbols, so they can never collide with a real libdwarfs_c linked by
// someone else.
// ---------------------------------------------------------------------------
#[cfg(not(feature = "vendored"))]
mod skeleton {
    use super::*;
    use core::cell::Cell;

    thread_local! {
        /// Thread-local errno channel, mirroring the real ABI's semantics.
        static LAST_ERRNO: Cell<c_int> = const { Cell::new(0) };
    }

    /// The one message every skeleton failure reports.
    static SKELETON_MSG: &[u8] = b"dwarfs-t-sys skeleton build: the `vendored` feature is disabled, no native library is linked\0";

    /// Version string reported by [`dwarfs_c_version_string`].
    static SKELETON_VERSION: &[u8] = b"0.0.0-skeleton\0";

    /// Record ENOTSUP in the thread-local channel and return it.
    fn fail_not_supported() -> c_int {
        LAST_ERRNO.with(|e| e.set(libc::ENOTSUP));
        libc::ENOTSUP
    }

    fn msg_ptr() -> *const c_char {
        SKELETON_MSG.as_ptr().cast()
    }

    pub unsafe fn dwarfs_c_errno() -> c_int {
        LAST_ERRNO.with(Cell::get)
    }

    pub unsafe fn dwarfs_c_error_message() -> *const c_char {
        msg_ptr()
    }

    pub unsafe fn dwarfs_c_strerror(_err: c_int) -> *const c_char {
        msg_ptr()
    }

    pub unsafe fn dwarfs_c_version() -> c_int {
        0
    }

    pub unsafe fn dwarfs_c_version_string() -> *const c_char {
        SKELETON_VERSION.as_ptr().cast()
    }

    pub unsafe fn dwarfs_c_abi_version() -> c_int {
        DWARFS_C_ABI_VERSION
    }

    pub unsafe fn dwarfs_c_open(_path: *const c_char) -> *mut dwarfs_c_filesystem {
        fail_not_supported();
        core::ptr::null_mut()
    }

    pub unsafe fn dwarfs_c_open_region(
        _path: *const c_char,
        _offset: i64,
        _length: i64,
    ) -> *mut dwarfs_c_filesystem {
        fail_not_supported();
        core::ptr::null_mut()
    }

    pub unsafe fn dwarfs_c_open_memory(
        _data: *const c_void,
        _size: usize,
    ) -> *mut dwarfs_c_filesystem {
        fail_not_supported();
        core::ptr::null_mut()
    }

    pub unsafe fn dwarfs_c_close(_fs: *mut dwarfs_c_filesystem) {}

    pub unsafe fn dwarfs_c_stat(
        _fs: *mut dwarfs_c_filesystem,
        _path: *const c_char,
        _st: *mut dwarfs_c_stat,
    ) -> c_int {
        fail_not_supported();
        -1
    }

    pub unsafe fn dwarfs_c_pread(
        _fs: *mut dwarfs_c_filesystem,
        _path: *const c_char,
        _buf: *mut c_void,
        _count: usize,
        _offset: i64,
    ) -> i64 {
        fail_not_supported();
        -1
    }

    pub unsafe fn dwarfs_c_opendir(
        _fs: *mut dwarfs_c_filesystem,
        _path: *const c_char,
    ) -> *mut dwarfs_c_dir {
        fail_not_supported();
        core::ptr::null_mut()
    }

    pub unsafe fn dwarfs_c_readdir(_dir: *mut dwarfs_c_dir, _out: *mut dwarfs_c_dirent) -> c_int {
        fail_not_supported();
        -1
    }

    pub unsafe fn dwarfs_c_closedir(_dir: *mut dwarfs_c_dir) {}

    pub unsafe fn dwarfs_c_image_info_json(_fs: *mut dwarfs_c_filesystem) -> *mut c_char {
        fail_not_supported();
        core::ptr::null_mut()
    }

    pub unsafe fn dwarfs_c_free(_ptr: *mut c_void) {}

    /// Pure data initialization — implemented for real, exactly like the
    /// native version (mkdwarfs defaults profile, version-stamped).
    pub unsafe fn dwarfs_c_writer_options_init(opts: *mut dwarfs_c_writer_options) {
        if let Some(opts) = opts.as_mut() {
            *opts = dwarfs_c_writer_options {
                struct_version: DWARFS_C_WRITER_OPTIONS_VERSION,
                compression: DWARFS_C_COMPRESSION_ZSTD,
                compression_level: -1,
                block_size_bits: 0,
                enable_categorizer: 0,
                num_workers: 0,
            };
        }
    }

    pub unsafe fn dwarfs_c_writer_create(
        _opts: *const dwarfs_c_writer_options,
    ) -> *mut dwarfs_c_writer {
        fail_not_supported();
        core::ptr::null_mut()
    }

    pub unsafe fn dwarfs_c_writer_add_tree(
        _w: *mut dwarfs_c_writer,
        _host_path: *const c_char,
        _image_prefix: *const c_char,
    ) -> c_int {
        fail_not_supported();
        -1
    }

    pub unsafe fn dwarfs_c_writer_add_file(
        _w: *mut dwarfs_c_writer,
        _host_path: *const c_char,
        _image_path: *const c_char,
    ) -> c_int {
        fail_not_supported();
        -1
    }

    pub unsafe fn dwarfs_c_writer_write(
        _w: *mut dwarfs_c_writer,
        _out_path: *const c_char,
    ) -> c_int {
        fail_not_supported();
        -1
    }

    pub unsafe fn dwarfs_c_writer_free(_w: *mut dwarfs_c_writer) {}
}

#[cfg(not(feature = "vendored"))]
pub use skeleton::*;
