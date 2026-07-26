//! The [`Writer`] — in-process creation of DwarFS images.
//!
//! No `mkdwarfs` subprocess, no shell, no PATH dependency: the image is
//! produced by the same stable C ABI the reader uses. Single-shot
//! discipline: create a `Writer`, add content, then [`Writer::write`]
//! (which consumes the writer) — dropping a `Writer` always releases the
//! native handle.
//!
//! ```no_run
//! use dwarfs_t::{Compression, Writer, WriterOptions};
//!
//! # fn main() -> Result<(), dwarfs_t::DwarfsError> {
//! let mut w = Writer::new(WriterOptions::default())?;
//! w.add_tree("app/", "/")?;
//! w.write("app.dwarfs")?; // consumes w
//!
//! // ...and read it straight back:
//! let fs = dwarfs_t::Filesystem::open("app.dwarfs")?;
//! assert!(fs.stat("hello.txt").is_ok());
//! # Ok(())
//! # }
//! ```
//!
//! # v1 source rules (enforced by the C ABI, surfaced as errors)
//!
//! - The writer is single-source: exactly one [`Writer::add_tree`] XOR one
//!   or more [`Writer::add_file`] calls; mixing the two fails.
//! - `add_tree` accepts only `""` or `"/"` as the image prefix (the tree
//!   lands at the image root; arbitrary prefixes/renames are a v1
//!   limitation of the underlying scanner).
//! - `add_file` places files at the image root by basename: `image_path`
//!   must equal the host file's basename and all files must live in the
//!   same directory.
//! - [`Writer::write`] never overwrites: the output path must not exist
//!   (errno `EEXIST`), and a writer cannot be written twice
//!   (`EALREADY`).
//!
//! # Determinism
//!
//! Output bytes are **not** run-to-run deterministic: the image history
//! records creation timestamps. `num_workers` affects only throughput,
//! never the image layout.

use std::ffi::CString;
use std::path::Path;
use std::ptr::NonNull;

use dwarfs_t_sys::{
    dwarfs_c_writer, dwarfs_c_writer_options, DWARFS_C_COMPRESSION_BROTLI,
    DWARFS_C_COMPRESSION_LZMA, DWARFS_C_COMPRESSION_NONE, DWARFS_C_COMPRESSION_ZSTD,
    DWARFS_C_WRITER_OPTIONS_VERSION,
};

use crate::error::DwarfsError;
use crate::path_cstring;

/// Block compression algorithm for [`WriterOptions`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Compression {
    /// Store blocks uncompressed (`"null"`).
    None,
    /// Zstandard (the mkdwarfs default).
    #[default]
    Zstd,
    /// LZMA.
    Lzma,
    /// Brotli.
    Brotli,
}

impl Compression {
    fn to_raw(self) -> i32 {
        match self {
            Compression::None => DWARFS_C_COMPRESSION_NONE,
            Compression::Zstd => DWARFS_C_COMPRESSION_ZSTD,
            Compression::Lzma => DWARFS_C_COMPRESSION_LZMA,
            Compression::Brotli => DWARFS_C_COMPRESSION_BROTLI,
        }
    }
}

/// Options for [`Writer::new`]; `Default` reproduces the mkdwarfs defaults
/// profile (zstd block compression at the default level, 16 MiB blocks,
/// similarity ordering, categorizer off, one worker per CPU).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WriterOptions {
    /// Block compression algorithm.
    pub compression: Compression,
    /// Algorithm-native compression level; `None` uses the mkdwarfs default
    /// for the chosen algorithm (zstd 22, lzma 9, brotli 11). Ignored for
    /// [`Compression::None`].
    pub compression_level: Option<i32>,
    /// log2 of the block size (10..=30); `None` uses the mkdwarfs default
    /// (24, i.e. 16 MiB).
    pub block_size_bits: Option<u32>,
    /// Enable the `"pcmaudio"` categorizer (off by default, as in mkdwarfs).
    pub enable_categorizer: bool,
    /// Worker threads for scanning and compression; 0 = one per CPU.
    pub num_workers: u32,
}

impl WriterOptions {
    /// Options with a specific compression algorithm and level (other
    /// fields at their mkdwarfs defaults).
    pub fn with_compression(compression: Compression, level: Option<i32>) -> Self {
        WriterOptions {
            compression,
            compression_level: level,
            ..Default::default()
        }
    }

    fn to_raw(self) -> dwarfs_c_writer_options {
        dwarfs_c_writer_options {
            struct_version: DWARFS_C_WRITER_OPTIONS_VERSION,
            compression: self.compression.to_raw(),
            compression_level: self.compression_level.unwrap_or(-1),
            block_size_bits: self.block_size_bits.unwrap_or(0),
            enable_categorizer: i32::from(self.enable_categorizer),
            num_workers: self.num_workers,
        }
    }
}

/// An in-process DwarFS image writer (single-shot; see the module docs).
///
/// Not thread-safe: do not share one `Writer` between threads without
/// external synchronization.
pub struct Writer {
    handle: NonNull<dwarfs_c_writer>,
}

