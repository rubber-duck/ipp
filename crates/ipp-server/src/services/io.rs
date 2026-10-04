//! Native sources use asynchronous operations and the Host's shared IO pool.
//!
//! FileSystemIoSource confines literal names to its configured root. The shipped
//! server enables HTTP only through repeatable `--http-prefix` options (none by
//! default); Host registration rejects overlapping namespaces. An enabled source
//! does not itself grant a connection asset-export authority.
//!
//! Linux SealedMappedIoSource lends immutable memfd bytes directly after kernel
//! WRITE/GROW/SHRINK seals are verified. It does not support ordinary mutable-file
//! mapping. File and HTTP streams fill one eventual lent buffer; HTTP/TLS library
//! transport buffers are separate from that reader-storage accounting. External
//! browser ArrayBuffer/SAB sources use one admitted copy into WASM storage, not
//! native mapping or directly borrowed shared WebAssembly.Memory.

use ipp_core::services::io::{
    IoListFuture, IoListing, IoListingBackend, IoOpenReadFuture, IoOpenWriteFuture, IoReadOptions,
    IoReader, IoSource, IoWriteBackend, IoWriter,
};
use ipp_host_session::services::task_scheduler::{IoScheduler, TaskHandle};
use std::{
    fs::File,
    path::{Component, Path, PathBuf},
    task::{Context, Poll},
};

/// Root-confined filesystem source. Identifiers are literal paths after its prefix.
/// Reads use the shared blocking pool and lend one reusable, lookahead-sized buffer.
#[derive(Clone)]
pub struct FileSystemIoSource {
    prefix: String,
    root: PathBuf,
    writable: bool,
    scheduler: IoScheduler,
}

impl FileSystemIoSource {
    /// Select the Host-authorized root and explicit write capability.
    pub fn new(
        prefix: &str,
        root: impl AsRef<Path>,
        writable: bool,
        scheduler: IoScheduler,
    ) -> Result<Self, String> {
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
            scheduler,
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

impl IoSource for FileSystemIoSource {
    fn list(&mut self, identifier: &str) -> IoListFuture {
        let source = self.clone();
        let listing_source = source.clone();
        let identifier = identifier.to_owned();
        let scheduler = self.scheduler.clone();
        Box::pin(async move {
            let directory = scheduler
                .blocking(move || {
                    let path = source.path(&identifier, false)?;
                    std::fs::read_dir(path).map_err(|error| error.to_string())
                })
                .await
                .map_err(|error| error.to_string())??;
            Ok(Box::new(FileIoListing {
                source: listing_source,
                directory: Some(directory),
                finished: false,
                pending: None,
            }) as Box<dyn IoListing>)
        })
    }

    fn open_read(&mut self, identifier: &str, options: IoReadOptions) -> IoOpenReadFuture {
        let source = self.clone();
        let identifier = identifier.to_owned();
        let scheduler = self.scheduler.clone();
        Box::pin(async move {
            if options.recovery {
                return Err("Filesystem recovery has no immutable content validator".into());
            }
            let file = scheduler
                .blocking(move || {
                    let path = source.path(&identifier, false)?;
                    let file = File::open(path).map_err(|error| error.to_string())?;
                    let length = file.metadata().map_err(|error| error.to_string())?.len();
                    if options.max_bytes.is_some_and(|limit| length > limit as u64) {
                        return Err("Filesystem input byte budget exhausted".into());
                    }
                    Ok::<_, String>(file)
                })
                .await
                .map_err(|error| error.to_string())??;
            Ok(Box::new(super::stream_input::NativeStreamIoReader::new(
                file, scheduler, options,
            )) as Box<dyn IoReader>)
        })
    }

    fn can_write(&self, identifier: &str) -> bool {
        self.writable
            && identifier
                .strip_prefix(&self.prefix)
                .is_some_and(|relative| {
                    let relative = if self.separator().is_empty() {
                        relative
                    } else {
                        relative.strip_prefix('/').unwrap_or(relative)
                    };
                    !relative.is_empty()
                        && Path::new(relative)
                            .components()
                            .all(|part| matches!(part, Component::Normal(_) | Component::CurDir))
                })
    }

    fn open_write(&mut self, identifier: &str, max_bytes: usize) -> IoOpenWriteFuture {
        let source = self.clone();
        let identifier = identifier.to_owned();
        let scheduler = self.scheduler.clone();
        Box::pin(async move {
            if !source.writable {
                return Err("Filesystem source is read-only".into());
            }
            let path = scheduler
                .blocking(move || source.path(&identifier, true))
                .await
                .map_err(|error| error.to_string())??;
            let writer = super::asset_output::NativeFileIoWriter::new(path, scheduler).await?;
            Ok(Box::new(BoundedFileIoWriter {
                writer,
                remaining: max_bytes,
            }) as Box<dyn IoWriter>)
        })
    }
}

type FileListingStep = (std::fs::ReadDir, Result<Option<String>, String>);

struct FileIoListing {
    source: FileSystemIoSource,
    directory: Option<std::fs::ReadDir>,
    finished: bool,
    pending: Option<TaskHandle<FileListingStep>>,
}

impl IoListingBackend for FileIoListing {
    fn poll_next(&mut self, cx: &mut Context<'_>) -> Poll<Result<Option<String>, String>> {
        if self.finished {
            return Poll::Ready(Ok(None));
        }
        if self.pending.is_none() {
            let mut directory = self.directory.take().expect("filesystem listing");
            let source = self.source.clone();
            self.pending = Some(self.source.scheduler.blocking(move || {
                let result = (|| {
                    loop {
                        let Some(entry) = directory.next() else {
                            return Ok(None);
                        };
                        let path = entry.map_err(|error| error.to_string())?.path();
                        let canonical = path.canonicalize().map_err(|error| error.to_string())?;
                        if !canonical.starts_with(&source.root) {
                            continue;
                        }
                        let relative = path
                            .strip_prefix(&source.root)
                            .map_err(|error| error.to_string())?;
                        return Ok(Some(format!(
                            "{}{}{}",
                            source.prefix,
                            source.separator(),
                            relative
                                .to_str()
                                .ok_or("Filesystem identifier is not UTF-8")?
                        )));
                    }
                })();
                (directory, result)
            }));
        }
        use std::future::Future;
        match std::pin::Pin::new(self.pending.as_mut().expect("pending filesystem listing"))
            .poll(cx)
        {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(error)) => {
                self.pending = None;
                self.finished = true;
                Poll::Ready(Err(error.to_string()))
            }
            Poll::Ready(Ok((directory, result))) => {
                self.pending = None;
                self.finished = !matches!(result, Ok(Some(_)));
                if !self.finished {
                    self.directory = Some(directory);
                }
                Poll::Ready(result)
            }
        }
    }
}

struct BoundedFileIoWriter {
    writer: super::asset_output::NativeFileIoWriter,
    remaining: usize,
}

impl IoWriteBackend for BoundedFileIoWriter {
    fn cancellation(&self) -> Option<ipp_core::services::io::IoCancellation> {
        self.writer.cancellation()
    }

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
#[path = "io_tests.rs"]
mod tests;
