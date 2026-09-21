//! Checked little-endian primitives for the durable World container.

pub(crate) struct WorldBinaryWriter {
    pub bytes: Vec<u8>,
    limit: usize,
}

impl WorldBinaryWriter {
    pub fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }

    pub fn raw(&mut self, bytes: &[u8]) -> Result<(), String> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err("World file byte budget exhausted".into());
        }
        let required = self.bytes.len() + bytes.len();
        if required > self.bytes.capacity() {
            let capacity = required
                .max(self.bytes.capacity().saturating_mul(2))
                .max(4096)
                .min(self.limit);
            self.bytes
                .try_reserve_exact(capacity - self.bytes.len())
                .map_err(|error| error.to_string())?;
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    pub fn u8(&mut self, value: u8) -> Result<(), String> {
        self.raw(&[value])
    }

    pub fn u16(&mut self, value: u16) -> Result<(), String> {
        self.raw(&value.to_le_bytes())
    }

    pub fn u32(&mut self, value: u32) -> Result<(), String> {
        self.raw(&value.to_le_bytes())
    }

    pub fn u64(&mut self, value: u64) -> Result<(), String> {
        self.raw(&value.to_le_bytes())
    }

    pub fn count(&mut self, value: usize) -> Result<(), String> {
        self.u32(u32::try_from(value).map_err(|_| "World count exceeds format range")?)
    }

    pub fn blob(&mut self, value: &[u8]) -> Result<(), String> {
        self.count(value.len())?;
        self.raw(value)
    }

    pub fn string(&mut self, value: &str) -> Result<(), String> {
        self.blob(value.as_bytes())
    }
}

pub(crate) struct WorldBinaryReader<'a> {
    bytes: &'a [u8],
    offset: usize,
    allocation_remaining: usize,
}

impl<'a> WorldBinaryReader<'a> {
    pub fn new(bytes: &'a [u8], allocation_limit: usize) -> Self {
        Self {
            bytes,
            offset: 0,
            allocation_remaining: allocation_limit,
        }
    }

    pub fn claim(&mut self, bytes: usize) -> Result<(), String> {
        self.allocation_remaining = self
            .allocation_remaining
            .checked_sub(bytes)
            .ok_or("World decoded state byte budget exhausted")?;
        Ok(())
    }

    pub fn remaining(&self) -> usize {
        self.bytes.len() - self.offset
    }

    pub fn raw(&mut self, length: usize) -> Result<&'a [u8], String> {
        if length > self.remaining() {
            return Err("Truncated World file".into());
        }
        let bytes = &self.bytes[self.offset..self.offset + length];
        self.offset += length;
        Ok(bytes)
    }

    pub fn u8(&mut self) -> Result<u8, String> {
        Ok(self.raw(1)?[0])
    }

    pub fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(
            self.raw(2)?.try_into().expect("checked length"),
        ))
    }

    pub fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(
            self.raw(4)?.try_into().expect("checked length"),
        ))
    }

    pub fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(
            self.raw(8)?.try_into().expect("checked length"),
        ))
    }

    pub fn count(&mut self, minimum_bytes: usize) -> Result<usize, String> {
        let count = self.u32()? as usize;
        if count > self.remaining() / minimum_bytes.max(1) {
            return Err("World count exceeds remaining data".into());
        }
        Ok(count)
    }

    pub fn blob(&mut self) -> Result<&'a [u8], String> {
        let n = self.count(1)?;
        self.raw(n)
    }

    pub fn string(&mut self) -> Result<String, String> {
        let bytes = self.blob()?;
        self.claim(bytes.len().saturating_mul(2).saturating_add(64))?;
        String::from_utf8(bytes.to_vec()).map_err(|_| "Invalid World UTF-8".into())
    }

    pub fn end(&self) -> Result<(), String> {
        if self.remaining() == 0 {
            Ok(())
        } else {
            Err("Trailing World file data".into())
        }
    }
}
