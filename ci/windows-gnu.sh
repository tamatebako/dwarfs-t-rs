#!/usr/bin/env bash
# ci/windows-gnu.sh — the vendored windows-gnu (ucrt64) leg, end to end:
# cargo build, DLL-import forensics, then the serialized test run.
#
# Everything the leg needs is HERE, not inline in the workflow YAML —
# run-blocks get string-edited and break silently; a script is reviewed
# and shellcheck-able. The workflow only exports the environment and
# calls this file.
set -euo pipefail

export PATH="/c/mingw64/bin:/c/Users/runneradmin/.cargo/bin:$PATH"

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
exec cargo test --workspace --target x86_64-pc-windows-gnu -- "$SERIAL" --nocapture
