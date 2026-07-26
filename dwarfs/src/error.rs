//! Error type for the safe wrapper, mapping the C ABI's errno channel.

use std::fmt;

/// Coarse classification of a [`DwarfsError`], mapped from the errno value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// `ENOENT` — the image file or the looked-up path does not exist.
    NotFound,
    /// `EISDIR` — attempted to read a directory as a file.
    IsADirectory,
    /// `ENOTDIR` — attempted to list a non-directory.
    NotADirectory,
    /// `EINVAL` — bad arguments (null/empty path, negative offset, ...).
    InvalidInput,
    /// `ENOMEM` — allocation failure inside the library.
    OutOfMemory,
    /// `EIO` — image parse/read failure (the message carries details).
    Io,
    /// `ENOTSUP` — the operation is not supported by this build; reported
    /// for every operation when the crate is compiled without the `vendored`
    /// feature (the pure-cargo skeleton links no native library).
    NotSupported,
    /// Any other errno value.
    Other(i32),
}

impl ErrorKind {
    pub(crate) fn from_errno(errno: i32) -> Self {
        match errno {
            libc::ENOENT => ErrorKind::NotFound,
            libc::EISDIR => ErrorKind::IsADirectory,
            libc::ENOTDIR => ErrorKind::NotADirectory,
            libc::EINVAL => ErrorKind::InvalidInput,
            libc::ENOMEM => ErrorKind::OutOfMemory,
            libc::EIO => ErrorKind::Io,
            libc::ENOTSUP => ErrorKind::NotSupported,
            other => ErrorKind::Other(other),
        }
    }
}

/// Error returned by all fallible operations in this crate.
///
/// Wraps the thread-local errno-style error channel of the C ABI:
/// the raw errno value and the library's human-readable message.
#[derive(Debug)]
pub struct DwarfsError {
    errno: i32,
    message: String,
}

impl DwarfsError {
    pub(crate) fn new(errno: i32, message: String) -> Self {
        DwarfsError { errno, message }
    }

    /// Capture the C ABI's thread-local error state after a failed call.
    pub(crate) fn from_last_error() -> Self {
        // SAFETY: both functions are always safe to call; the returned
        // pointers are borrowed and valid until the next FFI call on this
        // thread, which cannot interleave here.
        unsafe {
            let errno = dwarfs_t_sys::dwarfs_c_errno();
            let msg = dwarfs_t_sys::dwarfs_c_error_message();
            let message = if msg.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(msg).to_string_lossy().into_owned()
            };
            DwarfsError::new(errno, message)
        }
    }

    /// An argument-level error not originating from the C library
    /// (e.g. a path containing an interior NUL byte).
    pub(crate) fn invalid_input(message: impl Into<String>) -> Self {
        DwarfsError::new(libc::EINVAL, message.into())
    }

    /// The raw errno value reported by the C ABI.
    pub fn errno(&self) -> i32 {
        self.errno
    }

    /// The coarse error classification.
    pub fn kind(&self) -> ErrorKind {
        ErrorKind::from_errno(self.errno)
    }

    /// The library's human-readable message (may be empty).
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for DwarfsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.message.is_empty() {
            write!(f, "{} (errno {})", self.kind_str(), self.errno)
        } else {
            write!(f, "{} ({}: {})", self.kind_str(), self.errno, self.message)
        }
    }
}

impl DwarfsError {
    fn kind_str(&self) -> String {
        match self.kind() {
            ErrorKind::NotFound => "not found".into(),
            ErrorKind::IsADirectory => "is a directory".into(),
            ErrorKind::NotADirectory => "not a directory".into(),
            ErrorKind::InvalidInput => "invalid input".into(),
            ErrorKind::OutOfMemory => "out of memory".into(),
            ErrorKind::Io => "i/o error".into(),
            ErrorKind::NotSupported => "not supported".into(),
            ErrorKind::Other(e) => format!("error {e}"),
        }
    }
}

impl std::error::Error for DwarfsError {}
