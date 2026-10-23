#!/usr/bin/env bash
set -euo pipefail
PROJECT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
BINARY="$PROJECT_DIR/dist/usage-report"
SCAN_ROOT="$(getent passwd "$(id -u)" | cut -d: -f6)"
DEMO_DIR="$PROJECT_DIR/var/demo"
test -t 0 && test -t 1
test -x "$BINARY"
umask 077
mkdir -p "$DEMO_DIR"
if [ ! -s "$DEMO_DIR/index.db" ]; then
    "$BINARY" scan --unprivileged --root "$SCAN_ROOT" --db "$DEMO_DIR/index.db"
fi
"$BINARY" serve --unprivileged --root "$SCAN_ROOT" --db "$DEMO_DIR/index.db" \
    --socket "$DEMO_DIR/report.sock" &
DEMO_PID=$!
cleanup() {
    kill -TERM "$DEMO_PID" 2>/dev/null || true
    wait "$DEMO_PID" 2>/dev/null || true
}
trap cleanup EXIT
trap 'exit 130' INT TERM
for attempt in {1..50}; do
    if "$BINARY" --socket "$DEMO_DIR/report.sock" --status >/dev/null 2>&1; then break; fi
    kill -0 "$DEMO_PID"
    if [ "$attempt" -eq 50 ]; then printf '演示服务启动超时\n' >&2; exit 1; fi
    sleep 0.1
done
"$BINARY" "$(id -u)" --socket "$DEMO_DIR/report.sock"
