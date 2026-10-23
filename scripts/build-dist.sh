#!/usr/bin/env bash
set -euo pipefail
PROJECT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cargo build --release --locked --manifest-path "$PROJECT_DIR/Cargo.toml" \
    --target-dir "$PROJECT_DIR/target"
install -d -m 755 "$PROJECT_DIR/dist"
install -m 755 "$PROJECT_DIR/target/release/usage-report" "$PROJECT_DIR/dist/usage-report.new"
mv -f "$PROJECT_DIR/dist/usage-report.new" "$PROJECT_DIR/dist/usage-report"
(
    cd "$PROJECT_DIR/dist"
    sha256sum usage-report > SHA256SUMS.new
    mv -f SHA256SUMS.new SHA256SUMS
    sha256sum -c SHA256SUMS
)
printf 'Distribution ready: %s/dist/usage-report\n' "$PROJECT_DIR"
