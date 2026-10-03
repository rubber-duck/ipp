//! Finite ordered byte assembly; consumers own identity, admission and publication.

/// Exclusive staging for an upload with an exact caller-validated byte length.
///
/// Construction allocates nothing. Chunks grow the buffer fallibly up to the
/// declared length. Dropping the assembly cancels it and frees its staging;
/// only consuming successful completion returns bytes to the caller.
pub struct IoUploadAssembly {
    length: usize,
    bytes: Vec<u8>,
}

/// Byte assembly failures, interpreted by the consuming protocol or service.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IoUploadError {
    /// The next accumulated byte count cannot be represented.
    LengthOverflow,
    /// A chunk is empty, out of order or extends beyond the declared length.
    InvalidChunkBounds,
    /// Staging allocation failed before accepting the chunk.
    AllocationFailed,
    /// Completion arrived before the exact declared byte count.
    Incomplete,
}

impl IoUploadAssembly {
    /// The consumer validates representability and any admission budget first.
    pub fn new(length: usize) -> Self {
        Self {
            length,
            bytes: Vec::new(),
        }
    }

    /// Append a nonempty chunk at the next byte offset within the exact length.
    /// An error leaves accepted bytes unchanged; the consumer decides cancellation.
    pub fn push(&mut self, offset: u64, bytes: &[u8]) -> Result<(), IoUploadError> {
        let end = self
            .bytes
            .len()
            .checked_add(bytes.len())
            .ok_or(IoUploadError::LengthOverflow)?;
        if offset != self.bytes.len() as u64 || end > self.length || bytes.is_empty() {
            return Err(IoUploadError::InvalidChunkBounds);
        }

        self.bytes
            .try_reserve(bytes.len())
            .map_err(|_| IoUploadError::AllocationFailed)?;
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    /// Consume staging, returning complete owned bytes or discarding incomplete input.
    /// The consumer removes its transfer identity before calling this method.
    pub fn finish(self) -> Result<Vec<u8>, IoUploadError> {
        if self.bytes.len() != self.length {
            return Err(IoUploadError::Incomplete);
        }

        Ok(self.bytes)
    }
}

#[cfg(test)]
#[path = "upload_tests.rs"]
mod tests;
