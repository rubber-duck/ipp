//! Sequential typed parsing from immutable I/O windows.

use super::IoReader;
use std::{future::poll_fn, num::NonZeroUsize, task::Poll};

const WORK_BYTES: usize = 64 << 10;
const WORK_RECORDS: usize = 1024;

/// Grow private output from validated records instead of trusting unbounded counts.
pub fn push<T>(output: &mut Vec<T>, value: T) -> Result<(), String> {
    output.try_reserve(1).map_err(|error| error.to_string())?;
    output.push(value);
    Ok(())
}

/// Yield once to the owning executor without allocating a future.
pub async fn yield_decode() {
    let mut yielded = false;
    poll_fn(|cx| {
        if yielded {
            Poll::Ready(())
        } else {
            yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    })
    .await;
}

/// Bounds CPU work even when all source windows are immediately ready.
#[derive(Default)]
pub struct DecodeBudget {
    bytes: usize,
    records: usize,
}

impl DecodeBudget {
    /// Account a decoded prefix or one logical processing step.
    pub async fn advance(&mut self, bytes: usize) {
        self.bytes = self.bytes.saturating_add(bytes);
        self.records += 1;
        if self.bytes >= WORK_BYTES || self.records >= WORK_RECORDS {
            self.bytes = 0;
            self.records = 0;
            yield_decode().await;
        }
    }
}

/// Private destination builders borrow input only within an individual read.
pub struct AssetReader<'a> {
    reader: &'a mut dyn IoReader,
    budget: DecodeBudget,
    position: u64,
}

impl<'a> AssetReader<'a> {
    /// Wrap the exact reader selected by resource acquisition.
    pub fn new(reader: &'a mut dyn IoReader) -> Self {
        Self {
            reader,
            budget: DecodeBudget::default(),
            position: 0,
        }
    }

    /// Consumed source bytes, independent of buffering and transport framing.
    pub fn position(&self) -> u64 {
        self.position
    }

    /// Read a fixed record directly from the current immutable window.
    pub async fn array<const N: usize>(&mut self) -> Result<[u8; N], String> {
        let minimum = NonZeroUsize::new(N).ok_or("Empty asset record")?;
        let window = self.reader.read(minimum).await?;
        let value: [u8; N] = window
            .bytes()
            .get(..N)
            .ok_or("Truncated asset record")?
            .try_into()
            .expect("fixed record length");
        let next = self
            .position
            .checked_add(N as u64)
            .ok_or("Asset length overflow")?;
        window.consume(N)?;
        self.position = next;
        self.budget.advance(N).await;
        Ok(value)
    }

    /// One wire tag, with no assumption about transport chunk boundaries.
    pub async fn u8(&mut self) -> Result<u8, String> {
        Ok(self.array::<1>().await?[0])
    }

