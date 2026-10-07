#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")"
if command -v cargo >/dev/null 2>&1; then
    cargo build --manifest-path native/Cargo.toml --release -p sa-runtime
fi
if [[ ! -x native/target/release/sa-runtime ]]; then
    echo "Install Rust/Cargo and the native dependencies described in docs/linux.md."
    exit 1
fi
if [[ -n "${GTA_SA_DIR:-}" ]]; then
    exec native/target/release/sa-runtime --renderer vulkan --game-dir "$GTA_SA_DIR" "$@"
fi
exec native/target/release/sa-runtime --renderer vulkan "$@"
