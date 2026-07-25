/*
 * ABI cross-check for the hand-written Rust FFI declarations in
 * dwarfs-sys/src/lib.rs against the real dwarfs_c.h.
 *
 * Every struct offset/size, enum value and constant the Rust side relies on
 * is asserted here; a drift on the C side fails the build. The matching
 * `const _: () = assert!(...)` checks on the Rust side pin the same values.
 *
 * Compiled by dwarfs-sys/build.rs with the vendored dwarfs-t include dir.
 */

#include <dwarfs_c.h>

#include <stddef.h>

/* struct dwarfs_c_stat layout (pinned in Rust as #[repr(C)] dwarfs_c_stat) */
_Static_assert(sizeof(struct dwarfs_c_stat) == 40, "dwarfs_c_stat size drift");
_Static_assert(offsetof(struct dwarfs_c_stat, size) == 0, "size offset");
_Static_assert(offsetof(struct dwarfs_c_stat, mtime) == 8, "mtime offset");
_Static_assert(offsetof(struct dwarfs_c_stat, mtime_nsec) == 16, "mtime_nsec offset");
_Static_assert(offsetof(struct dwarfs_c_stat, mode) == 20, "mode offset");
_Static_assert(offsetof(struct dwarfs_c_stat, uid) == 24, "uid offset");
_Static_assert(offsetof(struct dwarfs_c_stat, gid) == 28, "gid offset");
_Static_assert(offsetof(struct dwarfs_c_stat, nlink) == 32, "nlink offset");
_Static_assert(offsetof(struct dwarfs_c_stat, type) == 36, "type offset");

/* struct dwarfs_c_dirent layout */
_Static_assert(sizeof(struct dwarfs_c_dirent) == 16, "dwarfs_c_dirent size drift");
_Static_assert(offsetof(struct dwarfs_c_dirent, name) == 0, "name offset");
_Static_assert(offsetof(struct dwarfs_c_dirent, type) == 8, "type offset");

/* enum dwarfs_c_file_type values */
_Static_assert(DWARFS_C_FILE_UNKNOWN == 0, "FILE_UNKNOWN value");
_Static_assert(DWARFS_C_FILE_REGULAR == 1, "FILE_REGULAR value");
_Static_assert(DWARFS_C_FILE_DIRECTORY == 2, "FILE_DIRECTORY value");
_Static_assert(DWARFS_C_FILE_SYMLINK == 3, "FILE_SYMLINK value");
_Static_assert(DWARFS_C_FILE_OTHER == 4, "FILE_OTHER value");

/* DWARFS_C_OFFSET_AUTO */
_Static_assert(DWARFS_C_OFFSET_AUTO == -1, "OFFSET_AUTO value");

int dwarfs_c_abi_check(void)
{
  return 0;
}
