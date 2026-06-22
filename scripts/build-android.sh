#!/usr/bin/env bash
#
# Cross-compile ttree for Android / Termux (aarch64, native Bionic PIE).
# Full rationale and the dead-ends we ruled out: docs/android-termux-build.md
#
# Usage:
#   scripts/build-android.sh            # build release binary
#   scripts/build-android.sh --deploy   # build, strip, scp+install to ssh host "phone"
#
set -euo pipefail
cd "$(dirname "$0")/.."

API=24                       # min Android API; binary runs on this level and newer
TARGET=aarch64-linux-android

# Locate the NDK (r28 known-good). Override with ANDROID_NDK_HOME.
NDK_ROOT="${ANDROID_NDK_HOME:-$(ls -d "$HOME"/android_sdk/ndk/* 2>/dev/null | sort -V | tail -1)}"
NB="$NDK_ROOT/toolchains/llvm/prebuilt/linux-x86_64/bin"
LINKER="$NB/aarch64-linux-android${API}-clang"
if [ ! -x "$LINKER" ]; then
  echo "error: NDK linker not found at $LINKER" >&2
  echo "       install Android NDK r28 or set ANDROID_NDK_HOME" >&2
  exit 1
fi

rustup target add "$TARGET" >/dev/null 2>&1 || true

# Pure-Rust deps, so only the linker (and AR for any build scripts) need to be the NDK's.
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$LINKER"
export CC_aarch64_linux_android="$LINKER"
export AR_aarch64_linux_android="$NB/llvm-ar"

echo ">> cargo build --release --target $TARGET"
cargo build --release --target "$TARGET"

BIN="target/$TARGET/release/ttree"
"$NB/llvm-strip" "$BIN" 2>/dev/null || true
echo ">> built: $BIN"
file "$BIN"

if [ "${1:-}" = "--deploy" ]; then
  HOST="${2:-phone}"
  PREFIX_BIN=/data/data/com.termux/files/usr/bin/ttree
  echo ">> deploying to $HOST"
  scp "$BIN" "$HOST:~/ttree.new"
  ssh "$HOST" "sh -c 'mv ~/ttree.new $PREFIX_BIN && chmod +x $PREFIX_BIN && echo installed: \$(command -v ttree)'"
fi
