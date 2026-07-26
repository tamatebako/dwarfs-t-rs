//! Integration tests for the safe `Writer` — create images in-process and
//! read them back through `Filesystem` (the reader half of the same ABI).
//!
//! Covers the writer round-trip (tree structure + full file contents via
//! stat/pread/read_dir), the option paths, and the errno contract
//! (EEXIST / EALREADY / EINVAL / ENOENT / ENOTDIR).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use dwarfs_t::{Compression, DwarfsError, ErrorKind, FileType, Filesystem, Writer, WriterOptions};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Unique writable scratch dir for one test.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "dwarfs-t-rs-writer-{}-{name}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn write_file(path: &Path, contents: &str) {
    std::fs::write(path, contents).expect("write fixture file");
}

fn read_back(image: &Path, inner: &str, expect: &str) {
    let fs = Filesystem::open(image).expect("read back image");
    let meta = fs
        .stat(inner)
        .unwrap_or_else(|e| panic!("stat {inner}: {e}"));
    assert_eq!(meta.file_type, FileType::Regular);
    assert_eq!(meta.size as usize, expect.len());
    let mut buf = vec![0u8; expect.len()];
    let n = fs.pread(inner, &mut buf, 0).expect("pread");
    assert_eq!(n, expect.len());
    assert_eq!(buf, expect.as_bytes());
}

fn errno_is(err: &DwarfsError, errno: i32) -> bool {
    err.errno() == errno
}

#[test]
fn tree_round_trip_via_reader() {
    let dir = scratch("tree");
    let tree = dir.join("tree");
    std::fs::create_dir_all(tree.join("sub")).unwrap();
    write_file(&tree.join("hello.txt"), "hello-dwarfs-t-rs\n");
    write_file(&tree.join("sub/nested.txt"), "nested-42\n");

    let out = dir.join("out.dwarfs");
    let mut w = Writer::new(WriterOptions::default()).expect("create writer");
    w.add_tree(&tree, "/").expect("add_tree");
    w.write(&out).expect("write image");

    let fs = Filesystem::open(&out).expect("open image");
    assert_eq!(fs.stat("/").unwrap().file_type, FileType::Directory);
    assert_eq!(fs.stat("sub").unwrap().file_type, FileType::Directory);

    let entries: Vec<String> = fs.read_dir("/").unwrap().map(|e| e.unwrap().name).collect();
    assert_eq!(entries, ["hello.txt", "sub"]);

    read_back(&out, "hello.txt", "hello-dwarfs-t-rs\n");
    read_back(&out, "sub/nested.txt", "nested-42\n");

    // Metadata sections must be present and parse.
    let json = fs.image_info_json().expect("image info json");
    assert!(json.contains("\"sections\""));
    assert!(json.contains("\"history\""));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn compression_option_paths() {
    let dir = scratch("algos");
    let tree = dir.join("tree");
    std::fs::create_dir_all(&tree).unwrap();
    write_file(&tree.join("payload.txt"), "payload-payload-payload\n");

    for (algo, level, name) in [
        (Compression::None, None, "none"),
        (Compression::Zstd, Some(3), "zstd3"),
        (Compression::Lzma, None, "lzma"),
        (Compression::Brotli, Some(4), "brotli4"),
    ] {
        let out = dir.join(format!("{name}.dwarfs"));
        let mut w = Writer::with_compression(algo, level).expect("create writer");
        w.add_tree(&tree, "/").expect("add_tree");
        w.write(&out)
            .unwrap_or_else(|e| panic!("write {name}: {e}"));
        read_back(&out, "payload.txt", "payload-payload-payload\n");
    }

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn add_file_mode_round_trip_and_rules() {
    let dir = scratch("files");
    let files = dir.join("files");
    std::fs::create_dir_all(&files).unwrap();
    write_file(&files.join("a.txt"), "aaa\n");
    write_file(&files.join("b.txt"), "bbb\n");

    let out = dir.join("files.dwarfs");
    let mut w = Writer::new(WriterOptions::default()).unwrap();

    // v1: no renames
    let err = w.add_file(files.join("a.txt"), "renamed.txt").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidInput);

    // v1: all files must share one directory
    write_file(&dir.join("elsewhere.txt"), "nope\n");
    w.add_file(files.join("a.txt"), "a.txt").unwrap();
    let err = w
        .add_file(dir.join("elsewhere.txt"), "elsewhere.txt")
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidInput);

    w.add_file(files.join("b.txt"), "b.txt").unwrap();
    w.write(&out).expect("write image");

    read_back(&out, "a.txt", "aaa\n");
    read_back(&out, "b.txt", "bbb\n");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn add_tree_error_paths() {
    let dir = scratch("errors");
    write_file(&dir.join("a-file.txt"), "x\n");

    let mut w = Writer::new(WriterOptions::default()).unwrap();

    let err = w.add_tree(dir.join("no-such-dir"), "/").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::NotFound);

    let err = w.add_tree(dir.join("a-file.txt"), "/").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::NotADirectory);
    assert!(errno_is(&err, libc::ENOTDIR));

    let err = w.add_tree(&dir, "/app/sub").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidInput);

    // No source added so far: write must fail with EINVAL.
    let err = w.write(dir.join("nothing.dwarfs")).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidInput);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn single_source_and_ealready_semantics() {
    let dir = scratch("single");
    let tree = dir.join("tree");
    std::fs::create_dir_all(&tree).unwrap();
    write_file(&tree.join("x.txt"), "x\n");

    let mut w = Writer::new(WriterOptions::default()).unwrap();
    w.add_tree(&tree, "/").unwrap();

    let err = w.add_tree(&tree, "/").unwrap_err();
    assert!(errno_is(&err, libc::EALREADY));

    let err = w.add_file(tree.join("x.txt"), "x.txt").unwrap_err();
    assert!(errno_is(&err, libc::EALREADY));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn write_never_overwrites_and_write_consumes() {
    let dir = scratch("eexist");
    let tree = dir.join("tree");
    std::fs::create_dir_all(&tree).unwrap();
    write_file(&tree.join("x.txt"), "x\n");

    let out = dir.join("out.dwarfs");
    std::fs::write(&out, b"already here").unwrap();

    let mut w = Writer::new(WriterOptions::default()).unwrap();
    w.add_tree(&tree, "/").unwrap();
    let err = w.write(&out).unwrap_err();
    assert!(errno_is(&err, libc::EEXIST));

    // A fresh writer can write to a fresh path, but only once.
    let mut w = Writer::new(WriterOptions::default()).unwrap();
    w.add_tree(&tree, "/").unwrap();
    std::fs::remove_file(&out).unwrap();
    w.write(&out).expect("write image");
    // (write consumes self — a second call is a compile-time impossibility)

    read_back(&out, "x.txt", "x\n");

    std::fs::remove_dir_all(&dir).ok();
}
