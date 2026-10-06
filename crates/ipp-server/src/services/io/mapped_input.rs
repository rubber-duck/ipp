//! Linux immutable IPC backing: seal producer writes before mapping publication.
//!
//! This adapter accepts owned descriptors of sealed memfd files. Ordinary mutable
//! files, read-only descriptors and F_SEAL_FUTURE_WRITE alone are insufficient.

use ipp_core::services::io::{
    IoImmutableBacking, IoListFuture, IoOpenReadFuture, IoReadOptions, IoReader, IoSource,
    MappedIoReader, MemoryIoListing,
};
use ipp_host_session::services::task_scheduler::IoScheduler;
use std::{fs::File, os::fd::AsRawFd, sync::Arc};

struct SealedMapping(memmap2::Mmap);

impl AsRef<[u8]> for SealedMapping {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl IoImmutableBacking for SealedMapping {
    fn retained_bytes(&self) -> usize {
        0
    }

    fn mapped_bytes(&self) -> usize {
        self.0.len()
    }
}

/// One exact immutable file publication, with independent directly lent cursors.
#[derive(Clone)]
pub struct SealedMappedIoSource {
    identifier: String,
    backing: Arc<dyn IoImmutableBacking>,
    length: usize,
}

impl SealedMappedIoSource {
    /// Capture an owned descriptor before awaiting. Kernel seals are irrevocable;
    /// every active mapping survives producer exit, descriptor close and revocation.
    pub async fn new(
        identifier: String,
        file: File,
        scheduler: IoScheduler,
    ) -> Result<Self, String> {
        let (backing, length) = scheduler
            .blocking(move || {
                // SAFETY: file owns this live descriptor throughout fcntl. F_GET_SEALS
                // reads kernel metadata only; no pointers, mutable aliases or references
                // to mapped bytes exist yet. Closing another process's copy is harmless.
                let seals = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GET_SEALS) };
                let required = libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK;
                if seals < 0 || seals & required != required {
                    return Err(
                        "Mapped input requires irrevocable WRITE/GROW/SHRINK seals".to_owned()
                    );
                }
                let length =
                    usize::try_from(file.metadata().map_err(|error| error.to_string())?.len())
                        .map_err(|_| "Mapped input length is unrepresentable")?;
                let backing: Arc<dyn IoImmutableBacking> = if length == 0 {
                    Arc::new(Vec::<u8>::new())
                } else {
                    // SAFETY: required seals prevent writes through descriptors or existing
                    // writable mappings and forbid resizing; adding later seals cannot relax
                    // them. Arc ownership retains this mapping throughout every window; only
                    // immutable slices are lent and no producer can reuse its sealed storage.
                    Arc::new(SealedMapping(
                        unsafe { memmap2::MmapOptions::new().map(&file) }
                            .map_err(|error| error.to_string())?,
                    ))
                };
                Ok::<_, String>((backing, length))
            })
            .await
            .map_err(|error| error.to_string())??;
        Ok(Self {
            identifier,
            backing,
            length,
        })
    }
}

impl IoSource for SealedMappedIoSource {
    fn list(&mut self, identifier: &str) -> IoListFuture {
        let listing = if identifier == self.identifier {
            Ok(
                Box::new(MemoryIoListing::new(vec![self.identifier.clone()]))
                    as Box<dyn ipp_core::services::io::IoListing>,
            )
        } else {
            Err("Mapped source identifier does not match publication".into())
        };
        Box::pin(std::future::ready(listing))
    }

    fn open_read(&mut self, identifier: &str, options: IoReadOptions) -> IoOpenReadFuture {
        let result = if identifier != self.identifier {
            Err("Mapped source identifier does not match publication".into())
        } else if options.max_bytes.is_some_and(|limit| self.length > limit) {
            Err("Mapped input byte budget exhausted".into())
        } else {
            MappedIoReader::new(self.backing.clone(), 0..self.length)
                .map(|reader| Box::new(reader) as Box<dyn IoReader>)
        };
        Box::pin(std::future::ready(result))
    }
}

#[cfg(test)]
#[path = "mapped_input_tests.rs"]
mod tests;
