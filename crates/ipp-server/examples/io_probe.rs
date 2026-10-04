//! Stream an explicitly selected public HTTP(S) source through the native provider.

use ipp_core::services::io::{IoReadOptions, IoService};
use ipp_host_session::services::task_scheduler::TaskSchedulerService;
use std::num::NonZeroUsize;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = std::env::args()
        .nth(1)
        .ok_or("usage: io_probe HTTP_OR_HTTPS_URL")?;
    let tasks = TaskSchedulerService::new();
    let mut io = IoService::new();
    io.register(
        &source,
        ipp_server::services::http::HttpIoSource::new(tasks.schedulers().io())?,
    )?;
    let (received, peak) = futures_lite::future::block_on(async {
        let mut reader = io
            .open_read(
                &source,
                IoReadOptions {
                    max_bytes: None,
                    recovery: false,
                },
            )
            .await?;
        let mut received = 0_u64;
        let mut peak = 0;
        loop {
            peak = peak.max(reader.retained_storage().map_or(0, |storage| storage.bytes));
            let window = reader.read(NonZeroUsize::new(4096).unwrap()).await?;
            let count = window.bytes().len();
            let final_input = window.is_final();
            received = received
                .checked_add(count as u64)
                .ok_or("input length overflow")?;
            window.consume(count)?;
            peak = peak.max(reader.retained_storage().map_or(0, |storage| storage.bytes));
            if final_input {
                return Ok::<_, String>((received, peak));
            }
        }
    })?;
    println!("{{\"source_bytes\":{received},\"reader_owned_heap_peak\":{peak}}}");
    Ok(())
}
