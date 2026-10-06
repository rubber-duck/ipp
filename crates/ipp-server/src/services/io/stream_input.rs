//! Pool-backed acquisition directly into the stream's eventual lent storage.
//!
//! Reader accounting describes live reader backing. Cancellation may detach an
//! already-running blocking fill: that closure still owns its reservation and
//! backing until completion, even after the reader's bulk lease is released.
//! IoStreamInput::buffered_bytes observes this provider-owned allocation while
//! the fill lease exists; reader drop alone does not report it deallocated.

use ipp_core::services::io::{
    IoCancellation, IoReadBackend, IoReadOptions, IoReadWindow, IoStreamInput, STREAM_CAPACITY,
    StreamIoReader,
};
use ipp_host_session::services::task_scheduler::{IoScheduler, TaskHandle};
use std::{
    io::Read,
    num::NonZeroUsize,
    task::{Context, Poll},
};

pub(super) async fn await_operation<T: Send + 'static>(
    operation: TaskHandle<T>,
    cancellation: IoCancellation,
) -> Result<T, String> {
    futures_lite::future::or(
        async move { operation.await.map_err(|error| error.to_string()) },
        async move {
            cancellation.cancelled().await;
            Err("IO acquisition was cancelled".into())
        },
    )
    .await
}

pub(super) struct NativeStreamIoReader {
    stream: StreamIoReader,
    _task: TaskHandle<()>,
}

impl NativeStreamIoReader {
    pub fn new<R: Read + Send + 'static>(
        reader: R,
        scheduler: IoScheduler,
        options: IoReadOptions,
    ) -> Self {
        let (stream, input) = StreamIoReader::new(options);
        let context = scheduler.clone();
        let task = scheduler.spawn(async move {
            let result = acquire(reader, input.clone(), context).await;
            input.finish(result);
        });
        Self {
            stream,
            _task: task,
        }
    }
}

async fn acquire<R: Read + Send + 'static>(
    mut reader: R,
    input: IoStreamInput,
    scheduler: IoScheduler,
) -> Result<(), String> {
    loop {
        let mut reservation =
            std::future::poll_fn(|cx| input.poll_fill(cx, STREAM_CAPACITY)).await?;
        let operation = scheduler.blocking(move || {
            let result = reader
                .read(reservation.bytes_mut())
                .map_err(|error| error.to_string());
            (reader, reservation, result)
        });
        let (returned, reservation, result) =
            await_operation(operation, input.cancellation()).await?;
        reader = returned;
        let count = result?;
        reservation.commit(count)?;
        if count == 0 {
            return Ok(());
        }
    }
}

impl IoReadBackend for NativeStreamIoReader {
    fn register_storage_waker(&mut self, waker: &std::task::Waker) {
        self.stream.register_storage_waker(waker);
    }

    fn retained_storage(&self) -> Option<ipp_core::services::io::IoReaderStorage> {
        self.stream.retained_storage()
    }

    fn poll_ready(
        &mut self,
        cx: &mut Context<'_>,
        minimum: NonZeroUsize,
    ) -> Poll<Result<(), String>> {
        self.stream.poll_ready(cx, minimum)
    }

    fn window(&mut self) -> IoReadWindow<'_> {
        self.stream.window()
    }

    fn cancellation(&self) -> Option<IoCancellation> {
        self.stream.cancellation()
    }
}
