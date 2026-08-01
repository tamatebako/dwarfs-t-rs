#!/usr/bin/env bash
# ci/windows-gnu.sh — the vendored windows-gnu (ucrt64) leg, end to end:
# cargo build, DLL-import forensics, then the serialized test run.
#
# Everything the leg needs is HERE, not inline in the workflow YAML —
# run-blocks get string-edited and break silently; a script is reviewed
# and shellcheck-able. The workflow only exports the environment and
# calls this file.
set -euo pipefail

# The factory's proven PATH: ucrt64 gcc first; Git's /usr/bin for
# coreutils (safe — the ABI clash is specifically setup-msys2's
# /usr/bin, which stays OFF); git.exe from Git's /cmd; no choco mingw.
export PATH="/d/a/_temp/msys64/ucrt64/bin:/c/Program Files/Git/usr/bin:/c/Program Files/Git/cmd:/c/Users/runneradmin/.cargo/bin:/c/Windows/System32"

# The serialize note: a parallel harness hides which test crashes
# (windows-gnu legs died twice to a hidden segfault before this rule).
SERIAL="--test-threads=1"

# --- 1. build ---------------------------------------------------------------
cargo build --workspace --target x86_64-pc-windows-gnu
cargo test --workspace --target x86_64-pc-windows-gnu --no-run

# --- 2. DLL-import forensics ------------------------------------------------
# STATUS_ENTRYPOINT_NOT_FOUND fails the process BEFORE main, so the
# failure's own stderr never names the missing entry — enumerate every
# import up front instead.
for exe in target/x86_64-pc-windows-gnu/debug/deps/*.exe; do
  echo "=== imports: $exe ==="
  x86_64-w64-mingw32-objdump -p "$exe" \
    | grep -E "DLL Name|^\s+[0-9a-f]+\s+\S+\s*$" | head -60 || true
done

# --- 3. test (serialized) ---------------------------------------------------
if ! cargo test --workspace --target x86_64-pc-windows-gnu -- "$SERIAL" --nocapture; then
  # a segfault yields only STATUS_ACCESS_VIOLATION from the cargo harness —
  # rerun the crashing binary under gdb batch for the C++ backtrace.
  echo "=== test failed — rerunning the smoke binary under gdb ==="
  SMOKE_EXE="$(ls target/x86_64-pc-windows-gnu/debug/deps/smoke-*.exe | head -1)"
  gdb -batch -ex run -ex bt -ex "info registers" \
    --args "$SMOKE_EXE" --test-threads=1 || true
  exit 1
fi
