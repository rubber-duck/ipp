//! Poll-based owned output, independent of executors, filesystems and transports.

use std::task::{Context, Poll};

/// An output destination with explicit completion/publication. Pending writes do not
/// retain the borrowed input; the job keeps it alive until progress is acknowledged.
pub trait DataWriter {
    /// Accept a prefix of input. Pending accepts nothing; a nonempty zero write is an error.
    fn poll_write(&mut self, cx: &mut Context<'_>, bytes: &[u8]) -> Poll<Result<usize, String>>;

    /// Flush accepted bytes without publishing a partial destination.
    fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), String>>;

    /// Publish the completed destination. This may itself require asynchronous progress.
    fn poll_finish(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), String>>;

    /// Cancel unpublished output and release destination staging.
    fn abort(&mut self);
}

/// Bounded owned encoding waiting for partial writes, flush and publication.
pub struct DataWriteJob<W: DataWriter> {
    writer: Option<W>,
    bytes: Vec<u8>,
    offset: usize,
    flushed: bool,
    complete: bool,
    failure: Option<String>,
}

impl<W: DataWriter> DataWriteJob<W> {
    /// The caller's export budget must include these owned bytes until completion.
    pub fn new(bytes: Vec<u8>, writer: W) -> Self {
        Self {
            writer: Some(writer),
            bytes,
            offset: 0,
            flushed: false,
            complete: false,
            failure: None,
        }
    }

    /// Progress at most sixteen 64 KiB writes per Host service phase.
    pub fn poll(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), String>> {
        if let Some(error) = &self.failure {
            return Poll::Ready(Err(error.clone()));
        }
        if self.complete {
            return Poll::Ready(Ok(()));
        }
        let result = self.poll_inner(cx);
        if let Poll::Ready(Err(error)) = &result {
            self.writer.as_mut().expect("active writer").abort();
            self.bytes.clear();
            self.failure = Some(error.clone());
        }
        result
    }

    fn poll_inner(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), String>> {
        let writer = self.writer.as_mut().expect("active writer");
        for _ in 0..16 {
            if self.offset == self.bytes.len() {
                break;
            }
            let end = self.bytes.len().min(self.offset.saturating_add(64 << 10));
            match writer.poll_write(cx, &self.bytes[self.offset..end]) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Ready(Ok(n)) if n == 0 || n > end - self.offset => {
                    return Poll::Ready(Err("Invalid output writer progress".into()));
                }
                Poll::Ready(Ok(n)) => self.offset += n,
            }
        }
        if self.offset != self.bytes.len() {
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        if !self.flushed {
            match writer.poll_flush(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Ready(Ok(())) => self.flushed = true,
            }
        }
        match writer.poll_finish(cx) {
            Poll::Ready(Ok(())) => {
                self.complete = true;
                self.bytes = Vec::new();
                Poll::Ready(Ok(()))
            }
            result => result,
        }
    }

    /// Recover a successfully published sink. Unfinished or failed jobs are cancelled.
    pub fn into_writer(mut self) -> Result<W, String> {
        if !self.complete {
            return Err(self
                .failure
                .clone()
                .unwrap_or_else(|| "Output is incomplete".into()));
        }
        Ok(self.writer.take().expect("completed writer"))
    }
}

impl<W: DataWriter> Drop for DataWriteJob<W> {
    fn drop(&mut self) {
        if !self.complete
            && let Some(writer) = &mut self.writer
        {
            writer.abort();
        }
    }
}

/// Bounded output for a caller-owned buffer or browser file publication adapter.
pub struct MemoryDataWriter {
    bytes: Vec<u8>,
    limit: usize,
    complete: bool,
    aborted: bool,
}

impl MemoryDataWriter {
    /// Construct an unpublished destination with an explicit byte budget.
    pub fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
            complete: false,
            aborted: false,
        }
    }

    /// Take bytes only after successful output completion.
    pub fn into_bytes(self) -> Result<Vec<u8>, String> {
        if !self.complete {
            return Err("Output is not published".into());
        }
        Ok(self.bytes)
    }
}

impl DataWriter for MemoryDataWriter {
    fn poll_write(&mut self, _cx: &mut Context<'_>, bytes: &[u8]) -> Poll<Result<usize, String>> {
        if self.complete || self.aborted {
            return Poll::Ready(Err("Output is closed".into()));
        }
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Poll::Ready(Err("Output byte budget exhausted".into()));
        }
        if let Err(error) = self.bytes.try_reserve_exact(bytes.len()) {
            return Poll::Ready(Err(error.to_string()));
        }
        self.bytes.extend_from_slice(bytes);
        Poll::Ready(Ok(bytes.len()))
    }

    fn poll_flush(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), String>> {
        Poll::Ready(if self.aborted {
            Err("Output is cancelled".into())
        } else {
            Ok(())
        })
    }

    fn poll_finish(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), String>> {
        if self.aborted {
            return Poll::Ready(Err("Output is cancelled".into()));
        }
        self.complete = true;
        Poll::Ready(Ok(()))
    }

    fn abort(&mut self) {
        if !self.complete {
            self.aborted = true;
            self.bytes = Vec::new();
        }
    }
}

impl<W: DataWriter + ?Sized> DataWriter for Box<W> {
    fn poll_write(&mut self, cx: &mut Context<'_>, bytes: &[u8]) -> Poll<Result<usize, String>> {
        (**self).poll_write(cx, bytes)
    }

    fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), String>> {
        (**self).poll_flush(cx)
    }

    fn poll_finish(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), String>> {
        (**self).poll_finish(cx)
    }

    fn abort(&mut self) {
        (**self).abort();
    }
}
