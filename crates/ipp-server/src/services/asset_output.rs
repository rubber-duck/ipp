//! Native staged output. File I/O runs on an owned worker; only a completed file is renamed.

use ipp_core::services::data_source::DataWriter;
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll, Waker},
};

enum FileWriteOperation {
    Write(Vec<u8>),
    Flush,
    Finish,
}

#[derive(Default)]
struct FileWriteProgress {
    operation: Option<FileWriteOperation>,
    result: Option<Result<usize, String>>,
    waker: Option<Waker>,
    cancelled: bool,
    published: bool,
}

/// An asynchronous sibling-file writer with atomic rename publication.
///
/// Accepted bytes and the file are synced before publication. Existing destinations
/// survive errors and cancellation before the rename. Cancellation racing publication
/// cannot undo a rename that has already committed. Directory crash durability is a
/// separate platform policy; successful completion does not promise directory fsync.
/// One worker and at most one 64 KiB chunk are retained by each active writer.
pub struct NativeFileDataWriter {
    shared: Arc<(Mutex<FileWriteProgress>, Condvar)>,
    pending: Option<FileWriteKind>,
    flushed: bool,
    closed: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FileWriteKind {
    Write,
    Flush,
    Finish,
}

impl NativeFileDataWriter {
    /// Resolve destinations through trusted Host/application policy, never file contents.
    pub fn new(destination: impl AsRef<Path>) -> Result<Self, String> {
        let destination = destination.as_ref().to_path_buf();
        if destination.file_name().is_none() {
            return Err("Output destination must name a file".into());
        }
        let shared = Arc::new((Mutex::new(FileWriteProgress::default()), Condvar::new()));
        let worker = shared.clone();
        std::thread::Builder::new()
            .name("ipp-asset-output".into())
            .spawn(move || run_writer(destination, worker))
            .map_err(|error| error.to_string())?;
        Ok(Self {
            shared,
            pending: None,
            flushed: false,
            closed: false,
        })
    }

    fn poll_acknowledgement(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<FileWriteKind>, String>> {
        if self.closed {
            return Poll::Ready(Err("File output is closed".into()));
        }
        let (lock, _) = &*self.shared;
        let Ok(mut progress) = lock.try_lock() else {
            cx.waker().wake_by_ref();
            return Poll::Pending;
        };
        progress.waker = Some(cx.waker().clone());
        if let Some(result) = progress.result.take() {
            let kind = self.pending.take();
            if let Err(error) = result {
                self.closed = true;
                return Poll::Ready(Err(error));
            }
            if kind == Some(FileWriteKind::Flush) {
                self.flushed = true;
            }
            return Poll::Ready(Ok(kind));
        }
        if self.pending.is_some() {
            Poll::Pending
        } else {
            Poll::Ready(Ok(None))
        }
    }

    fn submit(&mut self, kind: FileWriteKind, operation: FileWriteOperation) {
        let (lock, ready) = &*self.shared;
        lock.lock().expect("file output mutex").operation = Some(operation);
        self.pending = Some(kind);
        ready.notify_one();
    }
}

impl DataWriter for NativeFileDataWriter {
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
        self.flushed = false;
        self.submit(
            FileWriteKind::Write,
            FileWriteOperation::Write(bytes[..count].to_vec()),
        );
        // Ownership and progress are acknowledged together. A Pending call never
        // copies or consumes the caller's bytes; it only waits for buffer space.
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
        self.submit(FileWriteKind::Flush, FileWriteOperation::Flush);
        Poll::Pending
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
        if self.flushed {
            self.submit(FileWriteKind::Finish, FileWriteOperation::Finish);
        } else {
            self.submit(FileWriteKind::Flush, FileWriteOperation::Flush);
        }
        Poll::Pending
    }

    fn abort(&mut self) {
        let (lock, ready) = &*self.shared;
        let mut progress = lock.lock().expect("file output mutex");
        progress.cancelled = true;
        progress.operation = None;
        progress.result = None;
        ready.notify_one();
        self.closed = true;
    }
}

impl Drop for NativeFileDataWriter {
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

fn run_writer(destination: PathBuf, shared: Arc<(Mutex<FileWriteProgress>, Condvar)>) {
    let (lock, ready) = &*shared;
    let (temporary, mut file) = match temporary_file(&destination) {
        Ok(result) => result,
        Err(error) => {
            finish_operation(lock, Err(error));
            return;
        }
    };
    loop {
        let mut progress = lock.lock().expect("file output mutex");
        while progress.operation.is_none() && !progress.cancelled {
            progress = ready.wait(progress).expect("file output mutex");
        }
        if progress.cancelled {
            break;
        }
        let operation = progress.operation.take().expect("queued file operation");
        if matches!(operation, FileWriteOperation::Finish) {
            // Serialize cancellation against the publication point. The worker owns all
            // file work and the caller never waits for writes or sync in a poll method.
            let result = std::fs::rename(&temporary, &destination)
                .map(|()| 0)
                .map_err(|error| error.to_string());
            progress.published = result.is_ok();
            progress.result = Some(result);
            if let Some(waker) = progress.waker.take() {
                waker.wake();
            }
            break;
        }
        drop(progress);
        let result = match operation {
            FileWriteOperation::Write(bytes) => file.write_all(&bytes).map(|()| bytes.len()),
            FileWriteOperation::Flush => file.flush().and_then(|()| file.sync_all()).map(|()| 0),
            FileWriteOperation::Finish => unreachable!(),
        }
        .map_err(|error| error.to_string());
        let failed = result.is_err();
        finish_operation(lock, result);
        if failed {
            break;
        }
    }
    drop(file);
    if !lock.lock().expect("file output mutex").published {
        let _ = std::fs::remove_file(temporary);
    }
}

fn finish_operation(lock: &Mutex<FileWriteProgress>, result: Result<usize, String>) {
    let mut progress = lock.lock().expect("file output mutex");
    progress.result = Some(result);
    if let Some(waker) = progress.waker.take() {
        waker.wake();
    }
}

#[cfg(test)]
#[path = "asset_output_tests.rs"]
mod tests;
