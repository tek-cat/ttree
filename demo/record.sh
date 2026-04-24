#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

cleanup() {
    echo "==> Cleaning up demo sessions..."
    tmux -L ttree-demo kill-server 2>/dev/null || true
}
trap cleanup EXIT

echo "==> Building ttree release binary..."
cd ..
cargo build --release
cd demo

echo "==> Running VHS tape..."
vhs demo.tape

echo ""
echo "Done! Output files:"
ls -lh demo.mp4 demo.gif 2>/dev/null
