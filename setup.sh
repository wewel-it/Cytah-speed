#!/usr/bin/env bash
# prepare environment for building the Cytah-speed project
# install required C/C++ toolchain for native crates like rocksdb

set -euo pipefail

# prefer to run as root in container, sudo may not be available or necessary
if [[ $EUID -ne 0 ]]; then
    echo "re-running as root..."
    exec sudo bash "$0" "$@"
fi

# update package list (ignore errors from unsigned repos)
apt-get update || true

# install compile dependencies
apt-get install -y build-essential clang cmake pkg-config libssl-dev
# When building on Codespaces the rocksdb headers often lack <cstdint>
# which results in errors such as "uint64_t does not name a type".
# Patch the downloaded crate next time the registry is populated.
echo "Patching rocksdb headers to include <cstdint>..."
if [ -d "$HOME/.cargo/registry/src" ]; then
    find "$HOME/.cargo/registry/src" \
        -path "*librocksdb-sys-*/rocksdb/*" -name '*.h' ! -name 'c.h' \
        -exec sed -i '1i#include <cstdint>' {} + || true
fi

# Cargo and Rust should already be installed in Codespaces dev container
# but we ensure C++ compiler is available

echo "system dependencies installed. You can now run 'cargo build'."