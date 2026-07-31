//! Build script for dwarfs-t-sys.
//!
//! `vendored` mode (the only native mode in v1): compiles `libdwarfs_c` and
//! its whole static dependency closure from the vendored `dwarfs-t` git
//! submodule using dwarfs-t's own CMake/vcpkg build (overlay ports and
//! triplets included), then links the curated closure in dependent-first
//! order.
//!
//! The native build is skipped in two cases:
//!
//! - **docs.rs** (`DOCS_RS` env var set): docs.rs cannot run CMake/vcpkg,
//!   and rustdoc never links the native library, so there is nothing to do.
//! - **skeleton mode** (the `vendored` feature disabled, i.e.
//!   `--no-default-features`): nothing native is built or linked; the crate
//!   compiles Rust-side stubs that fail every ABI call with `ENOTSUP`, so
//!   dependent crates always build — and link — without a native toolchain.
//!
//! Environment knobs:
//!
//! - `DWARFS_RS_DWARFS_T_SOURCE` — path to a dwarfs-t checkout.
//!   Default: the `dwarfs-t` submodule next to this crate's manifest
//!   (run `git submodule update --init` after cloning). REQUIRED for the
//!   crates.io package, which does not bundle the dwarfs-t sources.
//! - `DWARFS_RS_VCPKG_ROOT` (or `VCPKG_ROOT`) — vcpkg installation root
//!   (must contain `scripts/buildsystems/vcpkg.cmake`). REQUIRED.
//! - `DWARFS_RS_VCPKG_TRIPLET` — vcpkg triplet; default is derived from the
//!   Rust target (e.g. `arm64-osx-static`, `x64-linux-static`,
//!   `x64-windows-static-md`).
//! - `DWARFS_RS_CMAKE_BUILD_TYPE` — default `Release`.
//! - `DWARFS_RS_VERBOSE=1` — stream CMake output instead of swallowing it.
//!
//! Notes:
//! - Reconfigures only when there is no CMake cache; the compile step itself
//!   is incremental (ninja/make no-op). Run `cargo clean -p dwarfs-sys` after
//!   moving the dwarfs-t submodule to a new ref.
//! - Windows CRT strategy: Rust links the dynamic CRT (/MD) by default, so
//!   the default MSVC triplets are the `*-windows-static-md` variants
//!   (static libraries, dynamic CRT) — NOT `*-windows-static`, which is /MT
//!   and mismatches at link time. A build-script probe refuses the
//!   combination of `+crt-static` (/MT) with the default /MD triplet; see
//!   `default_triplet` and `crt_static_enabled` below. The vendored Windows
//!   build is wired but has no CI coverage yet (unproven).

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// dwarfs libraries, in dependent-first static link order.
///
/// `true` = required (link always); `false` = emit only when the archive
/// exists (drift tolerance for older vendored refs: the writer API of
/// libdwarfs_c links dwarfs_writer, which in turn needs dwarfs_compressor —
/// refs predating the writer binding ship neither).
const DWARFS_LIBS: &[(&str, bool)] = &[
    ("dwarfs_c", true),
    ("dwarfs_writer", false),
    ("dwarfs_reader", true),
    ("dwarfs_decompressor", true),
    ("dwarfs_compressor", false),
    ("dwarfs_common", true),
    ("dwarfs_metadata_legacy", true),
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

    // docs.rs cannot run CMake/vcpkg — and rustdoc never links the native
    // library, so there is nothing to build.
    if env::var_os("DOCS_RS").is_some() {
        println!(
            "cargo:warning=DOCS_RS detected: skipping the libdwarfs_c native build \
             (documentation-only build)"
        );
        return;
    }

    // Skeleton mode (`--no-default-features`): no native library is built or
    // linked; src/lib.rs compiles ENOTSUP stubs for every ABI entry point.
    if env::var_os("CARGO_FEATURE_VENDORED").is_none() {
        println!(
            "cargo:warning=the `vendored` feature is disabled: building the ENOTSUP \
             skeleton — no native library is linked and every dwarfs_c_* call fails \
             with ENOTSUP"
        );
        return;
    }

    // ---------------------------------------------------------------
    // Target triplet + CRT strategy (policy first: refuse misconfigurations
    // before touching the toolchain or sources)
    // ---------------------------------------------------------------
    let explicit_triplet = env::var("DWARFS_RS_VCPKG_TRIPLET").ok();
    let triplet = explicit_triplet
        .clone()
        .unwrap_or_else(|| default_triplet(&target));
    let build_type = env::var("DWARFS_RS_CMAKE_BUILD_TYPE").unwrap_or_else(|_| "Release".into());
    let verbose = env::var("DWARFS_RS_VERBOSE").is_ok();

    // CRT strategy enforcement (windows-msvc only): Rust and the vcpkg-built
    // static closure must agree on the CRT. /MD is the default on both sides
    // (Rust default, *-windows-static-md triplet); /MT is opt-in on both
    // sides (+crt-static, *-windows-static triplet). Anything mismatched
    // fails at link time with inscrutable errors, so refuse/warn early.
    if target.contains("windows-msvc") {
        let crt_static = crt_static_enabled();
        match (crt_static, &explicit_triplet) {
            // /MT forced on the Rust side, but the default triplet is /MD:
            // refuse with a named, actionable error.
            (true, None) => panic!(
                "windows-msvc CRT mismatch: Rust is built with the static CRT \
                 (-C target-feature=+crt-static, /MT) but the default vcpkg triplet \
                 is {triplet} (dynamic CRT, /MD).\n\
                 Either keep the /MD default (drop +crt-static, or pass \
                 -C target-feature=-crt-static), or opt into /MT on the vcpkg side \
                 too: DWARFS_RS_VCPKG_TRIPLET=x64-windows-static (resp. \
                 arm64-windows-static)."
            ),
            // /MT forced with an explicit triplet: the user owns CRT
            // consistency, but make sure they noticed.
            (true, Some(_)) => println!(
                "cargo:warning=windows-msvc with +crt-static (/MT): the explicit \
                 DWARFS_RS_VCPKG_TRIPLET={triplet} must be a static-CRT (/MT) triplet \
                 (e.g. x64-windows-static); a /MD triplet (*-windows-static-md) will \
                 fail to link."
            ),
            // /MD Rust (default) with an explicitly selected /MT triplet.
            (false, Some(t)) if is_static_crt_triplet(t) => println!(
                "cargo:warning=windows-msvc: Rust defaults to the dynamic CRT (/MD) \
                 but DWARFS_RS_VCPKG_TRIPLET={t} is a static-CRT (/MT) triplet; link \
                 errors are likely. Use the default *-windows-static-md triplet or \
                 build Rust with -C target-feature=+crt-static."
            ),
            (false, _) => {}
        }
    }

    // ---------------------------------------------------------------
    // Locate the dwarfs-t sources (vendored submodule by default). The
    // paths cargo hands us are already absolute; do NOT canonicalize —
    // on Windows that yields the `\\?\D:\` verbatim form, which the
    // mingw toolchain cannot parse (proven on windows-latest:
    // `dwarfs_c.h: No such file or directory` from abi_check.c with the
    // submodule present).
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
    let header = dwarfs_t.join("include/dwarfs_c.h");
    println!("cargo:rerun-if-changed={}", header.display());
    if !header.exists() {
        panic!(
            "dwarfs_c.h not found at {}.\n\
             In a git checkout: run `git submodule update --init`.\n\
             From crates.io: the dwarfs-t sources are not bundled — point\n\
             DWARFS_RS_DWARFS_T_SOURCE at a dwarfs-t checkout, or build with\n\
             --no-default-features for the pure-cargo ENOTSUP skeleton.",
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
    // vcpkg root
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
            // The dwarfs_c binding (reader + writer): no tools, no tests,
            // no FUSE driver. The driver is a dwarfs-t product-line tool and
            // is never part of the binding surface, so it is always OFF here;
            // with DWARFS_WITH_FUSE=OFF, dwarfs-t's need_fuse.cmake never
            // probes for libfuse. dwarfs-t's vcpkg manifest mirrors this:
            // libfuse sits behind the opt-in `fuse` manifest feature, which
            // the default-feature install used here never enables (libfuse
            // does not build on musl).
            //
            // TODO(fuse): if the tfs-fuse work ever needs the driver through
            // this binding, add a `fuse` cargo feature that flips these two
            // flags ON and passes -DVCPKG_MANIFEST_FEATURES=fuse.
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
        // MinGW cross-compilation (windows-gnu/gnullvm): vcpkg's port
        // builds pick the triplet's compiler, but a downstream CMAKE
        // PROJECT like this one needs it explicitly — otherwise cmake
        // quietly configures the HOST compiler and every dwarfs-t object
        // lands as a host binary (proven: libdwarfs_c.a held arm64 Mach-O
        // objects for an x86_64-pc-windows-gnu build).
        if target == "x86_64-pc-windows-gnu" || target == "x86_64-pc-windows-gnullvm" {
            cmd.arg("-DCMAKE_SYSTEM_NAME=Windows")
                .arg("-DCMAKE_C_COMPILER=x86_64-w64-mingw32-gcc")
                .arg("-DCMAKE_CXX_COMPILER=x86_64-w64-mingw32-g++")
                .arg("-DCMAKE_RC_COMPILER=x86_64-w64-mingw32-windres")
                .arg("-DCMAKE_FIND_ROOT_PATH_MODE_PROGRAM=NEVER")
                .arg("-DCMAKE_FIND_ROOT_PATH_MODE_LIBRARY=ONLY")
                .arg("-DCMAKE_FIND_ROOT_PATH_MODE_INCLUDE=ONLY");
        }
        // vcpkg's autotools ports (the libiconv/openssl family) install via
        // msys2 `make -j N install` — parallel INSTALL is a proven failure
        // class on GitHub's windows runners (the packages compile, then
        // `make -j 5 install` dies mid-install; we lost three consecutive
        // legs to it: libiconv, libsodium, openssl). Serialize the port
        // builds on Windows hosts; the archives cache amortizes the cost.
        // The user's own VCPKG_MAX_CONCURRENCY always wins.
        if cfg!(windows) && env::var("VCPKG_MAX_CONCURRENCY").is_err() {
            cmd.env("VCPKG_MAX_CONCURRENCY", "1");
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

    for (lib, required) in DWARFS_LIBS {
        if *required {
            println!("cargo:rustc-link-lib=static={lib}");
        } else if let Some(stem) = lib_stem(&build_dir, lib) {
            println!("cargo:rustc-link-lib=static={stem}");
        }
    }
    for lib in VCPKG_LIBS {
        if let Some(stem) = lib_stem(&vcpkg_lib, lib) {
            println!("cargo:rustc-link-lib=static={stem}");
        }
    }

    // C++ runtime + platform extras. windows-msvc needs nothing explicit:
    // MSVC-compiled objects record their CRT/C++ runtime default libraries
    // (msvcprt, msvcrt, vcruntime) in .drectve sections, and the /MD vs /MT
    // choice is enforced by the CRT probe above. windows-gnu links the
    // MinGW C++ runtime chain (stdc++/gcc_eh/pthread are what the C++
    // objects reference).
    if target.contains("apple") {
        println!("cargo:rustc-link-lib=c++");
    } else if target.contains("linux") || target.contains("android") {
        println!("cargo:rustc-link-lib=stdc++");
        for lib in ["pthread", "dl", "m"] {
            println!("cargo:rustc-link-lib={lib}");
        }
    } else if target == "x86_64-pc-windows-gnu" || target == "x86_64-pc-windows-gnullvm" {
        // The MinGW C++ runtime chain plus the Windows system libraries
        // the C++ objects reference (boost.process → shell32/psapi,
        // OpenSSL's CAPI engine → crypt32, sockets → ws2_32, and the
        // usual bcrypt/ole32/uuid/advapi32 tail).
        for lib in [
            "stdc++", "gcc", "gcc_eh", "pthread", "shell32", "psapi", "crypt32", "ws2_32",
            "bcrypt", "ole32", "uuid", "advapi32",
        ] {
            println!("cargo:rustc-link-lib={lib}");
        }
    }
}

/// Default vcpkg triplet per Rust target.
///
/// CRT strategy (windows-msvc): Rust links the dynamic CRT (/MD) by default,
/// so the static triplets are the `*-windows-static-md` variants
/// (`VCPKG_CRT_LINKAGE dynamic` + static libraries) — NOT `*-windows-static`,
/// which is /MT and mismatches at link time. The `-md` triplets are upstream
/// vcpkg community triplets (no overlay file needed; dwarfs-t also ships
/// `x64-windows-static-md` CMake presets). `crt_static_enabled` /
/// `is_static_crt_triplet` police the /MT opt-in.
///
/// windows-gnullvm (llvm-mingw, UCRT — the ucrt64-class target of the C++
/// line) maps to `x64-mingw-static`: CRT linkage there is dynamic against
/// the UCRT, matching Rust's gnullvm targets, and it is the triplet the C++
/// libtfs builds used for windows-ucrt64. dwarfs-t ships an overlay
/// `x64-mingw-static.cmake`, so no new overlay triplet is needed.
fn default_triplet(target: &str) -> String {
    match target {
        "aarch64-apple-darwin" => "arm64-osx-static",
        "x86_64-apple-darwin" => "x64-osx-static",
        "x86_64-unknown-linux-gnu" | "x86_64-unknown-linux-musl" => "x64-linux-static",
        "aarch64-unknown-linux-gnu" | "aarch64-unknown-linux-musl" => "arm64-linux-static",
        "x86_64-pc-windows-msvc" => "x64-windows-static-md",
        "aarch64-pc-windows-msvc" => "arm64-windows-static-md",
        "x86_64-pc-windows-gnullvm" => "x64-mingw-static",
        // The msys ruby legs link with mingw-gcc: the same MinGW triplet
        // (dwarfs-t's overlay ships it); the CRT is ucrt on both sides.
        "x86_64-pc-windows-gnu" => "x64-mingw-static",
        other => panic!(
            "no default vcpkg triplet for target {other}; set DWARFS_RS_VCPKG_TRIPLET explicitly"
        ),
    }
    .to_string()
}

/// True when the Rust side is built with the static CRT
/// (`-C target-feature=+crt-static`, i.e. /MT on windows-msvc). Cargo
/// exposes the resolved target features to build scripts via
/// `CARGO_CFG_TARGET_FEATURE`.
fn crt_static_enabled() -> bool {
    env::var("CARGO_CFG_TARGET_FEATURE")
        .unwrap_or_default()
        .split(',')
        .any(|f| f == "crt-static")
}

/// True for the vcpkg triplets that build with the static CRT (/MT): the
/// `*-windows-static` family (upstream sets `VCPKG_CRT_LINKAGE static`;
/// dwarfs-t's overlay additionally pins `/MT` flags explicitly). The `-md`
/// variants are their dynamic-CRT counterparts; the MinGW triplets link the
/// (U)CRT dynamically.
fn is_static_crt_triplet(triplet: &str) -> bool {
    triplet.ends_with("-windows-static")
}

/// Archive file naming differs across toolchains: `libfmt.a` (unix/MinGW),
/// `fmt.lib` (MSVC), and openssl keeps its `lib` prefix even on MSVC
/// (`libcrypto.lib`). Returns the link stem to pass to
/// `cargo:rustc-link-lib=static=` — rustc derives the archive file name from
/// it per toolchain (`lib{stem}.a` resp. `{stem}.lib`) — or `None` when the
/// library is absent (drift tolerance, see `VCPKG_LIBS`).
fn lib_stem(dir: &Path, name: &str) -> Option<String> {
    if dir.join(format!("lib{name}.a")).exists() || dir.join(format!("{name}.lib")).exists() {
        Some(name.to_string())
    } else if dir.join(format!("lib{name}.lib")).exists() {
        Some(format!("lib{name}"))
    } else {
        // MinGW's boost naming (vcpkg's autoconfig convention):
        // libboost_x-<toolset>-mt-<arch>-<ver>.a — the stem the linker
        // wants drops the `lib` prefix but keeps the suffix.
        let prefix = format!("lib{name}-");
        std::fs::read_dir(dir).ok()?.find_map(|e| {
            let e = e.ok()?;
            let fname = e.file_name().to_string_lossy().into_owned();
            if fname.starts_with(&prefix) && fname.ends_with(".a") {
                fname
                    .strip_prefix("lib")
                    .and_then(|f| f.strip_suffix(".a"))
                    .map(str::to_string)
            } else {
                None
            }
        })
    }
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
