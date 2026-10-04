//! Local headless IPP host with optional WebSocket ingress.

#[cfg(feature = "instrumentation")]
#[global_allocator]
static ALLOCATOR: ipp_core::profiling::CountingAllocator = ipp_core::profiling::CountingAllocator;

use std::io::{self, Write};
use std::net::{SocketAddr, TcpListener};

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut bind = "127.0.0.1:9231".parse::<SocketAddr>()?;
    let mut file_root = None;
    let mut file_prefix = None;
    let mut asset_cache_bytes = None;
    let mut http_prefixes = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bind" => {
                bind = args.next().ok_or("--bind requires an IP:PORT")?.parse()?;
            }
            "--file-root" => {
                file_root = Some(args.next().ok_or("--file-root requires a directory")?)
            }
            "--file-prefix" => {
                file_prefix = Some(
                    args.next()
                        .ok_or("--file-prefix requires a literal prefix")?,
                )
            }
            "--http-prefix" => {
                http_prefixes.push(
                    args.next()
                        .ok_or("--http-prefix requires a literal prefix")?,
                );
            }
            "--asset-cache-bytes" => {
                asset_cache_bytes = Some(
                    args.next()
                        .ok_or("--asset-cache-bytes requires a byte count")?
                        .parse::<usize>()?,
                )
            }
            "--help" | "-h" => {
                println!(
                    "ipp-server [--bind LOOPBACK_IP:PORT] [--file-root DIRECTORY --file-prefix PREFIX] [--asset-cache-bytes BYTES] [--http-prefix PREFIX]...\n\nBinary IPP WebSocket sessions; use port 0 for an ephemeral endpoint. The Host keeps up to 64 MiB of unused completed assets by default; --asset-cache-bytes sets that target and 0 evicts on release."
                );
                return Ok(());
            }
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    if !bind.ip().is_loopback() {
        return Err("the PoC host requires a loopback address".into());
    }
    ipp_server::diagnostics::level_from_env()?;

    let file_access = match (file_root, file_prefix) {
        (None, None) => None,
        (Some(root), Some(prefix)) => {
            if prefix.is_empty() {
                return Err("--file-prefix must not be empty".into());
            }
            Some((prefix, std::path::PathBuf::from(root)))
        }
        _ => return Err("--file-root and --file-prefix must be provided together".into()),
    };
    let listener = TcpListener::bind(bind)?;
    let options = ipp_server::websocket::ServeOptions {
        file_access,
        http_prefixes,
        asset_cache_bytes,
    };
    ipp_server::websocket::serve(listener, options, |address| {
        println!("{{\"event\":\"ready\",\"url\":\"ws://{address}\"}}");
        io::stdout().flush()
    })?;
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("ipp-server: {error}");
        std::process::exit(1);
    }
}
