# dwarfs-t-rs

[![crates.io](https://img.shields.io/crates/v/dwarfs-t.svg)](https://crates.io/crates/dwarfs-t)
[![docs.rs](https://docs.rs/dwarfs-t/badge.svg)](https://docs.rs/dwarfs-t)
[![CI](https://github.com/tamatebako/dwarfs-t-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/tamatebako/dwarfs-t-rs/actions/workflows/ci.yml)

Rust bindings to [DwarFS](https://github.com/tamatebako/dwarfs-t) — a fast
high-compression **read-only** file system — via its stable C ABI
(`libdwarfs_c`): mount and read images, and **create** them fully in-process
(no `mkdwarfs` subprocess, no shell, no PATH dependency).

This repository is **standalone from day one**: it is not tebako-specific,
it is not part of any other project, and it tracks
[dwarfs-t](https://github.com/tamatebako/dwarfs-t) releases. It is published
to crates.io as [`dwarfs-t`](https://crates.io/crates/dwarfs-t) and
[`dwarfs-t-sys`](https://crates.io/crates/dwarfs-t-sys).

## Layers

```
┌─────────────────────────────────────────────────────────────┐
│ consumers (your crate, tebako-rs, any Rust/FFI consumer)    │
├─────────────────────────────────────────────────────────────┤
│ dwarfs-t      — safe, idiomatic Rust API (this repo)        │
│                 Result-based errors, owned types, iterators │
├─────────────────────────────────────────────────────────────┤
│ dwarfs-t-sys  — raw extern "C" declarations (this repo)     │
│                 + build.rs that compiles the native library │
├─────────────────────────────────────────────────────────────┤
│ libdwarfs_c   — the STABLE C ABI (22 functions, dwarfs_c_*)  │
│                 lives in dwarfs-t: include/dwarfs_c.h       │
├─────────────────────────────────────────────────────────────┤
│ dwarfs-t      — the C++20 DwarFS reader/writer (filesystem_ │
│                 v2 & co.), statically absorbed in the C ABI │
└─────────────────────────────────────────────────────────────┘
```

The C ABI is the boundary that keeps Rust consumers away from C++ headers,
templates and ABI-fragile internals; rebuilding the native side does not
change what Rust sees.

## Crates

| crate | purpose |
|---|---|
| [`dwarfs-t`](dwarfs/) | Safe wrapper: `Filesystem::open` / `open_memory` / `open_region`, `stat`, `pread`, `read_dir` iterator, `image_info_json`, `Writer` (in-process image creation), `DwarfsError` mapping the errno channel. |
| [`dwarfs-t-sys`](dwarfs-t-sys/) | Hand-written `extern "C"` declarations for the 22-function C ABI (hand-written because the surface is small and frozen — no bindgen/libclang needed; an `abi_check.c` with `_Static_assert`s pins the layout at build time) plus the vendored-source build. |

## Requirements

- Rust (stable, 1.74+)
- For the default `vendored` feature only: CMake, a C++20 compiler, and
  ninja (or make)
- **vcpkg** — the native dependency chain (flatbuffers, xxhash, zstd, lz4,
  liblzma, brotli, boost, fmt, openssl, ...) is built by dwarfs-t's own
  vcpkg manifest through its overlay ports.

The no-default-features [skeleton](#feature-flags) needs none of the native
toolchain.

## Building

### From a git checkout

```console
$ git submodule update --init            # vendored dwarfs-t (pinned ref)
$ export DWARFS_RS_VCPKG_ROOT=/path/to/vcpkg
$ cargo test --workspace
```

The first build compiles the whole native dep chain (vcpkg) and then
`libdwarfs_c` — expect tens of minutes on a cold vcpkg binary cache, a few
minutes on a warm one. Later builds are incremental.

### From crates.io

The crates.io packages do **not** bundle the dwarfs-t sources (the C++
tree is far over the package size limit). With the default `vendored`
feature you therefore need your own dwarfs-t checkout:

```console
$ export DWARFS_RS_DWARFS_T_SOURCE=/path/to/dwarfs-t
$ export DWARFS_RS_VCPKG_ROOT=/path/to/vcpkg
$ cargo build            # in your project, with dwarfs-t as a dependency
```

If you only need the crate to compile — e.g. your DwarFS support is behind
an optional feature of your own — use the skeleton instead:

```toml
[dependencies]
dwarfs-t = { version = "0.1", default-features = false }
```

### Environment knobs (dwarfs-t-sys build.rs)

| variable | default | meaning |
|---|---|---|
| `DWARFS_RS_VCPKG_ROOT` (or `VCPKG_ROOT`) | — (required for `vendored`) | vcpkg installation root |
| `DWARFS_RS_VCPKG_TRIPLET` | derived from the Rust target (e.g. `arm64-osx-static`, `x64-linux-static`, `x64-windows-static-md`) | vcpkg triplet to build against |
| `DWARFS_RS_DWARFS_T_SOURCE` | the `dwarfs-t` submodule (git checkout only) | path to a dwarfs-t checkout |
| `DWARFS_RS_CMAKE_BUILD_TYPE` | `Release` | CMake build type |
| `DWARFS_RS_VERBOSE` | unset | set to stream CMake output |

### Feature flags

| feature | default | meaning |
|---|---|---|
| `vendored` | ✔ | Build `libdwarfs_c` and its whole static dependency closure from dwarfs-t sources (git submodule, pinned ref; or `DWARFS_RS_DWARFS_T_SOURCE` from crates.io) via CMake/vcpkg. **The only native mode in v1.** |
| *(none)* | — | `--no-default-features` builds the pure-cargo **skeleton**: nothing native is built or linked, the crate always compiles, and every operation fails at runtime with `ENOTSUP` ([`ErrorKind::NotSupported`](https://docs.rs/dwarfs-t)). This lets consumers gate the DwarFS backend behind their own optional feature and build either way. |
| `system` | — | (planned) Link a prebuilt `libdwarfs_c` (e.g. discovered via pkg-config) instead of building from source. |

**docs.rs** cannot run CMake/vcpkg. The `dwarfs-t-sys` build script detects
the `DOCS_RS` environment variable and skips the native build there
(rustdoc never links it), so `cargo doc` and the published documentation
always work.

## Usage

```rust,no_run
use dwarfs_t::{Filesystem, FileType};

fn main() -> Result<(), dwarfs_t::DwarfsError> {
    let fs = Filesystem::open("image.dwarfs")?;

    let meta = fs.stat("format.sh")?;
    assert_eq!(meta.file_type, FileType::Regular);

    let mut buf = vec![0u8; meta.size as usize];
    let n = fs.pread("format.sh", &mut buf, 0)?;
    buf.truncate(n);

    for entry in fs.read_dir("/")? {
        let entry = entry?;
        println!("{:?} {}", entry.file_type, entry.name);
    }

    println!("{}", fs.image_info_json()?);
    Ok(())
}
```

`Filesystem` is `Send + Sync`: concurrent `stat`/`pread`/`read_dir` calls
on the same handle are safe (the underlying reader is thread-safe for
reads, and each directory iterator is independent state).

## Writing images (in-process)

The same ABI also *creates* images — no `mkdwarfs` binary, no shell, no
PATH dependency anywhere. Single-shot discipline: create a `Writer`, add
content, `write()` (which consumes the writer); dropping always releases
the native handle.

```rust,no_run
use dwarfs_t::{Compression, Filesystem, Writer, WriterOptions};

fn main() -> Result<(), dwarfs_t::DwarfsError> {
    // WriterOptions::default() is the mkdwarfs defaults profile (zstd
    // blocks, 16 MiB block size, similarity ordering, categorizers off,
    // one worker per CPU).
    let mut w = Writer::new(WriterOptions::default())?;
    w.add_tree("app/", "/")?;            // the mkdwarfs -i equivalent
    w.write("app.dwarfs")?;              // consumes w; never overwrites

    // ...and read it straight back through the same crate:
    let fs = Filesystem::open("app.dwarfs")?;
    assert!(fs.stat("hello.txt").is_ok());

    // Custom compression (zstd/lzma/brotli/none + algo-native level):
    let mut fast = Writer::with_compression(Compression::Zstd, Some(3))?;
    fast.add_tree("app/", "/")?;
    fast.write("app-fast.dwarfs")?;
    Ok(())
}
```

v1 source rules (enforced by the ABI, surfaced as `DwarfsError`): the
writer is single-source — one `add_tree` XOR `add_file`s sharing one
directory, placed at the image root by basename; arbitrary
prefixes/renames are rejected with `EINVAL`. `write` never overwrites
(`EEXIST`) and a writer cannot be written twice (`EALREADY`).

**Determinism:** output bytes are *not* run-to-run deterministic (the
image history records creation timestamps); `num_workers` affects only
throughput, never the layout.

## Platform support

| platform | status |
|---|---|
| macOS (arm64, x86_64) | supported, tested in CI |
| Linux (x86_64, aarch64, gnu) | supported, tested in CI |
| Windows (MSVC) | skeleton tested in CI; vendored build wired (CRT strategy below) but **not yet CI-proven** |
| Windows (gnullvm, ucrt64) | skeleton builds; vendored build wired via `x64-mingw-static`, likewise unproven |

(The pure-cargo skeleton builds on any platform, including Windows — it
contains no native code at all.)

### Windows and the CRT (/MD vs /MT)

Rust links the **dynamic CRT (/MD)** by default on `*-windows-msvc`, so the
default vcpkg triplets are the `*-windows-static-md` variants (static
libraries, dynamic CRT) — not `*-windows-static`, which is /MT and would
mismatch at link time. The `-md` triplets are upstream vcpkg community
triplets (no overlay file needed; dwarfs-t also ships matching
`x64-windows-static-md` CMake presets).

Forcing the static CRT on the Rust side (`RUSTFLAGS=-C
target-feature=+crt-static`, /MT) against the default /MD triplet is refused
by the build script with a named error; opting into /MT requires an explicit
/MT triplet too (`DWARFS_RS_VCPKG_TRIPLET=x64-windows-static`, resp.
`arm64-windows-static`). The build script warns on the remaining mismatched
combinations.

For `x86_64-pc-windows-gnullvm` (llvm-mingw, UCRT — the ucrt64-class target
of the C++ line) the default triplet is `x64-mingw-static`, the triplet the
C++ libtfs builds used for windows-ucrt64; CRT linkage there is dynamic
against the UCRT, matching Rust's gnullvm targets. dwarfs-t's overlay already
ships `x64-mingw-static.cmake`, so no new overlay triplet is needed.

A vendored Windows CI leg is deliberately absent until the vendored build is
proven on a Windows runner; the skeleton leg covers check/clippy/test for
`x86_64-pc-windows-msvc`.

## Version policy

Both crates version together (`0.x`) and **track dwarfs-t releases**: each
release of this repo pins a specific dwarfs-t ref in the git submodule (the
`DWARFS_RS_DWARFS_T_SOURCE` override accepts any compatible checkout). The
C ABI itself is frozen; while the crates are at `0.x`, minor bumps may
still change the safe Rust API.

## License

- The **Rust sources** in this repository are licensed under
  [MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE), at your option.
- The **native library** these bindings link — DwarFS / dwarfs-t, including
  `libdwarfs_c` and its statically linked dependency closure — is licensed
  under **GPL-3.0**. This is stated plainly because it has teeth: with the
  default `vendored` feature the native code is linked **statically** into
  your binary, and distributing such a binary makes it a combined work
  subject to the GPL-3.0 (source disclosure, GPL-3.0-or-compatible
  licensing of the combination). If your project cannot accept those terms,
  do not distribute binaries that link this library. The `dwarfs_c.h` C ABI
  header itself carries the MIT license, but that does not change the
  license of the implementation you link.

  DwarFS's bundled third-party libraries carry their own (permissive)
  licenses; see the dwarfs-t repository for the full list.

  For context: this GPL-3.0 obligation is specific to the DwarFS backend.
  Other image backends used alongside it in the wider tebako ecosystem
  differ — the ZIP backend is a pure-Rust crate (MIT) and the SquashFS
  backend links libsquashfs from squashfs-tools-ng (LGPL-3.0-or-later) —
  so backend selection changes a consumer's license obligations. The
  no-default-features skeleton of this crate links no native code at all
  (it fails every operation with `ENOTSUP`), so a binary that only ships
  the skeleton is not combined with GPL-3.0 code by this crate.

## Why dwarfs-t-rs (and not dwarfs-rs)

This binding targets **dwarfs-t**, tamatebako's fork of DwarFS (by Marcus
Holland-Moritz), and not upstream DwarFS:

- dwarfs-t adds an additional FlatBuffers-based image format that upstream
  DwarFS cannot read (a real format divergence, not just a patch series).
- `libdwarfs` / `libdwarfs_c` (the supported library and C ABI this crate
  binds) exist only in dwarfs-t — upstream ships tools, not a library.
- The C++ side removes the Facebook folly/thrift dependency (the "t" fork's
  original purpose); DwarFS itself is mhx's project, not Facebook's.
