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

/// Install host-owned output on the calling session thread.
pub fn install(level: Level, session: u64) {
    SESSION.set(session);
    configure(level, Some(stderr));
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
