//! Build script for dwarfs-sys.
//!
//! `vendored` mode (the only mode in v1): compiles `libdwarfs_c` and its
//! whole static dependency closure from the vendored `dwarfs-t` git
//! submodule using dwarfs-t's own CMake/vcpkg build (overlay ports and
//! triplets included), then links the curated closure in dependent-first
//! order.
//!
//! Environment knobs:
//!
//! - `DWARFS_RS_DWARFS_T_SOURCE` — path to a dwarfs-t checkout.
//!   Default: the `dwarfs-t` submodule next to this crate's manifest
//!   (run `git submodule update --init` after cloning).
//! - `DWARFS_RS_VCPKG_ROOT` (or `VCPKG_ROOT`) — vcpkg installation root
//!   (must contain `scripts/buildsystems/vcpkg.cmake`). REQUIRED.
//! - `DWARFS_RS_VCPKG_TRIPLET` — vcpkg triplet; default is derived from the
//!   Rust target (e.g. `arm64-osx-static`, `x64-linux-static`).
//! - `DWARFS_RS_CMAKE_BUILD_TYPE` — default `Release`.
//! - `DWARFS_RS_VERBOSE=1` — stream CMake output instead of swallowing it.
//!
//! Notes:
//! - Reconfigures only when there is no CMake cache; the compile step itself
//!   is incremental (ninja/make no-op). Run `cargo clean -p dwarfs-sys` after
//!   moving the dwarfs-t submodule to a new ref.
//! - Windows/MSVC: dwarfs-t's `*-windows-static` triplets use the static CRT
//!   (/MT) which mismatches Rust's default dynamic CRT (/MD). Untested in v1.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// dwarfs libraries, in dependent-first static link order.
const DWARFS_LIBS: &[&str] = &[
    "dwarfs_c",
    "dwarfs_reader",
    "dwarfs_decompressor",
    "dwarfs_common",
    "dwarfs_metadata_legacy",
];

