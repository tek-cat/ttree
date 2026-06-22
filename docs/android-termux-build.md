# Building ttree for Android / Termux (aarch64)

ttree is built to be driven from a phone over Termux + SSH, so it needs to run *on*
the phone. This documents how to produce a working Termux binary, and, just as
importantly, the approaches that **don't** work, so nobody re-walks the same wall.

## TL;DR

```sh
scripts/build-android.sh --deploy          # builds + installs to ssh host "phone"
```

Produces a native **`aarch64-linux-android`** (Bionic) PIE binary, strips it, and
`scp`s it to `$PREFIX/bin/ttree` on the phone. Requires Android NDK r28
(`ANDROID_NDK_HOME` or `~/android_sdk/ndk/*`) and `rustup`.

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

## The one blocker on the Android target: `termios 0.2.2`

`portable-pty 0.8.1` depends (unconditionally, on unix) on
`serial → serial-unix → termios 0.2.2`. `termios 0.2.2`'s `src/os/mod.rs` only has
arms for linux/macos/freebsd/openbsd (**no `target_os = "android"`**), so it fails:

```
error[E0433]: could not find `target` in `os`
```

`ioctl-rs` (the sibling dep) already handles android; only `termios` is missing it.

### Fix: vendored shim, via `[patch.crates-io]`

`third_party/termios/` is a copy of `termios 0.2.2` with one change in
`src/os/mod.rs`: android is mapped to the existing `linux` module.

```rust
#[cfg(any(target_os = "linux", target_os = "android"))] pub use self::linux as target;
#[cfg(any(target_os = "linux", target_os = "android"))] pub mod linux;
```

`Cargo.toml` wires it in:

```toml
[patch.crates-io]
termios = { path = "third_party/termios" }
```

**Why this is safe:** ttree only uses portable-pty's PTY path, never its serial-port
path, so all of `serial`/`termios` is dead code (and `--gc-sections` drops it). The
linux struct layout is wrong for Bionic (`NCCS` 32 vs 19, etc.), but since the code
is never called, it never matters at runtime. It only needs to *compile*. This has
no effect on native Linux/macOS builds: `[patch]` for the android cross-build aside,
those still use the crates.io `termios`.

## Toolchain details

- `rustup target add aarch64-linux-android` (prebuilt std, **no** `-Zbuild-std`).
- Linker = NDK clang wrapper `aarch64-linux-android24-clang` (min API 24).
  Set via `CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER` (and `CC_*`/`AR_*` for safety).
- No C dependencies in the tree, so nothing else from the NDK is needed.

## Deploy

```sh
scp target/aarch64-linux-android/release/ttree phone:~/ttree.new
ssh phone 'mv ~/ttree.new $PREFIX/bin/ttree && chmod +x $PREFIX/bin/ttree'
```

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
