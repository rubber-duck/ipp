//! Native WebSocket Host that presents its attached World on a GLES context.
//!
//! A testing entry point, not a production host: it serves the unchanged IPP
//! WebSocket through [`ipp_server::websocket::serve_with`] and adds a separate
//! loopback presentation channel for completed-frame captures, viewport sizing
//! and the `@ipp/client/testing` renderer controls. Clients reach that channel
//! through `nativePresentationTransport` of `@ipp/client/testing`; the windowed
//! gallery adapter also uses it for platform input delivery.
//!
//! ```text
//! gles_host [--bind 127.0.0.1:PORT] --egl-dir DIRECTORY [--io-read PREFIX DIRECTORY] [--http-prefix PREFIX] [--window] [--uncapped]
//! ```
//!
//! Readiness is one stdout line with both loopback URLs:
//! `{"event":"ready","url":"ws://…","presentation":"ws://…"}`.

#[cfg(feature = "instrumentation")]
#[global_allocator]
static ALLOCATOR: ipp_core::profiling::CountingAllocator = ipp_core::profiling::CountingAllocator;

// Share the maintained test-host context loader, never a production device shim.
// This host does not call every loader method, such as the error-check helpers.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[allow(dead_code)]
#[path = "../../ipp-render-gl/examples/smoke/egl.rs"]
mod egl;

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod gles_presentation;

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod cef;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod window;

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    use std::io::{self, Write};
    use std::net::{SocketAddr, TcpListener};
    use std::path::PathBuf;

    let mut window = false;
    let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut bind = "127.0.0.1:0".parse::<SocketAddr>()?;
    let mut egl_directory = std::env::var_os("IPP_EGL_LIBRARY_DIR").map(PathBuf::from);
    let mut read_sources = Vec::new();
    let mut cef = None;
    let mut http_prefixes = Vec::new();
    let mut uncapped = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--uncapped" => uncapped = true,
            "--window" => window = true,
            "--cef" => {
                cef = Some(crate::cef::CefSetup {
                    engine: args
                        .next()
                        .ok_or("--cef requires engine directory and page root")?
                        .into(),
                    root: args
                        .next()
                        .ok_or("--cef requires engine directory and page root")?
                        .into(),
                });
            }
            "--bind" => bind = args.next().ok_or("--bind requires an IP:PORT")?.parse()?,
            "--egl-dir" => {
                egl_directory = Some(args.next().ok_or("--egl-dir requires a directory")?.into())
            }
            "--io-read" => {
                let prefix = args
                    .next()
                    .ok_or("--io-read requires a prefix and directory")?;
                let directory = args.next().ok_or("--io-read requires a directory")?;
                read_sources.push((prefix, PathBuf::from(directory)));
            }
            "--http-prefix" => {
                http_prefixes.push(
                    args.next()
                        .ok_or("--http-prefix requires a literal prefix")?,
                );
            }
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    if !bind.ip().is_loopback() {
        return Err("the GLES test host requires a loopback address".into());
    }

    let egl_directory = egl_directory
        .ok_or("select the EGL/GLES libraries with --egl-dir or IPP_EGL_LIBRARY_DIR")?;
    ipp_server::diagnostics::level_from_env()?;

    let presentation = TcpListener::bind((bind.ip(), 0))?;
    let presentation_address = presentation.local_addr()?;
    gles_presentation::prepare(
        egl_directory,
        presentation,
        read_sources,
        window,
        cef,
        shutdown.clone(),
    )?;

    let listener = TcpListener::bind(bind)?;
    ipp_server::websocket::serve_with::<gles_presentation::GlesHostServices>(
        listener,
        ipp_server::websocket::ServeOptions {
            http_prefixes,
            uncapped,
            shutdown: Some(shutdown),
            ..Default::default()
        },
        |address| {
            println!(
                "{{\"event\":\"ready\",\"url\":\"ws://{address}\",\"presentation\":\"ws://{presentation_address}\"}}"
            );
            io::stdout().flush()
        },
    )?;
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn main() {
    if let Err(error) = run() {
        eprintln!("gles_host: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn main() {
    eprintln!("The GLES test host currently requires Linux or macOS with EGL/GLES libraries");
    std::process::exit(1);
}