    /// One little-endian wire integer.
    pub async fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.array().await?))
    }

    /// One little-endian wire integer.
    pub async fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.array().await?))
    }

    /// One finite little-endian float.
    pub async fn f32(&mut self) -> Result<f32, String> {
        let value = f32::from_le_bytes(self.array().await?);
        if !value.is_finite() {
            return Err("Nonfinite asset float".into());
        }
        Ok(value)
    }

    /// One finite little-endian float.
    pub async fn f64(&mut self) -> Result<f64, String> {
        let value = f64::from_le_bytes(self.array().await?);
        if !value.is_finite() {
            return Err("Nonfinite asset float".into());
        }
        Ok(value)
    }

    /// Decode a finite float vector into final typed storage.
    pub async fn floats<const N: usize>(&mut self) -> Result<[f32; N], String> {
        let mut values = [0.0; N];
        for value in &mut values {
            *value = self.f32().await?;
        }
        Ok(values)
    }

    /// Ordered two-dimensional bounds.
    pub async fn bounds(&mut self) -> Result<[f32; 4], String> {
        let bounds = self.floats().await?;
        if bounds[0] > bounds[2] || bounds[1] > bounds[3] {
            return Err("Invalid asset bounds".into());
        }
        Ok(bounds)
    }

    /// Fill an owned destination without assembling another encoded input buffer.
    pub async fn bytes(&mut self, count: usize) -> Result<Vec<u8>, String> {
        let mut output = Vec::new();
        output
            .try_reserve_exact(count)
            .map_err(|error| error.to_string())?;
        while output.len() < count {
            let minimum = (count - output.len()).min(WORK_BYTES);
            let window = self
                .reader
                .read(NonZeroUsize::new(minimum).unwrap())
                .await?;
            let available = window.bytes().len().min(minimum);
            if available == 0 {
                return Err("Truncated asset bytes".into());
            }
            output.extend_from_slice(&window.bytes()[..available]);
            let next = self
                .position
                .checked_add(available as u64)
                .ok_or("Asset length overflow")?;
            window.consume(available)?;
            self.position = next;
            self.budget.advance(available).await;
        }
        Ok(output)
    }

    /// Fill a final destination or fixed parser record without an intermediate vector.
    pub async fn fill(&mut self, output: &mut [u8]) -> Result<(), String> {
        let mut offset = 0;
        while offset < output.len() {
            let requested = (output.len() - offset).min(WORK_BYTES);
            let window = self
                .reader
                .read(NonZeroUsize::new(requested).unwrap())
                .await?;
            let count = window.bytes().len().min(requested);
            if count == 0 {
                return Err("Truncated asset bytes".into());
            }
            output[offset..offset + count].copy_from_slice(&window.bytes()[..count]);
            let next = self
                .position
                .checked_add(count as u64)
                .ok_or("Asset length overflow")?;
            window.consume(count)?;
            self.position = next;
            offset += count;
            self.budget.advance(count).await;
        }
        Ok(())
    }

    /// Decode a fixed-width stream directly into the caller's final typed store.
    pub async fn records(
        &mut self,
        width: NonZeroUsize,
        count: usize,
        mut decode: impl FnMut(&[u8]) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut remaining = count;
        while remaining != 0 {
            let window = self.reader.read(width).await?;
            let records = (window.bytes().len() / width.get())
                .min(remaining)
                .min((WORK_BYTES / width.get()).max(1));
            if records == 0 {
                return Err("Truncated asset record stream".into());
            }
            let count = records
                .checked_mul(width.get())
                .ok_or("Asset length overflow")?;
            for record in window.bytes()[..count].chunks_exact(width.get()) {
                decode(record)?;
            }
            let next = self
                .position
                .checked_add(count as u64)
                .ok_or("Asset length overflow")?;
            window.consume(count)?;
            self.position = next;
            remaining -= records;
            self.budget.advance(count).await;
        }
        Ok(())
    }

    /// Retain one semantic UTF-8 string, checking every source chunk before appending.
    pub async fn text(&mut self, count: usize) -> Result<String, String> {
        let mut output = String::new();
        output
            .try_reserve_exact(count)
            .map_err(|error| error.to_string())?;
        let mut remaining = count;
        let mut pending = [0; 4];
        let mut pending_len = 0;
        while remaining != 0 {
            let requested = remaining.min(WORK_BYTES);
            let window = self
                .reader
                .read(NonZeroUsize::new(requested).unwrap())
                .await?;
            let available = window.bytes().len().min(requested);
            if available == 0 {
                return Err("Truncated asset string".into());
            }
            let bytes = &window.bytes()[..available];
            let mut start = 0;
            while pending_len != 0 && start < bytes.len() {
                pending[pending_len] = bytes[start];
                pending_len += 1;
                start += 1;
                match std::str::from_utf8(&pending[..pending_len]) {
                    Ok(text) => {
                        output.push_str(text);
                        pending_len = 0;
                    }
                    Err(error) if error.error_len().is_none() && pending_len < 4 => {}
                    Err(_) => return Err("Invalid asset UTF-8".into()),
                }
            }
            match std::str::from_utf8(&bytes[start..]) {
                Ok(text) => output.push_str(text),
                Err(error) if error.error_len().is_none() => {
                    let end = start + error.valid_up_to();
                    output.push_str(std::str::from_utf8(&bytes[start..end]).unwrap());
                    let suffix = &bytes[end..];
                    pending[..suffix.len()].copy_from_slice(suffix);
                    pending_len = suffix.len();
                }
                Err(_) => return Err("Invalid asset UTF-8".into()),
            }
            let next = self
                .position
                .checked_add(available as u64)
                .ok_or("Asset length overflow")?;
            window.consume(available)?;
            self.position = next;
            remaining -= available;
            self.budget.advance(available).await;
        }
        if pending_len != 0 {
            return Err("Incomplete asset UTF-8".into());
        }
        Ok(output)
    }

    /// Require final input; declared lengths never stand in for EOF.
    pub async fn finish(&mut self) -> Result<(), String> {
        let window = self.reader.read(NonZeroUsize::new(1).unwrap()).await?;
        if !window.bytes().is_empty() {
            return Err("Trailing asset bytes".into());
        }
        if !window.is_final() {
            return Err("Asset reader returned empty nonfinal input".into());
        }
        Ok(())
    }
}
