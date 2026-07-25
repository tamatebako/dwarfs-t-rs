//! The [`Filesystem`] handle — an opened DwarFS image.

use std::ffi::{CStr, CString};
use std::path::Path;
use std::ptr::NonNull;

use dwarfs_sys::{dwarfs_c_filesystem, dwarfs_c_stat, DWARFS_C_OFFSET_AUTO};

use crate::dir::ReadDir;
use crate::error::DwarfsError;
use crate::metadata::Metadata;

/// Pass as `offset` to [`Filesystem::open_region`] to auto-detect the image
/// start inside the container file.
pub const OFFSET_AUTO: i64 = DWARFS_C_OFFSET_AUTO;

/// Convert a filesystem path to a `CString` for the C ABI.
fn path_cstring(path: &Path) -> Result<CString, DwarfsError> {
    CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| DwarfsError::invalid_input("path contains an interior NUL byte"))
}

/// Convert an in-image lookup path to a `CString` for the C ABI.
fn lookup_cstring(path: &str) -> Result<CString, DwarfsError> {
    CString::new(path).map_err(|_| DwarfsError::invalid_input("path contains an interior NUL byte"))
}

/// An opened DwarFS filesystem image.
///
/// Created by [`Filesystem::open`], [`Filesystem::open_memory`] or
/// [`Filesystem::open_region`]. All operations are read-only.
///
/// # Thread safety
///
/// Distinct `Filesystem` values may be used concurrently from multiple
/// threads, and concurrent `stat`/`pread`/`read_dir` calls on the *same*
/// `Filesystem` are safe: the underlying C++ reader is thread-safe for
/// reads and every directory iterator is independent state. A single
/// [`ReadDir`] iterator must not be shared (it advances through `&mut
/// self`, so the borrow checker enforces this).
pub struct Filesystem {
    handle: NonNull<dwarfs_c_filesystem>,
    /// Owned copy of the image for `open_memory` (the C ABI borrows the
    /// buffer; we copy it so the safe API carries no lifetime).
    _memory: Option<Vec<u8>>,
}

// SAFETY (Sync): per the C ABI contract, concurrent stat/pread/opendir
// calls on the same handle are safe; no API here mutates shared state
// through `&self` other than the reader's internal, thread-safe caches.
// SAFETY (Send): a handle may be moved to another thread freely.
unsafe impl Send for Filesystem {}
unsafe impl Sync for Filesystem {}

impl Filesystem {
    fn from_raw(
        handle: *mut dwarfs_c_filesystem,
        memory: Option<Vec<u8>>,
    ) -> Result<Self, DwarfsError> {
        match NonNull::new(handle) {
            Some(handle) => Ok(Filesystem {
                handle,
                _memory: memory,
            }),
            None => Err(DwarfsError::from_last_error()),
        }
    }

    /// Open a DwarFS image from a file.
    ///
    /// ```no_run
    /// let fs = dwarfs::Filesystem::open("image.dwarfs")?;
    /// # Ok::<(), dwarfs::DwarfsError>(())
    /// ```
    ///
    /// # Errors
    /// - [`ErrorKind::NotFound`](crate::ErrorKind::NotFound) if the file does
    ///   not exist
    /// - [`ErrorKind::Io`](crate::ErrorKind::Io) if the image cannot be parsed
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DwarfsError> {
        let path = path_cstring(path.as_ref())?;
        // SAFETY: path is a valid NUL-terminated string.
        let handle = unsafe { dwarfs_sys::dwarfs_c_open(path.as_ptr()) };
        Self::from_raw(handle, None)
    }

    /// Open a DwarFS image from a region of a file — for images embedded at
    /// an offset inside a larger file (e.g. self-extracting stubs).
    ///
    /// `offset` is a byte offset, or [`OFFSET_AUTO`] to auto-detect the
    /// image start. `length` is the image length in bytes and must be > 0.
    pub fn open_region(
        path: impl AsRef<Path>,
        offset: i64,
        length: u64,
    ) -> Result<Self, DwarfsError> {
        if length == 0 {
            return Err(DwarfsError::invalid_input("length must be positive"));
        }
        if offset < 0 && offset != OFFSET_AUTO {
            return Err(DwarfsError::invalid_input(
                "offset must be >= 0 or OFFSET_AUTO",
            ));
        }
        let path = path_cstring(path.as_ref())?;
        // SAFETY: path is a valid NUL-terminated string; length fits i64
        // because we clamp it.
        let length = i64::try_from(length)
            .map_err(|_| DwarfsError::invalid_input("length does not fit into i64"))?;
        let handle = unsafe { dwarfs_sys::dwarfs_c_open_region(path.as_ptr(), offset, length) };
        Self::from_raw(handle, None)
    }

    /// Open a DwarFS image from a memory buffer.
    ///
    /// The buffer is **copied**; the caller's slice may be dropped
    /// immediately. (The underlying C ABI borrows the buffer, so copying is
    /// what makes this safe API lifetime-free.)
    pub fn open_memory(data: &[u8]) -> Result<Self, DwarfsError> {
        if data.is_empty() {
            return Err(DwarfsError::invalid_input("data must not be empty"));
        }
        let mut owned = data.to_vec();
        // SAFETY: `owned` outlives the handle (it is stored in the
        // Filesystem and dropped after close); the C ABI only borrows it.
        let handle =
            unsafe { dwarfs_sys::dwarfs_c_open_memory(owned.as_ptr().cast(), owned.len()) };
        // Ensure the copy survives even on failure: it is dropped here if
        // from_raw errors, which is fine because the handle is NULL then.
        owned.shrink_to_fit();
        Self::from_raw(handle, Some(owned))
    }

