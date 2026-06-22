//! OS-specific definitions.

// Android shim (ttree): bionic's termios layout differs from the linux struct,
// but ttree never exercises portable-pty's serial path, so this dead code only needs
// to *compile* for aarch64-linux-android. Map android to the linux module.
#[cfg(any(target_os = "linux", target_os = "android"))] pub use self::linux as target;
#[cfg(target_os = "macos")] pub use self::macos as target;
#[cfg(target_os = "freebsd")] pub use self::freebsd as target;
#[cfg(target_os = "openbsd")] pub use self::openbsd as target;

#[cfg(any(target_os = "linux", target_os = "android"))] pub mod linux;
#[cfg(target_os = "macos")] pub mod macos;
#[cfg(target_os = "freebsd")] pub mod freebsd;
#[cfg(target_os = "openbsd")] pub mod openbsd;
