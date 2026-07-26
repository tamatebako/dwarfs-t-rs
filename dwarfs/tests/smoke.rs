//! Integration tests for the safe `dwarfs-t` wrapper against the
//! `tests/fixtures/data.dwarfs` image (borrowed from dwarfs-t's test data).
//!
//! Covers the whole reader surface: open from file / memory / file region,
//! stat, pread, directory listing, image info JSON, and the error paths.
//!
//! Requires the native library: without `vendored` every operation fails
//! with ENOTSUP, so the whole file is compiled out in skeleton builds.
#![cfg(feature = "vendored")]

use std::path::{Path, PathBuf};

use dwarfs_t::{DwarfsError, ErrorKind, FileType, Filesystem, OFFSET_AUTO};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/data.dwarfs")
        .canonicalize()
        .expect("fixture image must exist")
}

fn fixture_bytes() -> Vec<u8> {
    std::fs::read(fixture()).expect("fixture image must be readable")
}

/// Create `junk + image` in a temp file and return (path, image_len).
fn embedded_fixture(junk_len: usize) -> (PathBuf, u64) {
    let image = fixture_bytes();
    let path = std::env::temp_dir().join(format!(
        "dwarfs-rs-test-embedded-{}-{junk_len}.dwarfs",
        std::process::id()
    ));
    let mut data = vec![0xABu8; junk_len];
    data.extend_from_slice(&image);
    std::fs::write(&path, &data).expect("write embedded fixture");
    (path, image.len() as u64)
}

#[test]
fn version_is_reported() {
    assert!(Filesystem::version() >= 0); // 0 is legit in tag-less dev clones
    assert!(!Filesystem::version_string().is_empty());
}

#[test]
fn open_stat_pread_from_file() {
    let fs = Filesystem::open(fixture()).expect("open image");

    // Root is a directory (both "" and "/" spellings).
    let root = fs.stat("/").expect("stat root");
    assert_eq!(root.file_type, FileType::Directory);
    assert!(root.is_dir());
    let root2 = fs.stat("").expect("stat empty path = root");
    assert_eq!(root2.file_type, FileType::Directory);

    // A known file inside the fixture.
    let meta = fs.stat("format.sh").expect("stat format.sh");
    assert_eq!(meta.file_type, FileType::Regular);
    assert!(meta.is_file());
    assert!(meta.size > 0);
    assert!(meta.nlink >= 1);

    // Leading slash works the same.
    let meta2 = fs.stat("/format.sh").expect("stat /format.sh");
    assert_eq!(meta2.size, meta.size);

    // Read the whole file, then verify an offset read against it.
    let mut whole = vec![0u8; meta.size as usize];
    let n = fs
        .pread("format.sh", &mut whole, 0)
        .expect("pread whole file");
    assert_eq!(n as u64, meta.size);

    if meta.size >= 16 {
        let mut tail = [0u8; 16];
        let n = fs
            .pread("format.sh", &mut tail, meta.size as i64 - 16)
            .expect("pread tail");
        assert_eq!(n, 16);
        assert_eq!(&tail, &whole[whole.len() - 16..]);
    }

    // EOF semantics: reading at the end yields 0.
    let mut one = [0u8; 1];
    let n = fs
        .pread("format.sh", &mut one, meta.size as i64)
        .expect("pread at EOF");
    assert_eq!(n, 0);
}

#[test]
fn read_dir_lists_entries() {
    let fs = Filesystem::open(fixture()).expect("open image");
    let mut count = 0;
    let mut found_format_sh = false;
    for entry in fs.read_dir("/").expect("read_dir root") {
        let entry = entry.expect("entry must not error");
        count += 1;
        assert!(!entry.name.is_empty());
        assert_ne!(entry.name, ".");
        assert_ne!(entry.name, "..");
        if entry.name == "format.sh" {
            found_format_sh = true;
            assert_eq!(entry.file_type, FileType::Regular);
        }
    }
    assert!(count > 0, "root must have entries");
    assert!(found_format_sh, "format.sh must be in root listing");
}