    /// Look up an entry by path and return its stat-equivalent metadata.
    ///
    /// Paths are relative to the filesystem root; a leading `/` is accepted
    /// and ignored. `""` or `"/"` denote the root directory. Lookup never
    /// resolves the final path component if it is a symlink (lstat
    /// semantics).
    ///
    /// # Errors
    /// [`ErrorKind::NotFound`](crate::ErrorKind::NotFound) if the path does
    /// not exist.
    pub fn stat(&self, path: &str) -> Result<Metadata, DwarfsError> {
        let path = lookup_cstring(path)?;
        let mut raw = dwarfs_c_stat {
            size: 0,
            mtime: 0,
            mtime_nsec: 0,
            mode: 0,
            uid: 0,
            gid: 0,
            nlink: 0,
            r#type: 0,
        };
        // SAFETY: handle is live; path is a valid C string; raw points to
        // valid stack storage.
        let rc =
            unsafe { dwarfs_sys::dwarfs_c_stat(self.handle.as_ptr(), path.as_ptr(), &mut raw) };
        if rc != 0 {
            return Err(DwarfsError::from_last_error());
        }
        Ok(Metadata::from_raw(&raw))
    }

    /// Read from a regular file at a given offset (pread primitive).
    ///
    /// Reads are clamped to the end of the file; reading at or past the end
    /// yields `Ok(0)`.
    ///
    /// # Errors
    /// - [`ErrorKind::NotFound`](crate::ErrorKind::NotFound) if the path does
    ///   not exist
    /// - [`ErrorKind::IsADirectory`](crate::ErrorKind::IsADirectory) for a
    ///   directory
    /// - [`ErrorKind::InvalidInput`](crate::ErrorKind::InvalidInput) for
    ///   other non-regular entries or a negative offset
    /// - [`ErrorKind::Io`](crate::ErrorKind::Io) on read failure
    pub fn pread(&self, path: &str, buf: &mut [u8], offset: i64) -> Result<usize, DwarfsError> {
        if offset < 0 {
            return Err(DwarfsError::invalid_input("offset must not be negative"));
        }
        let path = lookup_cstring(path)?;
        // SAFETY: handle is live; path is a valid C string; buf is valid
        // for writes of buf.len() bytes.
        let n = unsafe {
            dwarfs_sys::dwarfs_c_pread(
                self.handle.as_ptr(),
                path.as_ptr(),
                buf.as_mut_ptr().cast(),
                buf.len(),
                offset,
            )
        };
        if n < 0 {
            return Err(DwarfsError::from_last_error());
        }
        Ok(n as usize)
    }

    /// Open a directory for iteration.
    ///
    /// The entries `.` and `..` are never yielded.
    ///
    /// # Errors
    /// - [`ErrorKind::NotFound`](crate::ErrorKind::NotFound) if the path does
    ///   not exist
    /// - [`ErrorKind::NotADirectory`](crate::ErrorKind::NotADirectory) if it
    ///   is not a directory
    pub fn read_dir(&self, path: &str) -> Result<ReadDir<'_>, DwarfsError> {
        let path = lookup_cstring(path)?;
        // SAFETY: handle is live; path is a valid C string.
        let dir = unsafe { dwarfs_sys::dwarfs_c_opendir(self.handle.as_ptr(), path.as_ptr()) };
        match NonNull::new(dir) {
            Some(dir) => Ok(ReadDir::new(self, dir)),
            None => Err(DwarfsError::from_last_error()),
        }
    }

    /// Image-level metadata as a JSON string: image format version, image
    /// offset, creation history (timestamps, mkdwarfs version, system,
    /// command line), metadata summary (inode/directory/chunk counts, block
    /// size, total size) and the section list.
    pub fn image_info_json(&self) -> Result<String, DwarfsError> {
        // SAFETY: handle is live. The returned pointer is heap-allocated by
        // the library and must be released with dwarfs_c_free.
        let raw = unsafe { dwarfs_sys::dwarfs_c_image_info_json(self.handle.as_ptr()) };
        if raw.is_null() {
            return Err(DwarfsError::from_last_error());
        }
        // Copy out, then release the library-owned allocation.
        let json = unsafe { CStr::from_ptr(raw) }
            .to_string_lossy()
            .into_owned();
        // SAFETY: raw was returned by dwarfs_c_image_info_json.
        unsafe { dwarfs_sys::dwarfs_c_free(raw.cast()) };
        Ok(json)
    }

    /// The library version as a single integer:
    /// `major * 10000 + minor * 100 + patch`.
    pub fn version() -> i32 {
        // SAFETY: always safe to call.
        unsafe { dwarfs_sys::dwarfs_c_version() }
    }

    /// The library version string (e.g. the git description).
    pub fn version_string() -> &'static str {
        // SAFETY: the returned pointer refers to a static C string.
        let ptr = unsafe { dwarfs_sys::dwarfs_c_version_string() };
        if ptr.is_null() {
            return "unknown";
        }
        unsafe { CStr::from_ptr(ptr) }.to_str().unwrap_or("unknown")
    }
}

impl Drop for Filesystem {
    fn drop(&mut self) {
        // SAFETY: handle is live and owned by us; directory iterators borrow
        // the Filesystem, so none can outlive it.
        unsafe { dwarfs_sys::dwarfs_c_close(self.handle.as_ptr()) }
    }
}

impl std::fmt::Debug for Filesystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Filesystem").finish_non_exhaustive()
    }
}
