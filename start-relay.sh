#!/usr/bin/env bash
# Linux counterpart of start-relay.cmd: build when Cargo is present, then run.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")"
if [[ -x ./sa-relay ]]; then
    exec ./sa-relay "$@"
fi
if command -v cargo >/dev/null 2>&1; then
    cargo build --manifest-path native/Cargo.toml --release -p sa-net --bin sa-relay
fi
if [[ ! -x native/target/release/sa-relay ]]; then
    echo "sa-relay is missing. Install Rust/Cargo (docs/linux.md) or use the packaged client." >&2
    exit 1
fi
exec native/target/release/sa-relay "$@"