#[test]
fn image_info_json_reports_metadata() {
    let fs = Filesystem::open(fixture()).expect("open image");
    let info = fs.image_info_json().expect("image info json");
    assert!(info.contains("\"version\""), "info has version: {info}");
    assert!(
        info.contains("\"block_size\""),
        "info has block_size: {info}"
    );
}

#[test]
fn open_memory_works() {
    let bytes = fixture_bytes();
    let size = bytes.len();
    let fs = Filesystem::open_memory(&bytes).expect("open memory image");
    drop(bytes); // the safe API owns its copy

    let meta = fs.stat("format.sh").expect("stat via memory image");
    assert!(meta.size > 0);

    let mut buf = [0u8; 8];
    let n = fs
        .pread("format.sh", &mut buf, 0)
        .expect("pread via memory");
    assert_eq!(n, 8);

    let disk_meta = Filesystem::open(fixture())
        .and_then(|fs| fs.stat("format.sh"))
        .expect("stat via file image");
    assert_eq!(meta.size, disk_meta.size);
    let _ = size;
}

#[test]
fn open_region_works() {
    let image_len = fixture_bytes().len() as u64;

    // The whole file as a region.
    let fs = Filesystem::open_region(fixture(), 0, image_len as i64 as u64)
        .expect("open whole-file region");
    assert!(fs.stat("format.sh").is_ok());
    drop(fs);

    // The image embedded behind a junk prefix (explicit offset).
    let (embedded, image_len) = embedded_fixture(128);
    let fs = Filesystem::open_region(&embedded, 128, image_len).expect("open embedded region");
    let meta = fs.stat("format.sh").expect("stat via embedded region");
    assert!(meta.size > 0);
    drop(fs);

    // Auto-detected offset.
    let fs = Filesystem::open_region(&embedded, OFFSET_AUTO, image_len)
        .expect("open embedded region with OFFSET_AUTO");
    assert!(fs.stat("format.sh").is_ok());
    drop(fs);

    let _ = std::fs::remove_file(&embedded);
}

#[test]
fn error_paths_are_mapped() {
    let fs = Filesystem::open(fixture()).expect("open image");

    // Missing image file.
    let err = Filesystem::open("/nonexistent/image.dwarfs").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::NotFound);
    assert_eq!(err.errno(), libc::ENOENT);
    assert!(!err.message().is_empty());

    // Missing path inside the image.
    let err = fs.stat("no-such-file").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::NotFound);

    let err = fs.pread("no-such-file", &mut [0u8; 4], 0).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::NotFound);

    // Directory read as a file.
    let err = fs.pread("/", &mut [0u8; 4], 0).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::IsADirectory);

    // File listed as a directory.
    let err = fs.read_dir("format.sh").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::NotADirectory);

    // Negative offset.
    let err = fs.pread("format.sh", &mut [0u8; 4], -1).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidInput);

    // Bad open arguments.
    let err = Filesystem::open_memory(&[]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidInput);

    let err = Filesystem::open_region(fixture(), 0, 0).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidInput);

    let err = Filesystem::open_region(fixture(), -5, 100).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidInput);
}

#[test]
fn errors_implement_std_error() {
    fn assert_error<T: std::error::Error>(_: &T) {}
    let err: DwarfsError = Filesystem::open("/nonexistent/image.dwarfs").unwrap_err();
    assert_error(&err);
    let _ = format!("{err}");
    let _ = format!("{err:?}");
}

#[test]
fn filesystem_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>(_: &T) {}
    let fs = Filesystem::open(fixture()).expect("open image");
    assert_send_sync(&fs);
}

#[test]
fn concurrent_preads_on_same_handle() {
    use std::sync::Arc;

    let fs = Arc::new(Filesystem::open(fixture()).expect("open image"));
    let meta = fs.stat("format.sh").expect("stat");
    let mut handles = Vec::new();
    for _ in 0..4 {
        let fs = Arc::clone(&fs);
        handles.push(std::thread::spawn(move || {
            let mut buf = vec![0u8; meta.size as usize];
            fs.pread("format.sh", &mut buf, 0).expect("pread")
        }));
    }
    for h in handles {
        assert_eq!(h.join().unwrap() as u64, meta.size);
    }
}
