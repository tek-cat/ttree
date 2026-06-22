# Building ttree for Android / Termux (aarch64)

ttree is built to be driven from a phone over Termux + SSH, so it needs to run *on*
the phone. This documents how to produce a working Termux binary, and, just as
importantly, the approaches that **don't** work, so nobody re-walks the same wall.

## TL;DR

```sh
scripts/build-android.sh          # builds + strips the binary
```

Produces a native **`aarch64-linux-android`** (Bionic) PIE binary at
`target/aarch64-linux-android/release/ttree` and strips it. Requires Android NDK r28
(`ANDROID_NDK_HOME` or `~/android_sdk/ndk/*`) and `rustup`. To get it onto the phone,
use the prebuilt release download (see the README), or copy a locally-built binary to
`$PREFIX/bin/ttree`.

## Why cross-compile instead of building on the phone

The target device (Pixel 3a) has ~3.6 GB RAM (often <600 MB free, zram swap full)
and ~2 GB free storage. A from-scratch Rust build of ttree's dependency tree
(tokio + ratatui + portable-pty + …) realistically OOMs or runs out of disk there.
Cross-compiling on the laptop is the sane path.

## The target: native Bionic, not musl

Termux runs on Android's **Bionic** libc. The correct Rust target is
`aarch64-linux-android`, which emits a PIE with interpreter `/system/bin/linker64`.

### Dead end: `aarch64-unknown-linux-musl` (static)

A static musl binary seems attractive ("runs anywhere"), but it does **not** work in
Termux, and the failure is instructive:

1. **Plain static** → `ET_EXEC`. Termux/Android reject it:
   `has unexpected e_type: 2` (Android requires PIE / `ET_DYN`).
2. **Static-PIE** (custom target spec with `static-position-independent-executables:
   true`, built via `-Zbuild-std`) → links and is `ET_DYN`, but Bionic's loader
   rejects it: `executable's TLS segment is underaligned: alignment is 8, needs to be
   at least 64 for ARM64 Bionic`. musl emits `PT_TLS` `p_align=8`.
3. **Patch `PT_TLS` `p_align` → 64** → passes the loader check, then **segfaults**.
   Android loads `ET_DYN` executables via `linker64`, and the static musl runtime
   conflicts with Bionic doing the loading/TLS setup.

Conclusion: on Android, even a "static" PIE is loaded by Bionic, so it has to *be* a
Bionic binary. Use the `aarch64-linux-android` target.

## Dependencies: no special handling needed

There are no C dependencies, and nothing in the tree needs an android shim. ttree
uses `portable-pty` 0.9, whose serial backend is `serial2`, and that compiles cleanly
for `aarch64-linux-android`. A plain `cargo build --target aarch64-linux-android`
(with the NDK linker, below) just works, for cross-builds and for native Termux
builds alike. No patch, no fork, no vendored crate.

> **History (so nobody re-walks this).** portable-pty 0.8 pulled
> `serial → serial-unix → termios 0.2.2`, and termios 0.2.2 has no
> `target_os = "android"` arm (`error[E0433]: could not find target in os`), so it
> failed to compile for this target. We briefly vendored a patched termios under
> `third_party/termios/` via `[patch.crates-io]`. Upgrading to **portable-pty 0.9**
> (which swapped `serial` for `serial2`) dropped the termios dependency entirely and
> made the vendored fork unnecessary, so it was removed. If a future dependency
> reintroduces a crate without an android arm, prefer upgrading or replacing that
> crate over vendoring a patch.

## Toolchain details

- `rustup target add aarch64-linux-android` (prebuilt std, **no** `-Zbuild-std`).
- Linker = NDK clang wrapper `aarch64-linux-android24-clang` (min API 24).
  Set via `CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER` (and `CC_*`/`AR_*` for safety).
- No C dependencies in the tree, so nothing else from the NDK is needed.

## Installing on the phone

Get the binary onto the phone via the prebuilt release download, or by copying a
locally-built binary to `$PREFIX/bin/ttree`. See the README's "Run on Android /
Termux" section.

## Verifying it runs

- `file ttree` → `ELF 64-bit LSB pie executable, ARM aarch64, … interpreter
  /system/bin/linker64, for Android 24`.
- On the phone, `ttree --version` over a **non-PTY** SSH exec prints
  `No such device or address (os error 6)`: that is ttree correctly detecting it has
  no controlling terminal (`open("/dev/tty")` → `ENXIO`), **not** a build problem.
  Run it in a real terminal (a Termux session / tmux pane).

## Runtime caveat on low-memory devices

ttree's live preview spawns a PTY + vt100 parser per visible pane. On a
memory-starved phone (Pixel 3a, <600 MB free, swap full) launching ttree against a
busy tmux server can spike memory enough for Android's low-memory killer to take down
processes, including `sshd`, which shows up as `client_loop: send disconnect: Broken
pipe`. Mitigations: free RAM first (close apps), keep the pane count modest, and use
`mosh` instead of plain SSH so the session survives the kill/reconnect. See also the
swap discussion: more swap does not help here; RAM is the constraint.
