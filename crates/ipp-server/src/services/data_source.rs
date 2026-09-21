//! Native filesystem access through generic data-source contracts.

use ipp_core::services::data_source::{DataReadOptions, DataReader, DataSource, DataWriter};
use std::{
    fs::File,
    io::Read,
    path::{Component, Path, PathBuf},
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, TryRecvError, sync_channel},
    },
    task::{Context, Poll, Waker},
};

/// Root-confined filesystem source. Identifiers are literal paths after its prefix.
/// Filesystem reads run on a worker and retain at most two 64 KiB input chunks.
pub struct FileSystemDataSource {
    prefix: String,
    root: PathBuf,
    writable: bool,
}

impl FileSystemDataSource {
    /// Select the Host-authorized root and explicit write capability.
    pub fn new(prefix: &str, root: impl AsRef<Path>, writable: bool) -> Result<Self, String> {
        let root = root
            .as_ref()
            .canonicalize()
            .map_err(|error| error.to_string())?;
        if !root.is_dir() {
            return Err("Filesystem root must be a directory".into());
        }
        Ok(Self {
            prefix: prefix.to_owned(),
            root,
            writable,
        })
    }

    fn separator(&self) -> &'static str {
        if self.prefix.ends_with([':', '/']) {
            ""
        } else {
            "/"
        }
    }

    fn path(&self, identifier: &str, writing: bool) -> Result<PathBuf, String> {
        let relative = identifier
            .strip_prefix(&self.prefix)
            .ok_or("Filesystem prefix mismatch")?;
        let relative = if relative.is_empty() || self.separator().is_empty() {
            relative
        } else {
            relative
                .strip_prefix('/')
                .ok_or("Filesystem path must follow its root separator")?
        };
        let relative = Path::new(relative);
        // Do not decode URL escapes or normalize away forbidden traversal.
        if relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir))
        {
            return Err("Filesystem parent traversal and absolute paths are forbidden".into());
        }
        let path = self.root.join(relative);
        let checked = if writing {
            let parent = path
                .parent()
                .ok_or("Missing filesystem parent")?
                .canonicalize()
                .map_err(|error| error.to_string())?;
            if !parent.starts_with(&self.root) {
                return Err("Filesystem path escapes its root".into());
            }
            if path.exists()
                && !path
                    .canonicalize()
                    .map_err(|error| error.to_string())?
                    .starts_with(&self.root)
            {
                return Err("Filesystem destination escapes its root".into());
            }
            parent.join(
                path.file_name()
                    .ok_or("Filesystem destination must name a file")?,
            )
        } else {
            path.canonicalize().map_err(|error| error.to_string())?
        };
        if !checked.starts_with(&self.root) {
            return Err("Filesystem path escapes its root".into());
        }
        Ok(checked)
    }
}

impl DataSource for FileSystemDataSource {
    fn list(&mut self, identifier: &str) -> Result<Vec<String>, String> {
        let path = self.path(identifier, false)?;
        let mut names = Vec::new();
        for entry in std::fs::read_dir(path).map_err(|error| error.to_string())? {
            if names.len() >= 4096 {
                return Err("Filesystem listing budget exhausted".into());
            }
            let path = entry.map_err(|error| error.to_string())?.path();
            let canonical = path.canonicalize().map_err(|error| error.to_string())?;
            if !canonical.starts_with(&self.root) {
                continue;
            }
            let relative = path
                .strip_prefix(&self.root)
                .map_err(|error| error.to_string())?;
            names.push(format!(
                "{}{}{}",
                self.prefix,
                self.separator(),
                relative
                    .to_str()
                    .ok_or("Filesystem identifier is not UTF-8")?
            ));
        }
        names.sort();
        Ok(names)
    }