impl Writer {
    fn from_raw(raw: &dwarfs_c_writer_options) -> Result<Self, DwarfsError> {
        // SAFETY: raw points to a fully initialized options struct whose
        // struct_version we stamped ourselves.
        let handle = unsafe { dwarfs_t_sys::dwarfs_c_writer_create(raw) };
        match NonNull::new(handle) {
            Some(handle) => Ok(Writer { handle }),
            None => Err(DwarfsError::from_last_error()),
        }
    }

    /// Create a writer from explicit options.
    ///
    /// # Errors
    /// [`ErrorKind::InvalidInput`](crate::ErrorKind::InvalidInput) for
    /// out-of-range option values.
    pub fn new(options: WriterOptions) -> Result<Self, DwarfsError> {
        Self::from_raw(&options.to_raw())
    }

    /// Create a writer with a specific compression algorithm and level
    /// (everything else at the mkdwarfs defaults).
    pub fn with_compression(
        compression: Compression,
        level: Option<i32>,
    ) -> Result<Self, DwarfsError> {
        Self::new(WriterOptions::with_compression(compression, level))
    }

    /// Add a whole directory tree to the image (the mkdwarfs `-i <dir>`
    /// equivalent): the directory's content lands at the image root.
    ///
    /// `image_prefix` must be `""` or `"/"` in v1.
    ///
    /// # Errors
    /// - [`ErrorKind::NotFound`](crate::ErrorKind::NotFound) if `host_path`
    ///   does not exist
    /// - [`ErrorKind::NotADirectory`](crate::ErrorKind::NotADirectory) if it
    ///   is not a directory
    /// - [`ErrorKind::InvalidInput`](crate::ErrorKind::InvalidInput) for any
    ///   other `image_prefix`
    /// - `EALREADY` (via [`crate::ErrorKind::Other`]) if a source was already added
    pub fn add_tree(
        &mut self,
        host_path: impl AsRef<Path>,
        image_prefix: &str,
    ) -> Result<(), DwarfsError> {
        let host = path_cstring(host_path.as_ref())?;
        let prefix = CString::new(image_prefix).map_err(|_| {
            DwarfsError::invalid_input("image_prefix contains an interior NUL byte")
        })?;
        // SAFETY: handle is live; both strings are valid C strings.
        let rc = unsafe {
            dwarfs_t_sys::dwarfs_c_writer_add_tree(
                self.handle.as_ptr(),
                host.as_ptr(),
                prefix.as_ptr(),
            )
        };
        if rc != 0 {
            return Err(DwarfsError::from_last_error());
        }
        Ok(())
    }

    /// Add a single file at the image root. `image_path` must equal
    /// basename(host_path); all files added to one writer must live in the
    /// same directory.
    ///
    /// # Errors
    /// - [`ErrorKind::NotFound`](crate::ErrorKind::NotFound) if `host_path`
    ///   does not exist
    /// - [`ErrorKind::InvalidInput`](crate::ErrorKind::InvalidInput) for a
    ///   rename or a second directory
    /// - `EALREADY` (via [`crate::ErrorKind::Other`]) if a tree source was added
    pub fn add_file(
        &mut self,
        host_path: impl AsRef<Path>,
        image_path: &str,
    ) -> Result<(), DwarfsError> {
        let host = path_cstring(host_path.as_ref())?;
        let image = CString::new(image_path)
            .map_err(|_| DwarfsError::invalid_input("image_path contains an interior NUL byte"))?;
        // SAFETY: handle is live; both strings are valid C strings.
        let rc = unsafe {
            dwarfs_t_sys::dwarfs_c_writer_add_file(
                self.handle.as_ptr(),
                host.as_ptr(),
                image.as_ptr(),
            )
        };
        if rc != 0 {
            return Err(DwarfsError::from_last_error());
        }
        Ok(())
    }

    /// Write the image to `out_path`, consuming the writer. This is where
    /// all scanning and compression happens.
    ///
    /// The output file must not exist (the writer never overwrites).
    ///
    /// # Errors
    /// - [`ErrorKind::InvalidInput`](crate::ErrorKind::InvalidInput) if no
    ///   source was added
    /// - `EEXIST` (via [`crate::ErrorKind::Other`]) if `out_path` exists
    /// - [`ErrorKind::Io`](crate::ErrorKind::Io) on scan/compress/write
    ///   failure (the message carries details)
    pub fn write(self, out_path: impl AsRef<Path>) -> Result<(), DwarfsError> {
        let out = path_cstring(out_path.as_ref())?;
        // SAFETY: handle is live; out is a valid C string. The handle is
        // released by Drop after this call regardless of the outcome.
        let rc = unsafe { dwarfs_t_sys::dwarfs_c_writer_write(self.handle.as_ptr(), out.as_ptr()) };
        if rc != 0 {
            return Err(DwarfsError::from_last_error());
        }
        Ok(())
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        // SAFETY: handle is live and owned by us.
        unsafe { dwarfs_t_sys::dwarfs_c_writer_free(self.handle.as_ptr()) }
    }
}

impl std::fmt::Debug for Writer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Writer").finish_non_exhaustive()
    }
}
