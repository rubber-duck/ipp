//! Private, cooperative semantic output with Host-owned allocation accounting.

use super::AssetCpuSnapshot;
use crate::services::asset_management::decode::DecodeBudget;
use std::rc::Rc;

/// Host policy observes private output before allocation and at each work boundary.
pub trait AssetOutputObserver {
    /// Charge output capacity before growth; refusal leaves output unpublished.
    fn reserve(&self, bytes: usize) -> Result<(), String>;

    /// Fail when pressure or authorization has cancelled private work.
    fn check(&self) -> Result<(), String>;
}

/// No bytes become publishable until the owning encoder returns successfully.
pub struct AssetOutput {
    bytes: Vec<u8>,
    observer: Rc<dyn AssetOutputObserver>,
    budget: DecodeBudget,
}

impl AssetOutput {
    /// Start a private destination governed by the Host observer.
    pub fn new(observer: Rc<dyn AssetOutputObserver>) -> Self {
        Self {
            bytes: Vec::new(),
            observer,
            budget: DecodeBudget::default(),
        }
    }

    /// Append a borrowed range in bounded work quanta.
    pub async fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        for chunk in bytes.chunks(64 << 10) {
            self.observer.check()?;
            self.reserve(chunk.len())?;
            self.bytes.extend_from_slice(chunk);
            self.budget.advance(chunk.len()).await;
            self.observer.check()?;
        }
        Ok(())
    }

    /// Fill the final destination directly in bounded chunks, without another full buffer.
    /// Callback offsets start at zero for this payload range; no slice crosses an await.
    pub async fn fill(
        &mut self,
        length: usize,
        mut fill: impl FnMut(usize, &mut [u8]) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut offset = 0;
        while offset < length {
            self.observer.check()?;
            let count = (length - offset).min(64 << 10);
            let begin = self.bytes.len();
            self.reserve(count)?;
            self.bytes.resize(begin + count, 0);
            fill(offset, &mut self.bytes[begin..])?;
            offset += count;
            self.budget.advance(count).await;
            self.observer.check()?;
        }
        Ok(())
    }

    fn reserve(&mut self, count: usize) -> Result<(), String> {
        let needed = self
            .bytes
            .len()
            .checked_add(count)
            .ok_or("Asset output overflow")?;
        if needed > self.bytes.capacity() {
            let capacity = needed
                .checked_add((64 << 10) - 1)
                .ok_or("Asset output overflow")?
                & !((64 << 10) - 1);
            self.observer.reserve(capacity)?;
            self.bytes
                .try_reserve_exact(capacity - self.bytes.len())
                .map_err(|error| error.to_string())?;
            self.observer.reserve(self.bytes.capacity())?;
        }
        Ok(())
    }

    /// Append one byte tag.
    pub async fn byte(&mut self, value: u8) -> Result<(), String> {
        self.write(&[value]).await
    }

    /// Append a little-endian 32-bit word.
    pub async fn u32(&mut self, value: u32) -> Result<(), String> {
        self.write(&value.to_le_bytes()).await
    }

    /// Append a checked format count.
    pub async fn count(&mut self, value: usize) -> Result<(), String> {
        self.u32(u32::try_from(value).map_err(|_| "Asset count overflow")?)
            .await
    }

    /// Append a checked length and UTF-8 bytes.
    pub async fn text(&mut self, value: &str) -> Result<(), String> {
        self.count(value.len()).await?;
        self.write(value.as_bytes()).await
    }

    /// Append little-endian float lanes cooperatively.
    pub async fn floats(&mut self, values: &[f32]) -> Result<(), String> {
        for value in values {
            self.write(&value.to_le_bytes()).await?;
        }
        Ok(())
    }

    /// Return complete private output after the final policy check.
    pub fn finish(self) -> Result<Vec<u8>, String> {
        self.observer.check()?;
        Ok(self.bytes)
    }
}

/// Availability is independent of the memory-safety retention of an immutable payload.
pub async fn encode_cpu_snapshot(
    snapshot: AssetCpuSnapshot,
    format: super::AssetExportFormat,
    observer: Rc<dyn AssetOutputObserver>,
) -> Result<Vec<u8>, String> {
    if snapshot.available.is_cancelled() {
        return Err("CPU asset representation unavailable".into());
    }
    struct AvailableOutput {
        observer: Rc<dyn AssetOutputObserver>,
        available: crate::services::io::IoCancellation,
    }

    impl AssetOutputObserver for AvailableOutput {
        fn reserve(&self, bytes: usize) -> Result<(), String> {
            self.check()?;
            self.observer.reserve(bytes)
        }

        fn check(&self) -> Result<(), String> {
            if self.available.is_cancelled() {
                return Err("CPU asset representation unloaded during export".into());
            }
            self.observer.check()
        }
    }

    let mut output = AssetOutput::new(Rc::new(AvailableOutput {
        observer,
        available: snapshot.available.clone(),
    }));
    super::encoding::encode(&*snapshot.data, format, &mut output).await?;
    if snapshot.available.is_cancelled() {
        return Err("CPU asset representation unloaded during export".into());
    }
    output.finish()
}