/// vcpkg dependency libraries, in dependent-first static link order.
/// Only libraries actually present in the vcpkg lib dir are emitted, so
/// configuration drift (e.g. FLAC off) does not break the link.
const VCPKG_LIBS: &[&str] = &[
    "crypto",
    "xxhash",
    "zstd",
    "fmt",
    "lz4",
    "lzma",
    "FLAC++",
    "FLAC",
    "ogg",
    "brotlidec",
    "brotlienc",
    "brotlicommon",
    "boost_chrono",
    "boost_program_options",
    "boost_process",
    "boost_date_time",
    "boost_container",
    "boost_context",
    "boost_filesystem",
    "boost_atomic",
];

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target = env::var("TARGET").unwrap();

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=abi_check.c");
    for var in [
        "DWARFS_RS_DWARFS_T_SOURCE",
        "DWARFS_RS_VCPKG_ROOT",
        "VCPKG_ROOT",
        "DWARFS_RS_VCPKG_TRIPLET",
        "DWARFS_RS_CMAKE_BUILD_TYPE",
    ] {
        println!("cargo:rerun-if-env-changed={var}");
    }

    // ---------------------------------------------------------------
    // Locate the dwarfs-t sources (vendored submodule by default)
    // ---------------------------------------------------------------
    let dwarfs_t = env::var("DWARFS_RS_DWARFS_T_SOURCE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            // The submodule lives at the workspace root in this repo; also
            // accept it next to the crate (standalone crate layout).
            let local = manifest_dir.join("dwarfs-t");
            if local.join("include/dwarfs_c.h").exists() {
                local
            } else {
                manifest_dir.join("../dwarfs-t")
            }
        });
    let dwarfs_t = dwarfs_t.canonicalize().unwrap_or_else(|e| {
        panic!(
            "dwarfs-t source dir {} not accessible: {e}",
            dwarfs_t.display()
        )
    });
    let header = dwarfs_t.join("include/dwarfs_c.h");
    println!("cargo:rerun-if-changed={}", header.display());
    if !header.exists() {
        panic!(
            "dwarfs_c.h not found at {}.\n\
             The dwarfs-t submodule is missing — run `git submodule update --init`,\n\
             or point DWARFS_RS_DWARFS_T_SOURCE at a dwarfs-t checkout.",
            header.display()
        );
    }

    // ---------------------------------------------------------------
    // ABI cross-check of the hand-written FFI declarations (cheap)
    // ---------------------------------------------------------------
    cc::Build::new()
        .file("abi_check.c")
        .include(dwarfs_t.join("include"))
        .compile("dwarfs_c_abi_check");

    // ---------------------------------------------------------------
    // vcpkg root + triplet
    // ---------------------------------------------------------------
    let vcpkg_root = env::var("DWARFS_RS_VCPKG_ROOT")
        .or_else(|_| env::var("VCPKG_ROOT"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            panic!(
                "vcpkg root not set.\n\
                 Set DWARFS_RS_VCPKG_ROOT (or VCPKG_ROOT) to a vcpkg installation\n\
                 (a directory containing scripts/buildsystems/vcpkg.cmake)."
            )
        });
    let toolchain = vcpkg_root.join("scripts/buildsystems/vcpkg.cmake");
    if !toolchain.exists() {
        panic!("vcpkg toolchain not found at {}", toolchain.display());
    }

    let triplet = env::var("DWARFS_RS_VCPKG_TRIPLET").unwrap_or_else(|_| default_triplet(&target));
    let build_type = env::var("DWARFS_RS_CMAKE_BUILD_TYPE").unwrap_or_else(|_| "Release".into());
    let verbose = env::var("DWARFS_RS_VERBOSE").is_ok();

    // ---------------------------------------------------------------
    // Configure (only on a cold cache) and build libdwarfs_c
    // ---------------------------------------------------------------
    let build_dir = out_dir.join("dwarfs-build");
    std::fs::create_dir_all(&build_dir).unwrap();

    let generator = if have_program("ninja") {
        "Ninja"
    } else if !target.contains("windows") {
        "Unix Makefiles"
    } else {
        panic!("ninja not found in PATH; install ninja (required on Windows)");
    };

    if !build_dir.join("CMakeCache.txt").exists() {
        let mut cmd = Command::new("cmake");
        cmd.arg("-S")
            .arg(&dwarfs_t)
            .arg("-B")
            .arg(&build_dir)
            .arg(format!("-G{generator}"))
            .arg(format!("-DCMAKE_BUILD_TYPE={build_type}"))
            .arg(format!("-DCMAKE_TOOLCHAIN_FILE={}", toolchain.display()))
            .arg(format!("-DVCPKG_TARGET_TRIPLET={triplet}"))
            .arg(format!(
                "-DVCPKG_OVERLAY_PORTS={}",
                dwarfs_t.join("vcpkg_ports").display()
            ))
            .arg(format!(
                "-DVCPKG_OVERLAY_TRIPLETS={}",
                dwarfs_t.join("vcpkg_triplets").display()
            ))
            // Reader binding only: no tools, no tests, no FUSE driver.
            .arg("-DWITH_TESTS=OFF")
            .arg("-DWITH_TOOLS=OFF")
            .arg("-DWITH_BENCHMARKS=OFF")
            .arg("-DWITH_LIBDWARFS=ON")
            .arg("-DDWARFS_WITH_FLATBUFFERS=ON")
            .arg("-DDWARFS_WITH_THRIFT=OFF")
            .arg("-DDWARFS_WITH_FUSE=OFF")
            .arg("-DWITH_FUSE_DRIVER=OFF");
        if target.contains("apple-darwin") {
            let arch = match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
                Ok("aarch64") => "arm64",
                Ok("x86_64") => "x86_64",
                Ok(other) => panic!("unsupported macOS arch {other}"),
                Err(_) => panic!("CARGO_CFG_TARGET_ARCH unset"),
            };
            cmd.arg(format!("-DCMAKE_OSX_ARCHITECTURES={arch}"));
        }
        run(cmd, verbose, "cmake configure");
    }

    let mut build = Command::new("cmake");
    build
        .arg("--build")
        .arg(&build_dir)
        .arg("--target")
        .arg("dwarfs_c")
        .arg("--parallel");
    run(build, verbose, "cmake build (target dwarfs_c)");

    // ---------------------------------------------------------------
    // Emit link instructions
    // ---------------------------------------------------------------
    let vcpkg_lib = build_dir.join("vcpkg_installed").join(&triplet).join("lib");
    println!("cargo:rustc-link-search=native={}", build_dir.display());
    println!("cargo:rustc-link-search=native={}", vcpkg_lib.display());

    for lib in DWARFS_LIBS {
        println!("cargo:rustc-link-lib=static={lib}");
    }
    for lib in VCPKG_LIBS {
        if lib_exists(&vcpkg_lib, lib) {
            println!("cargo:rustc-link-lib=static={lib}");
        }
    }

    // C++ runtime + platform extras
    if target.contains("apple") {
        println!("cargo:rustc-link-lib=c++");
    } else if target.contains("linux") || target.contains("android") {
        println!("cargo:rustc-link-lib=stdc++");
        for lib in ["pthread", "dl", "m"] {
            println!("cargo:rustc-link-lib={lib}");
        }
    } else if target.contains("windows-msvc") {
        println!(
            "cargo:warning=windows-msvc is untested in v1: dwarfs-t's static \
             triplets use /MT (static CRT), Rust defaults to /MD; link errors \
             are likely. See README."
        );
    }
}

fn default_triplet(target: &str) -> String {
    match target {
        "aarch64-apple-darwin" => "arm64-osx-static",
        "x86_64-apple-darwin" => "x64-osx-static",
        "x86_64-unknown-linux-gnu" | "x86_64-unknown-linux-musl" => "x64-linux-static",
        "aarch64-unknown-linux-gnu" | "aarch64-unknown-linux-musl" => "arm64-linux-static",
        "x86_64-pc-windows-msvc" => "x64-windows-static",
        "aarch64-pc-windows-msvc" => "arm64-windows-static",
        other => panic!(
            "no default vcpkg triplet for target {other}; set DWARFS_RS_VCPKG_TRIPLET explicitly"
        ),
    }
    .to_string()
}

fn lib_exists(dir: &Path, name: &str) -> bool {
    dir.join(format!("lib{name}.a")).exists() || dir.join(format!("{name}.lib")).exists()
}

fn have_program(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run(mut cmd: Command, verbose: bool, what: &str) {
    let display = format!("{cmd:?}");
    let output = if verbose {
        let status = cmd
            .status()
            .unwrap_or_else(|e| panic!("failed to spawn {what}: {e}\n  {display}"));
        if status.success() {
            return;
        }
        panic!("{what} failed with {status}\n  {display}");
    } else {
        cmd.output()
            .unwrap_or_else(|e| panic!("failed to spawn {what}: {e}\n  {display}"))
    };
    if !output.status.success() {
        panic!(
            "{what} failed with {}\n  {display}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
