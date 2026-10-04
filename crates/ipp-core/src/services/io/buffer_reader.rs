//! Direct lending from immutable, addressable backing.

use super::{
    IoError, IoPlatformSend, IoPlatformSync, IoReadBackend, IoReadWindow, IoWindowBackend,
};
use std::{
    num::NonZeroUsize,
    ops::Range,
    sync::Arc,
    task::{Context, Poll},
};

/// Addressable immutable storage retained for every reader and active window.
pub trait IoImmutableBacking: AsRef<[u8]> + IoPlatformSend + IoPlatformSync {
    /// Entire retained allocation, which may exceed its logical byte length.
    fn retained_bytes(&self) -> usize {
        self.as_ref().len()
    }

    /// File-backed virtual length, which is not an owned heap allocation.
    fn mapped_bytes(&self) -> usize {
        0
    }
}

impl IoImmutableBacking for Vec<u8> {
    fn retained_bytes(&self) -> usize {
        self.capacity()
    }
}

impl IoImmutableBacking for [u8] {}

impl IoImmutableBacking for Box<[u8]> {}

/// Independent cursor over owned or shared immutable bytes, without read copies.
pub struct BufferIoReader {
    backing: Arc<dyn IoImmutableBacking>,
    range: Range<usize>,
    cursor: usize,
}

impl BufferIoReader {
    /// Adopt an owned vector without copying into a second byte allocation.
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        let bytes = Arc::new(bytes.into());
        let length = bytes.len();
        Self {
            backing: bytes,
            range: 0..length,
            cursor: 0,
        }
    }

    /// Retain existing immutable addressable storage with a checked byte range.
    pub fn from_backing(
        backing: Arc<dyn IoImmutableBacking>,
        range: Range<usize>,
    ) -> Result<Self, IoError> {
        if range.start > range.end || range.end > backing.as_ref().as_ref().len() {
            return Err("IO backing range is invalid".into());
        }
        Ok(Self {
            backing,
            range,
            cursor: 0,
        })
    }
}

impl IoReadBackend for BufferIoReader {
    fn retained_storage(&self) -> Option<super::IoReaderStorage> {
        Some(super::IoReaderStorage {
            identity: super::IoStorageId(Arc::as_ptr(&self.backing) as *const () as usize),
            bytes: self.backing.retained_bytes(),
            mapped_bytes: self.backing.mapped_bytes(),
        })
    }

    fn poll_ready(
        &mut self,
        _cx: &mut Context<'_>,
        _minimum: NonZeroUsize,
    ) -> Poll<Result<(), IoError>> {
        Poll::Ready(Ok(()))
    }

    fn window(&mut self) -> IoReadWindow<'_> {
        IoReadWindow::new(self)
    }
}

impl IoWindowBackend for BufferIoReader {
    fn bytes(&self) -> &[u8] {
        &self.backing.as_ref().as_ref()[self.range.start + self.cursor..self.range.end]
    }

    fn is_final(&self) -> bool {
        true
    }

    fn consume(&mut self, count: usize) -> Result<(), IoError> {
        self.cursor = self.cursor.checked_add(count).ok_or("IO cursor overflow")?;
        Ok(())
    }
}

/// Reader for Host-owned immutable mapping or synchronized shared backing.
/// The Host establishes backing immutability; core checks ranges and retains it.
pub struct MappedIoReader(BufferIoReader);

impl MappedIoReader {
    /// Capture exact backing incarnation and a checked byte range.
    pub fn new(backing: Arc<dyn IoImmutableBacking>, range: Range<usize>) -> Result<Self, IoError> {
        BufferIoReader::from_backing(backing, range).map(Self)
    }
}

impl IoReadBackend for MappedIoReader {
    fn retained_storage(&self) -> Option<super::IoReaderStorage> {
        self.0.retained_storage()
    }

    fn poll_ready(
        &mut self,
        cx: &mut Context<'_>,
        minimum: NonZeroUsize,
    ) -> Poll<Result<(), IoError>> {
        self.0.poll_ready(cx, minimum)
    }

    fn window(&mut self) -> IoReadWindow<'_> {
        self.0.window()
    }
}
