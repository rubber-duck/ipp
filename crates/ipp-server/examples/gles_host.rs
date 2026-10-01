//! Native WebSocket Host that presents its attached World on a GLES context.
//!
//! A testing entry point, not a production host: it serves the unchanged IPP
//! WebSocket through [`ipp_server::websocket::serve_with`] and adds a separate
//! loopback presentation channel for completed-frame captures, viewport sizing
//! and the `@ipp/client/testing` renderer controls. Clients reach that channel
//! only through `nativePresentationTransport` of `@ipp/client/testing`.
//!
//! ```text
//! gles_host [--bind 127.0.0.1:PORT] --egl-dir DIRECTORY
//! ```
//!
//! Readiness is one stdout line with both loopback URLs:
//! `{"event":"ready","url":"ws://…","presentation":"ws://…"}`.

// Share the maintained test-host context loader, never a production device shim.
// This host does not call every loader method, such as the error-check helpers.
#[cfg(target_os = "linux")]
#[allow(dead_code)]
#[path = "../../ipp-render-gl/examples/smoke/egl.rs"]
mod egl;

#[cfg(target_os = "linux")]
mod gles_presentation;

#[cfg(target_os = "linux")]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    use std::io::{self, Write};
    use std::net::{SocketAddr, TcpListener};
    use std::path::PathBuf;

    let mut bind = "127.0.0.1:0".parse::<SocketAddr>()?;
    let mut egl_directory = std::env::var_os("IPP_EGL_LIBRARY_DIR").map(PathBuf::from);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bind" => bind = args.next().ok_or("--bind requires an IP:PORT")?.parse()?,
            "--egl-dir" => {
                egl_directory = Some(args.next().ok_or("--egl-dir requires a directory")?.into())
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
    gles_presentation::prepare(egl_directory, presentation)?;

    let listener = TcpListener::bind(bind)?;
    ipp_server::websocket::serve_with::<gles_presentation::GlesHostServices>(
        listener,
        ipp_server::websocket::ServeOptions::default(),
        |address| {
            println!(
                "{{\"event\":\"ready\",\"url\":\"ws://{address}\",\"presentation\":\"ws://{presentation_address}\"}}"
            );
            io::stdout().flush()
        },
    )?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn main() {
    if let Err(error) = run() {
        eprintln!("gles_host: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("The GLES test host currently requires the Linux EGL test-host loader");
    std::process::exit(1);
}
