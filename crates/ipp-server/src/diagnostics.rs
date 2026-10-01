//! Native configuration and stderr sink. Embedders can configure core directly.

use std::cell::Cell;
use std::fmt;
use std::io::{self, Write};

use ipp_core::diagnostics::{Level, configure};

thread_local! {
    static SESSION: Cell<u64> = const { Cell::new(0) };
}

/// Read IPP_LOG before announcing startup readiness. Absence selects info.
/// Invalid and non-Unicode values fail without exposing the supplied value.
pub fn level_from_env() -> io::Result<Level> {
    match std::env::var("IPP_LOG") {
        Ok(value) => Level::parse(&value).ok_or_else(invalid_level),
        Err(std::env::VarError::NotPresent) => Ok(Level::Info),
        Err(std::env::VarError::NotUnicode(_)) => Err(invalid_level()),
    }
}

fn invalid_level() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "IPP_LOG must be off, error, warn, info, debug, or trace",
    )
}

/// Install host-owned output on the calling session thread, and the panic hook
/// that reports a panic through it.
pub fn install(level: Level, session: u64) {
    SESSION.set(session);
    configure(level, Some(stderr));
    ipp_core::diagnostics::install_panic_hook();
}

fn stderr(level: Level, line: fmt::Arguments<'_>) {
    // Diagnostics must not turn a closed stderr into a runtime failure.
    let _ = writeln!(
        io::stderr().lock(),
        "[{}] [session={}] {line}",
        level.as_str(),
        SESSION.get()
    );
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::fmt;

    use ipp_core::diagnostics::{Level, configure};

    thread_local! {
        static LINES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    fn capture(_level: Level, line: fmt::Arguments<'_>) {
        LINES.with_borrow_mut(|lines| lines.push(line.to_string()));
    }

    #[test]
    fn panic_hook_logs_message_and_location_through_the_session_sink() {
        super::install(Level::Error, 7);
        configure(Level::Error, Some(capture));

        let line = line!() + 1;
        let result = std::panic::catch_unwind(|| panic!("forced server panic"));
        configure(Level::Off, None);

        assert!(result.is_err());
        let lines = LINES.take();
        let expected =
            format!("panic at crates/ipp-server/src/diagnostics.rs:{line}:50: forced server panic");
        println!("{}", lines.join("\n"));
        assert_eq!(lines, [expected]);
    }
}
