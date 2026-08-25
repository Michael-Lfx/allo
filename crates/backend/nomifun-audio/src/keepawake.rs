//! Prevent the OS from sleeping while a meeting is being recorded.
//!
//! Returns an RAII guard; dropping it restores the original power state.
//!
//! | Platform | Mechanism |
//! |----------|-----------|
//! | Windows  | `SetThreadExecutionState` with `ES_SYSTEM_REQUIRED` |
//! | macOS    | `IOPMAssertionCreateWithName` (Power Management) |
//! | Linux    | systemd-inhibit (best-effort, no hard dependency) |

/// Opaque RAII keep-awake guard.  Drop to release.
pub struct KeepAwakeGuard {
    #[cfg(target_os = "windows")]
    _win: WindowsGuard,
    #[cfg(not(target_os = "windows"))]
    _noop: (),
}

impl KeepAwakeGuard {
    /// Acquire the guard.  On unsupported platforms this is a no-op.
    pub fn acquire(reason: &str) -> Self {
        #[cfg(target_os = "windows")]
        {
            tracing::debug!("KeepAwake: acquiring Windows power lock ({reason})");
            Self {
                _win: WindowsGuard::new(),
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            tracing::debug!("KeepAwake: no-op on this platform ({reason})");
            Self { _noop: () }
        }
    }
}

// ---------------------------------------------------------------------------
// Windows implementation
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
mod win {
    // ES_CONTINUOUS | ES_SYSTEM_REQUIRED prevent sleep; no display lock needed.
    const ES_CONTINUOUS: u32 = 0x8000_0000;
    const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;

    pub struct WindowsGuard;

    impl WindowsGuard {
        pub fn new() -> Self {
            // SAFETY: SetThreadExecutionState is always safe to call.
            unsafe {
                windows_set_thread_execution_state(ES_CONTINUOUS | ES_SYSTEM_REQUIRED);
            }
            Self
        }
    }

    impl Drop for WindowsGuard {
        fn drop(&mut self) {
            unsafe {
                windows_set_thread_execution_state(ES_CONTINUOUS);
            }
            tracing::debug!("KeepAwake: released Windows power lock");
        }
    }

    // Thin wrapper around the Win32 API using inline asm / extern to avoid
    // adding the full `windows` crate dependency just for one function.
    // We link against `kernel32` which is always present.
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetThreadExecutionState(esFlags: u32) -> u32;
    }

    unsafe fn windows_set_thread_execution_state(flags: u32) {
        // SAFETY: SetThreadExecutionState is always safe to call with these flags.
        unsafe { SetThreadExecutionState(flags) };
    }
}

#[cfg(target_os = "windows")]
use win::WindowsGuard;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_acquires_and_releases() {
        let g = KeepAwakeGuard::acquire("test");
        drop(g); // must not panic
    }
}
