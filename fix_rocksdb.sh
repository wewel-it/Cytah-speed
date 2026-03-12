#!/usr/bin/env bash
# Script to patch RocksDB headers in the target directory when the
# "cstdint file not found" or similar issues occur during build in
# Codespaces/Linux environments.
#
# Usage: run from the repository root before `cargo build` or invoke it
# automatically in CI after a failed build.

set -euo pipefail

# ensure build environment variables are set for header locations
export CPATH="/usr/include/c++/13:$CPATH"
export CPLUS_INCLUDE_PATH="/usr/include/c++/13:$CPLUS_INCLUDE_PATH"
export BINDGEN_EXTRA_CLANG_ARGS="-I/usr/include/c++/13 $BINDGEN_EXTRA_CLANG_ARGS"

echo "Patching RocksDB headers under target/ to include <stdint.h> if needed..."

# search for headers (rocksdb/c.h is common but we include others just in case)
find target -type f -name "*.h" | grep -i rocksdb | while IFS= read -r file; do
    if ! grep -q "#include <stdint.h>" "$file"; then
        echo "Inserting include into $file"
        # insert at top of file
        sed -i '1i#include <stdint.h>' "$file"
    fi
    if ! grep -q "#include <cstdint>" "$file"; then
        echo "Inserting <cstdint> include into $file"
        sed -i '1i#include <cstdint>' "$file"
    fi
done

echo "Patch complete. You may retry cargo build."
