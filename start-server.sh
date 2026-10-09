#!/usr/bin/env bash
# Linux counterpart of start-server.cmd: build when Cargo is present, then run.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")"
if [[ -x ./sa-server ]]; then
    exec ./sa-server "$@"
fi
if command -v cargo >/dev/null 2>&1; then
    cargo build --manifest-path native/Cargo.toml --release -p sa-server
fi
if [[ ! -x native/target/release/sa-server ]]; then
    echo "sa-server is missing. Install Rust/Cargo (docs/linux.md) or use the packaged client." >&2
    exit 1
fi
exec native/target/release/sa-server "$@"
