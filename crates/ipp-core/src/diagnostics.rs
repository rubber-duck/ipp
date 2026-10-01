//! Synchronous diagnostics compiled into every build. Configure each world-owning
//! host thread; core starts off with no sink.
//! Sinks must consume borrowed arguments before returning and must not reenter
//! world mutation. Recursive diagnostics are suppressed, including during format.

use std::cell::Cell;
use std::fmt;
use std::panic::PanicHookInfo;
use std::sync::Once;

/// Ordered verbosity; Off never emits a record.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(u32)]
pub enum Level {
    /// Disable all diagnostics.
    Off = 0,
    /// Fatal session or subsystem failures.
    Error = 1,
    /// Rejected work and recoverable unexpected conditions.
    Warn = 2,
    /// WorldSession and subsystem lifecycle transitions.
    Info = 3,
    /// Command boundaries and committed entity effects.
    Debug = 4,
    /// Explicitly enabled bounded diagnostic detail.
    Trace = 5,
}

impl Level {
    /// Stable lowercase host configuration and output name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }

    /// Decode the host bridge's stable numeric level.
    pub const fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Off),
            1 => Some(Self::Error),
            2 => Some(Self::Warn),
            3 => Some(Self::Info),
            4 => Some(Self::Debug),
            5 => Some(Self::Trace),
            _ => None,
        }
    }

    /// Parse an exact lowercase host configuration name.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "off" => Some(Self::Off),
            "error" => Some(Self::Error),
            "warn" => Some(Self::Warn),
            "info" => Some(Self::Info),
            "debug" => Some(Self::Debug),
            "trace" => Some(Self::Trace),
            _ => None,
        }
    }
}

/// A synchronous host callback; arguments cannot escape this call.
pub type Sink = fn(Level, fmt::Arguments<'_>);

thread_local! {
    static CONFIG: Cell<(Level, Option<Sink>)> = const { Cell::new((Level::Off, None)) };
    static EMITTING: Cell<bool> = const { Cell::new(false) };
}

/// Replace the calling thread's threshold and sink. Core starts off with no sink.
pub fn configure(level: Level, sink: Option<Sink>) {
    CONFIG.set((level, sink));
}

/// Check before constructing arguments or allocating diagnostic details.
pub fn enabled(level: Level) -> bool {
    let (threshold, sink) = CONFIG.get();
    level != Level::Off && level <= threshold && sink.is_some() && !EMITTING.get()
}

struct Emitting;

impl Drop for Emitting {
    fn drop(&mut self) {
        EMITTING.set(false);
    }
}

/// Emit borrowed arguments without retaining a TLS borrow across the callback.
#[doc(hidden)]
pub fn emit(level: Level, arguments: fmt::Arguments<'_>) {
    if !enabled(level) {
        return;
    }

    let (_, sink) = CONFIG.get();
    EMITTING.set(true);
    let _guard = Emitting;
    if let Some(sink) = sink {
        sink(level, arguments);
    }
}

/// Install a process panic hook that writes the panic message and source
/// location to the panicking thread's sink at error level, then runs the
/// previous hook. Hosts install it with their sink. The line needs no symbols,
/// so a stripped `panic = "abort"` build reports it before aborting.
pub fn install_panic_hook() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            log_panic(info);
            previous(info);
        }));
    });
}

fn log_panic(info: &PanicHookInfo<'_>) {
    // A panic during thread teardown finds no configuration to read.
    if CONFIG.try_with(Cell::get).is_err() || EMITTING.try_with(Cell::get).is_err() {
        return;
    }

    let message = info.payload_as_str().unwrap_or("non-string panic payload");
    match info.location() {
        Some(location) => emit(
            Level::Error,
            format_args!(
                "panic at {}:{}:{}: {message}",
                location.file(),
                location.line(),
                location.column()
            ),
        ),
        None => emit(Level::Error, format_args!("panic: {message}")),
    }
}
