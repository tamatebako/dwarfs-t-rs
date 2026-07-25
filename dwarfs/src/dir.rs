//! Directory iteration.

use std::ptr::NonNull;

use dwarfs_t_sys::{dwarfs_c_dir, dwarfs_c_dirent};

use crate::error::DwarfsError;
use crate::metadata::FileType;
use crate::Filesystem;

/// A single directory entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// Entry name (never `.` or `..`).
    pub name: String,
    /// Entry type.
    pub file_type: FileType,
}

/// Iterator over a directory's entries, created by
/// [`Filesystem::read_dir`](crate::Filesystem::read_dir).
///
/// Each item is a `Result`: an `Err` is yielded at most once, after which
/// the iterator is exhausted.
///
/// The iterator borrows the [`Filesystem`] it was created from; the
/// filesystem cannot be closed while an iterator is alive (enforced by the
/// borrow checker).
pub struct ReadDir<'a> {
    _fs: &'a Filesystem,
    dir: NonNull<dwarfs_c_dir>,
    finished: bool,
}

impl ReadDir<'_> {
    pub(crate) fn new(fs: &Filesystem, dir: NonNull<dwarfs_c_dir>) -> ReadDir<'_> {
        ReadDir {
            _fs: fs,
            dir,
            finished: false,
        }
    }
}

impl Iterator for ReadDir<'_> {
    type Item = Result<DirEntry, DwarfsError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        let mut out = dwarfs_c_dirent {
            name: std::ptr::null(),
            r#type: 0,
        };
        // SAFETY: `dir` is a live iterator handle; `out` points to valid
        // stack storage. The returned name pointer is borrowed from the
        // iterator and copied into an owned String before the next call.
        let rc = unsafe { dwarfs_t_sys::dwarfs_c_readdir(self.dir.as_ptr(), &mut out) };
        match rc {
            1 => {
                let name = if out.name.is_null() {
                    String::new()
                } else {
                    unsafe { std::ffi::CStr::from_ptr(out.name) }
                        .to_string_lossy()
                        .into_owned()
                };
                Some(Ok(DirEntry {
                    name,
                    file_type: FileType::from_raw(out.r#type),
                }))
            }
            0 => {
                self.finished = true;
                None
            }
            _ => {
                self.finished = true;
                Some(Err(DwarfsError::from_last_error()))
            }
        }
    }
}

impl Drop for ReadDir<'_> {
    fn drop(&mut self) {
        // SAFETY: `dir` is a live iterator handle we own.
        unsafe { dwarfs_t_sys::dwarfs_c_closedir(self.dir.as_ptr()) }
    }
}

impl std::fmt::Debug for ReadDir<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReadDir").finish_non_exhaustive()
    }
}

// The iterator owns its handle exclusively; advancing requires `&mut self`,
// and the C API only forbids sharing one iterator across threads.
unsafe impl Send for ReadDir<'_> {}
