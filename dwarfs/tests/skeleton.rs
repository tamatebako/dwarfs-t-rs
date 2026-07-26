//! Skeleton-mode tests: compiled only when the `vendored` feature is OFF.
//! Every operation must fail with ENOTSUP ([`ErrorKind::NotSupported`]) —
//! this is what lets consumers build the crate with no native toolchain and
//! gate the DwarFS backend behind their own optional feature.
#![cfg(not(feature = "vendored"))]

use dwarfs_t::{DwarfsError, ErrorKind, Filesystem, Writer, WriterOptions};

fn assert_not_supported(err: DwarfsError) {
    assert_eq!(err.kind(), ErrorKind::NotSupported);
    assert_eq!(err.errno(), libc::ENOTSUP);
    assert!(
        err.message().contains("vendored"),
        "message should explain the skeleton build: {}",
        err.message()
    );
}

#[test]
fn open_reports_not_supported() {
    assert_not_supported(Filesystem::open("whatever.dwarfs").unwrap_err());
}

#[test]
fn open_memory_reports_not_supported() {
    assert_not_supported(Filesystem::open_memory(&[1, 2, 3, 4]).unwrap_err());
}

#[test]
fn open_region_reports_not_supported() {
    assert_not_supported(Filesystem::open_region("whatever.dwarfs", 0, 16).unwrap_err());
}

#[test]
fn writer_reports_not_supported() {
    assert_not_supported(Writer::new(WriterOptions::default()).unwrap_err());
}

#[test]
fn argument_validation_still_happens_first() {
    // Rust-side argument checks run before the FFI call, so they keep their
    // EINVAL behavior even in skeleton builds.
    let err = Filesystem::open_memory(&[]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidInput);
    let err = Filesystem::open_region("x", 0, 0).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidInput);
}

#[test]
fn version_reports_skeleton() {
    assert_eq!(Filesystem::version(), 0);
    assert!(Filesystem::version_string().contains("skeleton"));
}
