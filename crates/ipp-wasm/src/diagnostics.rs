//! Synchronous worker console bridge.

use std::cell::Cell;
use std::fmt;

use ipp_core::diagnostics::{Level, configure};

thread_local! {
    static SESSION: Cell<u64> = const { Cell::new(0) };
}

pub(crate) fn set_session(session: u64) {
    SESSION.set(session);
}

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "ipp_diagnostics")]
unsafe extern "C" {
    #[link_name = "write"]
    fn write(level: u32, ptr: u32, len: u32);
}

/// Set off/error/warn/info/debug/trace (0..=5). Invalid input preserves settings.
/// The host calls this before opening its first session; it also installs the
/// panic hook that reports a panic through the console before the abort.
/// Returns success=1.
// SAFETY: Unique symbol; configuration is thread-local and borrows no host memory.
#[unsafe(no_mangle)]
pub extern "C" fn ipp_diagnostics_set_level(level: u32) -> u32 {
    let Some(level) = Level::from_u32(level) else {
        return 0;
    };

    configure(level, Some(console));
    ipp_core::diagnostics::install_panic_hook();
    1
}

/// Testing fault that panics, so a test can observe the panic hook's line under
/// the shipped `panic = "abort"` profile.
// SAFETY: Unique symbol; it touches no worker state before panicking.
#[cfg(feature = "instrumentation")]
#[unsafe(no_mangle)]
pub extern "C" fn ipp_instrumentation_panic() {
    panic!("forced instrumentation panic");
}

fn console(_level: Level, _line: fmt::Arguments<'_>) {
    #[cfg(target_arch = "wasm32")]
    {
        let line = format!("[session={}] {_line}", SESSION.get());
        // SAFETY: The import synchronously borrows immutable UTF-8 bytes owned
        // by `line`, which remains live throughout the call. The host must not
        // mutate/retain this view or reenter exports; no mutable alias is exposed.
        unsafe {
            write(_level as u32, line.as_ptr() as u32, line.len() as u32);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_rejects_invalid_levels_without_changing_configuration() {
        for value in 0..=5 {
            assert_eq!(ipp_diagnostics_set_level(value), 1);
            assert_eq!(ipp_core::diagnostics::enabled(Level::Error), value != 0);
        }
        for value in [6, u32::MAX] {
            assert_eq!(ipp_diagnostics_set_level(value), 0);
            assert!(ipp_core::diagnostics::enabled(Level::Trace));
        }
        assert_eq!(ipp_diagnostics_set_level(0), 1);
        assert!(!ipp_core::diagnostics::enabled(Level::Error));
        configure(Level::Off, None);
    }

    thread_local! {
        static LINES: std::cell::RefCell<Vec<String>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }

    fn capture(_level: Level, line: fmt::Arguments<'_>) {
        LINES.with_borrow_mut(|lines| lines.push(line.to_string()));
    }

    #[test]
    fn panic_hook_installed_with_the_console_logs_message_and_location() {
        assert_eq!(ipp_diagnostics_set_level(1), 1);
        configure(Level::Error, Some(capture));

        let line = line!() + 1;
        let result = std::panic::catch_unwind(|| panic!("forced worker panic"));
        configure(Level::Off, None);

        assert!(result.is_err());
        let lines = LINES.take();
        println!("{}", lines.join("\n"));
        assert_eq!(
            lines,
            [format!(
                "panic at crates/ipp-wasm/src/diagnostics.rs:{line}:50: forced worker panic"
            )]
        );
    }
}