    fn open_read(
        &mut self,
        identifier: &str,
        options: DataReadOptions,
    ) -> Result<Box<dyn DataReader>, String> {
        // A mutable filesystem path has no immutable-content validator. Explicitly
        // fail recovery instead of silently accepting replacement file contents.
        if options.recovery {
            return Err("Filesystem recovery has no immutable content validator".into());
        }
        let path = self.path(identifier, false)?;
        let (sender, receiver) = sync_channel(1);
        let wake: Arc<Mutex<Option<Waker>>> = Arc::default();
        let worker_wake = wake.clone();
        std::thread::Builder::new()
            .name("ipp-data-input".into())
            .spawn(move || {
                let result = (|| {
                    let mut file = File::open(path).map_err(|error| error.to_string())?;
                    let length = file.metadata().map_err(|error| error.to_string())?.len();
                    if options.max_bytes.is_some_and(|limit| length > limit as u64) {
                        return Err("Filesystem input byte budget exhausted".into());
                    }
                    let mut read = 0usize;
                    loop {
                        let mut bytes =
                            vec![
                                0;
                                (64 << 10).min(options.max_bytes.map_or(64 << 10, |limit| {
                                    limit.saturating_sub(read).saturating_add(1)
                                }))
                            ];
                        let count = file.read(&mut bytes).map_err(|error| error.to_string())?;
                        if count == 0 {
                            return Ok(());
                        }
                        read += count;
                        if options.max_bytes.is_some_and(|limit| read > limit) {
                            return Err("Filesystem input byte budget exhausted".into());
                        }
                        bytes.truncate(count);
                        if sender.send(Ok(bytes)).is_err() {
                            return Ok(());
                        }
                        if let Some(waker) = worker_wake.lock().expect("input waker").take() {
                            waker.wake();
                        }
                    }
                })();
                let _ = sender.send(result.map(|()| Vec::new()));
                if let Some(waker) = worker_wake.lock().expect("input waker").take() {
                    waker.wake();
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Box::new(FileDataReader {
            receiver,
            wake,
            bytes: Vec::new(),
            offset: 0,
            ended: false,
        }))
    }

    fn can_write(&self, identifier: &str) -> bool {
        self.writable && self.path(identifier, true).is_ok()
    }

    fn open_write(
        &mut self,
        identifier: &str,
        max_bytes: usize,
    ) -> Result<Box<dyn DataWriter>, String> {
        if !self.writable {
            return Err("Filesystem source is read-only".into());
        }
        let path = self.path(identifier, true)?;
        Ok(Box::new(BoundedFileDataWriter {
            writer: super::asset_output::NativeFileDataWriter::new(path)?,
            remaining: max_bytes,
        }))
    }
}

struct FileDataReader {
    receiver: Receiver<Result<Vec<u8>, String>>,
    wake: Arc<Mutex<Option<Waker>>>,
    bytes: Vec<u8>,
    offset: usize,
    ended: bool,
}

impl DataReader for FileDataReader {
    fn poll_read(
        &mut self,
        cx: &mut Context<'_>,
        output: &mut [u8],
    ) -> Poll<Result<usize, String>> {
        if output.is_empty() || self.ended {
            return Poll::Ready(Ok(0));
        }
        if self.offset == self.bytes.len() {
            *self.wake.lock().expect("input waker") = Some(cx.waker().clone());
            match self.receiver.try_recv() {
                Ok(Ok(bytes)) => {
                    self.bytes = bytes;
                    self.offset = 0;
                    if self.bytes.is_empty() {
                        self.ended = true;
                        return Poll::Ready(Ok(0));
                    }
                }
                Ok(Err(error)) => {
                    self.ended = true;
                    return Poll::Ready(Err(error));
                }
                Err(TryRecvError::Empty) => return Poll::Pending,
                Err(TryRecvError::Disconnected) => {
                    return Poll::Ready(Err("Filesystem reader ended without completion".into()));
                }
            }
        }
        let count = output.len().min(self.bytes.len() - self.offset);
        output[..count].copy_from_slice(&self.bytes[self.offset..self.offset + count]);
        self.offset += count;
        Poll::Ready(Ok(count))
    }
}

struct BoundedFileDataWriter {
    writer: super::asset_output::NativeFileDataWriter,
    remaining: usize,
}

impl DataWriter for BoundedFileDataWriter {
    fn poll_write(&mut self, cx: &mut Context<'_>, bytes: &[u8]) -> Poll<Result<usize, String>> {
        if bytes.len() > self.remaining {
            return Poll::Ready(Err("Filesystem output byte budget exhausted".into()));
        }
        self.writer
            .poll_write(cx, bytes)
            .map(|result| result.inspect(|count| self.remaining -= count))
    }

    fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), String>> {
        self.writer.poll_flush(cx)
    }

    fn poll_finish(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), String>> {
        self.writer.poll_finish(cx)
    }

    fn abort(&mut self) {
        self.writer.abort();
    }
}

#[cfg(test)]
#[path = "data_source_tests.rs"]
mod tests;
