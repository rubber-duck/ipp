//! Offscreen native WebSocket Host for the WebGPU Metal comparison.

#[cfg(all(target_os = "macos", feature = "instrumentation"))]
#[global_allocator]
static ALLOCATOR: ipp_core::profiling::CountingAllocator = ipp_core::profiling::CountingAllocator;

#[cfg(target_os = "macos")]
mod host_services;

#[cfg(target_os = "macos")]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    use std::{
        io::{self, Write},
        net::{SocketAddr, TcpListener},
    };
    let mut bind: SocketAddr = "127.0.0.1:0".parse()?;
    let mut read_sources = Vec::new();
    let mut http_prefixes = Vec::new();
    let mut uncapped = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bind" => bind = args.next().ok_or("--bind requires an address")?.parse()?,
            "--io-read" => {
                let prefix = args
                    .next()
                    .ok_or("--io-read requires a prefix and directory")?;
                let directory = args.next().ok_or("--io-read requires a directory")?;
                read_sources.push((prefix, std::path::PathBuf::from(directory)));
            }
            "--http-prefix" => {
                http_prefixes.push(args.next().ok_or("--http-prefix requires a prefix")?)
            }
            "--uncapped" => uncapped = true,
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    if !bind.ip().is_loopback() {
        return Err("WebGPU Host requires a loopback address".into());
    }
    host_services::set_read_sources(read_sources)?;
    ipp_server::diagnostics::install(ipp_server::diagnostics::level_from_env()?, 0);
    ipp_server::websocket::serve_with::<host_services::WebGpuHostServices>(
        TcpListener::bind(bind)?,
        ipp_server::websocket::ServeOptions {
            uncapped,
            http_prefixes,
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
        eprintln!("ipp-webgpu-host: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("The WebGPU Metal comparison Host requires macOS");
    std::process::exit(1);
}
