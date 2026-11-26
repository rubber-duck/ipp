//! The native WebSocket distribution that submits offscreen Metal frames.

#[cfg(all(target_os = "macos", feature = "instrumentation"))]
#[global_allocator]
static ALLOCATOR: ipp_core::profiling::CountingAllocator = ipp_core::profiling::CountingAllocator;

#[cfg(target_os = "macos")]
mod cef;
#[cfg(target_os = "macos")]
mod host_services;
#[cfg(all(target_os = "macos", feature = "instrumentation"))]
mod presentation_testing;
#[cfg(all(target_os = "macos", feature = "instrumentation"))]
mod profiling;

#[cfg(target_os = "macos")]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    use std::{
        io::{self, Write},
        net::{SocketAddr, TcpListener},
        path::PathBuf,
    };

    let mut bind = "127.0.0.1:0".parse::<SocketAddr>()?;
    let mut read_sources = Vec::new();
    let mut http_prefixes = Vec::new();
    let mut uncapped = false;
    let mut cef = None;
    let mut pages = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bind" => bind = args.next().ok_or("--bind requires an IP:PORT")?.parse()?,
            "--io-read" => {
                let prefix = args
                    .next()
                    .ok_or("--io-read requires a prefix and directory")?;
                let directory = args.next().ok_or("--io-read requires a directory")?;
                read_sources.push((prefix, PathBuf::from(directory)));
            }
            "--http-prefix" => {
                http_prefixes.push(args.next().ok_or("--http-prefix requires a prefix")?)
            }
            "--cef" => {
                let engine = args
                    .next()
                    .ok_or("--cef requires engine and page directories")?
                    .into();
                let root = args
                    .next()
                    .ok_or("--cef requires engine and page directories")?
                    .into();
                cef = Some(cef::CefSetup {
                    engine,
                    root,
                    pages: Vec::new(),
                });
            }
            "--cef-page" => {
                let name = args
                    .next()
                    .ok_or("--cef-page requires a source and local HTML page")?;
                let path = args
                    .next()
                    .ok_or("--cef-page requires a source and local HTML page")?
                    .into();
                pages.push((name, path));
            }
            #[cfg(feature = "instrumentation")]
            "--simulate-device-recovery-after-frame" => presentation_testing::configure(
                args.next()
                    .ok_or("recovery test requires a frame count")?
                    .parse()?,
            )?,
            "--uncapped" => uncapped = true,
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    if !bind.ip().is_loopback() {
        return Err("the offscreen Metal Host requires a loopback address".into());
    }

    if let Some(mut setup) = cef {
        setup.pages = pages;
        host_services::set_cef(setup)?;
    } else if !pages.is_empty() {
        return Err("--cef-page requires --cef".into());
    }
    host_services::set_read_sources(read_sources)?;
    ipp_server::diagnostics::install(ipp_server::diagnostics::level_from_env()?, 0);
    ipp_server::websocket::serve_with::<host_services::MetalHostServices>(
        TcpListener::bind(bind)?,
        ipp_server::websocket::ServeOptions {
            http_prefixes,
            uncapped,
            ..Default::default()
        },
        |address| {
            println!("{{\"event\":\"ready\",\"url\":\"ws://{address}\"}}");
            io::stdout().flush()
        },
    )?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn main() {
    if let Err(error) = run() {
        eprintln!("ipp-metal-host: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("ipp-metal-host requires macOS");
    std::process::exit(1);
}
