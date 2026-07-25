//! Stat-equivalent metadata and file type classification.

use dwarfs_sys::{
    dwarfs_c_stat, DWARFS_C_FILE_DIRECTORY, DWARFS_C_FILE_OTHER, DWARFS_C_FILE_REGULAR,
    DWARFS_C_FILE_SYMLINK, DWARFS_C_FILE_UNKNOWN,
};

/// File type classification (mirrors `dwarfs_c_file_type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileType {
    /// Type could not be determined.
    Unknown,
    /// Regular file.
    Regular,
    /// Directory.
    Directory,
    /// Symbolic link.
    Symlink,
    /// Device, fifo, socket, ...
    Other,
}

impl FileType {
    pub(crate) fn from_raw(raw: i32) -> Self {
        match raw as _ {
            DWARFS_C_FILE_REGULAR => FileType::Regular,
            DWARFS_C_FILE_DIRECTORY => FileType::Directory,
            DWARFS_C_FILE_SYMLINK => FileType::Symlink,
            DWARFS_C_FILE_OTHER => FileType::Other,
            DWARFS_C_FILE_UNKNOWN => FileType::Unknown,
            _ => FileType::Unknown,
        }
    }
}

/// Stat-equivalent information for a filesystem entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    /// File size in bytes (regular files).
    pub size: u64,
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
    /// File type.
    pub file_type: FileType,
}

impl Metadata {
    pub(crate) fn from_raw(raw: &dwarfs_c_stat) -> Self {
        Metadata {
            size: raw.size.max(0) as u64,
            mtime: raw.mtime,
            mtime_nsec: raw.mtime_nsec,
            mode: raw.mode,
            uid: raw.uid,
            gid: raw.gid,
            nlink: raw.nlink,
            file_type: FileType::from_raw(raw.r#type),
        }
    }

    /// True for a regular file.
    pub fn is_file(&self) -> bool {
        self.file_type == FileType::Regular
    }

    /// True for a directory.
    pub fn is_dir(&self) -> bool {
        self.file_type == FileType::Directory
    }

    /// True for a symbolic link.
    pub fn is_symlink(&self) -> bool {
        self.file_type == FileType::Symlink
    }
}
