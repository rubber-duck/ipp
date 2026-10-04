//! Pool-backed sibling-file output with explicit atomic publication.

use ipp_core::services::io::{IoCancellation, IoWriteBackend};
use ipp_host_session::services::task_scheduler::{IoScheduler, TaskHandle};
use std::{
    fs::{File, OpenOptions},
    future::Future,
    io::Write,
    path::{Path, PathBuf},
    pin::Pin,
    sync::atomic::{AtomicU64, Ordering},
    task::{Context, Poll},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum FileWriteKind {
    Write,
    Flush,
    Finish,
}

struct FileOutput {
    file: Option<File>,
    temporary: PathBuf,
    destination: PathBuf,
    published: bool,
}

impl Drop for FileOutput {
    fn drop(&mut self) {
        self.file.take();
        if !self.published {
            let _ = std::fs::remove_file(&self.temporary);
        }
    }
}

/// Owned asynchronous file sink. Accepted chunks are synced before atomic rename.
/// Existing destinations survive cancellation/errors before publication. Cancellation
/// racing a committed rename cannot undo it; directory crash durability is not promised.
pub struct NativeFileIoWriter {
    output: Option<FileOutput>,
    pending: Option<TaskHandle<(FileOutput, Result<FileWriteKind, String>)>>,
    scheduler: IoScheduler,
    cancellation: IoCancellation,
    flushed: bool,
    closed: bool,
}

impl NativeFileIoWriter {
    /// Create private sibling staging through the Host's blocking pool.
    pub async fn new(
        destination: impl AsRef<Path>,
        scheduler: IoScheduler,
    ) -> Result<Self, String> {
        let destination = destination.as_ref().to_path_buf();
        let output = scheduler
            .blocking(move || {
                let (temporary, file) = temporary_file(&destination)?;
                Ok::<_, String>(FileOutput {
                    file: Some(file),
                    temporary,
                    destination,
                    published: false,
                })
            })
            .await
            .map_err(|error| error.to_string())??;
        Ok(Self {
            output: Some(output),
            pending: None,
            scheduler,
            cancellation: IoCancellation::default(),
            flushed: false,
            closed: false,
        })
    }

    fn poll_acknowledgement(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<FileWriteKind>, String>> {
        self.cancellation.register(cx.waker());
        if self.closed || self.cancellation.is_cancelled() {
            return Poll::Ready(Err("File output is closed".into()));
        }
        let Some(pending) = &mut self.pending else {
            return Poll::Ready(Ok(None));
        };
        match Pin::new(pending).poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(error)) => {
                self.pending = None;
                self.closed = true;
                Poll::Ready(Err(error.to_string()))
            }
            Poll::Ready(Ok((output, result))) => {
                self.pending = None;
                self.output = Some(output);
                match result {
                    Err(error) => {
                        self.abort();
                        Poll::Ready(Err(error))
                    }
                    Ok(kind) => {
                        if kind == FileWriteKind::Flush {
                            self.flushed = true;
                        }
                        Poll::Ready(Ok(Some(kind)))
                    }
                }
            }
        }
    }

    fn submit(&mut self, kind: FileWriteKind, bytes: Vec<u8>) {
        let mut output = self.output.take().expect("file output ownership");
        let cancellation = self.cancellation.clone();
        self.pending = Some(self.scheduler.blocking(move || {
            let result = (|| {
                if cancellation.is_cancelled() {
                    return Err("File output was cancelled".to_owned());
                }
                match kind {
                    FileWriteKind::Write => {
                        output.file.as_mut().expect("staged file").write_all(&bytes)
                    }
                    FileWriteKind::Flush => {
                        let file = output.file.as_mut().expect("staged file");
                        file.flush().and_then(|()| file.sync_all())
                    }
                    FileWriteKind::Finish => {
                        std::fs::rename(&output.temporary, &output.destination)
                            .map(|()| output.published = true)
                    }
                }
                .map_err(|error| error.to_string())?;
                Ok(kind)
            })();
            (output, result)
        }));
    }
}

impl IoWriteBackend for NativeFileIoWriter {
    fn cancellation(&self) -> Option<IoCancellation> {
        Some(self.cancellation.clone())
    }

    fn poll_write(&mut self, cx: &mut Context<'_>, bytes: &[u8]) -> Poll<Result<usize, String>> {
        match self.poll_acknowledgement(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
            Poll::Ready(Ok(Some(FileWriteKind::Finish))) => {
                self.closed = true;
                return Poll::Ready(Err("File output is published".into()));
            }
            Poll::Ready(Ok(_)) => {}
        }
        let count = bytes.len().min(64 << 10);
        if count == 0 {
            return Poll::Ready(Ok(0));
        }
        let mut accepted = Vec::new();
        if let Err(error) = accepted.try_reserve_exact(count) {
            return Poll::Ready(Err(error.to_string()));
        }
        accepted.extend_from_slice(&bytes[..count]);
        self.flushed = false;
        self.submit(FileWriteKind::Write, accepted);
        Poll::Ready(Ok(count))
    }

    fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), String>> {
        match self.poll_acknowledgement(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
            Poll::Ready(Ok(Some(FileWriteKind::Finish))) => {
                self.closed = true;
                return Poll::Ready(Err("File output is published".into()));
            }
            Poll::Ready(Ok(_)) => {}
        }
        if self.flushed {
            return Poll::Ready(Ok(()));
        }
        self.submit(FileWriteKind::Flush, Vec::new());
        self.poll_flush(cx)
    }

    fn poll_finish(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), String>> {
        match self.poll_acknowledgement(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
            Poll::Ready(Ok(Some(FileWriteKind::Finish))) => {
                self.closed = true;
                return Poll::Ready(Ok(()));
            }
            Poll::Ready(Ok(_)) => {}
        }
        if !self.flushed {
            self.submit(FileWriteKind::Flush, Vec::new());
        } else {
            self.submit(FileWriteKind::Finish, Vec::new());
        }
        self.poll_finish(cx)
    }

    fn abort(&mut self) {
        self.cancellation.cancel();
        self.pending = None;
        self.output = None;
        self.closed = true;
    }
}

impl Drop for NativeFileIoWriter {
    fn drop(&mut self) {
        self.abort();
    }
}

fn temporary_file(destination: &Path) -> Result<(PathBuf, File), String> {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    for _ in 0..32 {
        let mut name = destination
            .file_name()
            .ok_or("Missing output file name")?
            .to_os_string();
        name.push(format!(
            ".ipp-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let temporary = destination.with_file_name(name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => return Ok((temporary, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.to_string()),
        }
    }
    Err("Cannot reserve a unique output staging file".into())
}

#[cfg(test)]
#[path = "asset_output_tests.rs"]
mod tests;
