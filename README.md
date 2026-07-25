# dwarfs-rs

Rust bindings to [DwarFS](https://github.com/tamatebako/dwarfs-t) — a fast
high-compression **read-only** file system — via its stable C ABI.

This repository is **standalone from day one**: it is not tebako-specific,
it is not part of any other project, and it tracks
[dwarfs-t](https://github.com/tamatebako/dwarfs-t) releases. It is intended
to be published to crates.io eventually; today it is consumed as a git or
path dependency.

## Layers

```
┌─────────────────────────────────────────────────────────────┐
│ consumers (your crate, tebako-rs, any Rust/FFI consumer)    │
├─────────────────────────────────────────────────────────────┤
│ dwarfs        — safe, idiomatic Rust API (this repo)        │
│                 Result-based errors, owned types, iterators │
├─────────────────────────────────────────────────────────────┤
│ dwarfs-sys    — raw extern "C" declarations (this repo)     │
│                 + build.rs that compiles the native library │
├─────────────────────────────────────────────────────────────┤
│ libdwarfs_c   — the STABLE C ABI (16 functions, dwarfs_c_*) │
│                 lives in dwarfs-t: include/dwarfs_c.h       │
├─────────────────────────────────────────────────────────────┤
│ dwarfs-t      — the C++20 DwarFS reader (filesystem_v2 &    │
│                 co.), statically absorbed inside the C ABI  │
└─────────────────────────────────────────────────────────────┘
```

The C ABI is the boundary that keeps Rust consumers away from C++ headers,
templates and ABI-fragile internals; rebuilding the native side does not
change what Rust sees.

## Crates

| crate | purpose |
|---|---|
| [`dwarfs-t`](dwarfs/) | Safe wrapper: `Filesystem::open` / `open_memory` / `open_region`, `stat`, `pread`, `read_dir` iterator, `image_info_json`, `DwarfsError` mapping the errno channel. |
| [`dwarfs-t-sys`](dwarfs-sys/) | Hand-written `extern "C"` declarations for the 16-function C ABI (hand-written because the surface is small and frozen — no bindgen/libclang needed; an `abi_check.c` with `_Static_assert`s pins the layout at build time) plus the vendored-source build. |

## Requirements

- Rust (stable, 1.74+)
- CMake, a C++20 compiler, and ninja (or make)
- **vcpkg** — the native dependency chain (flatbuffers, xxhash, zstd, lz4,
  liblzma, brotli, boost, fmt, openssl, ...) is built by dwarfs-t's own
  vcpkg manifest through its overlay ports.

## Building

```console
$ git submodule update --init            # vendored dwarfs-t (pinned ref)
$ export DWARFS_RS_VCPKG_ROOT=/path/to/vcpkg
$ cargo test --workspace
```

The first build compiles the whole native dep chain (vcpkg) and then
`libdwarfs_c` — expect tens of minutes on a cold vcpkg binary cache, a few
minutes on a warm one. Later builds are incremental.

### Environment knobs (dwarfs-sys build.rs)

| variable | default | meaning |
|---|---|---|
| `DWARFS_RS_VCPKG_ROOT` (or `VCPKG_ROOT`) | — (required) | vcpkg installation root |
| `DWARFS_RS_VCPKG_TRIPLET` | derived from the Rust target (e.g. `arm64-osx-static`, `x64-linux-static`) | vcpkg triplet to build against |
| `DWARFS_RS_DWARFS_T_SOURCE` | the `dwarfs-t` submodule | path to a dwarfs-t checkout |
| `DWARFS_RS_CMAKE_BUILD_TYPE` | `Release` | CMake build type |
| `DWARFS_RS_VERBOSE` | unset | set to stream CMake output |

### Feature flags

| feature | default | meaning |
|---|---|---|
| `vendored` | ✔ | Build `libdwarfs_c` and its whole static dependency closure from the vendored dwarfs-t source (git submodule, pinned ref) via CMake/vcpkg. **The only mode in v1.** |
| `system` | — | (planned) Link a prebuilt `libdwarfs_c` (e.g. discovered via pkg-config) instead of building from source. |

## Usage

```rust,no_run
use dwarfs::{Filesystem, FileType};

fn main() -> Result<(), dwarfs::DwarfsError> {
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

## Platform support

| platform | status |
|---|---|
| macOS (arm64, x86_64) | supported, tested in CI |
| Linux (x86_64, aarch64, gnu) | supported, tested in CI |
| Windows (MSVC) | **not yet** — dwarfs-t's `*-windows-static` triplets use the static CRT (/MT), which mismatches Rust's default dynamic CRT (/MD); needs a dedicated triplet/CRT story |

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

## Why dwarfs-t-rs (and not dwarfs-rs)

This binding targets **dwarfs-t**, tamatebako's fork of DwarFS (by Marcus
Holland-Moritz), and not upstream DwarFS:

- dwarfs-t adds an additional FlatBuffers-based image format that upstream
  DwarFS cannot read (a real format divergence, not just a patch series).
- `libdwarfs` / `libdwarfs_c` (the supported library and C ABI this crate
  binds) exist only in dwarfs-t — upstream ships tools, not a library.
- The C++ side removes the Facebook folly/thrift dependency (the "t" fork's
  original purpose); DwarFS itself is mhx's project, not Facebook's.
