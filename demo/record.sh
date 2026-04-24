#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "==> Building ttree release binary..."
cd ..
cargo build --release
cd demo

echo "==> Running VHS tape..."
vhs demo.tape

echo ""
echo "Done! Output files:"
ls -lh demo.mp4 demo.gif 2>/dev/null
