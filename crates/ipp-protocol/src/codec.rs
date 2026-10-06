//! Bounded byte primitives shared by every protocol lane.

use crate::MAX_MESSAGE_BYTES;

/// Explicit wire rejection, before any core mutation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProtocolError {
    /// Request belongs to another connection.
    SessionMismatch,
    /// A transported World or output token no longer names its exact live lifetime.
    InvalidReference,
    /// Incomplete, invalid, or trailing data.
    Malformed(&'static str),
    /// Tag identifies no implemented operation.
    Unsupported(u8),
    /// A complete message or nested collection exceeds its bound.
    Limit(&'static str),
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ProtocolError {}

pub(crate) struct Reader<'a> {
    pub(crate) bytes: &'a [u8],
    pub(crate) at: usize,
}

pub(crate) struct Writer(pub(crate) Vec<u8>, Option<usize>);

impl Writer {
    pub(crate) fn new(bytes: Vec<u8>) -> Self {
        Self(bytes, None)
    }

    pub(crate) fn measuring() -> Self {
        Self(Vec::new(), Some(0))
    }

    pub(crate) fn len(&self) -> usize {
        self.1.unwrap_or(self.0.len())
    }

    pub(crate) fn framed(
        &mut self,
        write: impl Fn(&mut Self) -> Result<(), ProtocolError>,
    ) -> Result<(), ProtocolError> {
        let mut measurement = Self::measuring();
        write(&mut measurement)?;
        self.count(measurement.len(), MAX_MESSAGE_BYTES)?;
        write(self)
    }

    pub(crate) fn raw(&mut self, v: &[u8]) -> Result<(), ProtocolError> {
        let length = self
            .len()
            .checked_add(v.len())
            .filter(|length| *length <= MAX_MESSAGE_BYTES)
            .ok_or(ProtocolError::Limit("message"))?;
        if let Some(count) = &mut self.1 {
            *count = length;
        } else {
            self.0.extend_from_slice(v);
        }
        Ok(())
    }

    pub(crate) fn u8(&mut self, v: u8) -> Result<(), ProtocolError> {
        self.raw(&[v])
    }

    pub(crate) fn u16(&mut self, v: u16) -> Result<(), ProtocolError> {
        self.raw(&v.to_le_bytes())
    }

    pub(crate) fn u32(&mut self, v: u32) -> Result<(), ProtocolError> {
        self.raw(&v.to_le_bytes())
    }

    pub(crate) fn u64(&mut self, v: u64) -> Result<(), ProtocolError> {
        self.raw(&v.to_le_bytes())
    }

    pub(crate) fn f32(&mut self, v: f32) -> Result<(), ProtocolError> {
        if !v.is_finite() {
            return Err(ProtocolError::Malformed("nonfinite f32"));
        }
        self.raw(&v.to_le_bytes())
    }

    pub(crate) fn f64(&mut self, v: f64) -> Result<(), ProtocolError> {
        if !v.is_finite() || v < 0.0 {
            return Err(ProtocolError::Malformed("invalid simulation time"));
        }
        self.raw(&v.to_le_bytes())
    }

    pub(crate) fn count(&mut self, n: usize, max: usize) -> Result<(), ProtocolError> {
        if n > max {
            return Err(ProtocolError::Limit("count"));
        }
        self.u32(n as u32)
    }

    pub(crate) fn string(&mut self, s: &str) -> Result<(), ProtocolError> {
        self.count(s.len(), crate::MAX_FIELD_BYTES)?;
        self.raw(s.as_bytes())
    }

    pub(crate) fn bytes(&mut self, bytes: &[u8]) -> Result<(), ProtocolError> {
        self.count(bytes.len(), crate::MAX_FIELD_BYTES)?;
        self.raw(bytes)
    }
}

impl<'a> Reader<'a> {
    pub(crate) fn f64(&mut self) -> Result<f64, ProtocolError> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8], ProtocolError> {
        let end = self
            .at
            .checked_add(n)
            .ok_or(ProtocolError::Limit("length"))?;
        let v = self
            .bytes
            .get(self.at..end)
            .ok_or(ProtocolError::Malformed("truncated"))?;
        self.at = end;
        Ok(v)
    }

    pub(crate) fn u8(&mut self) -> Result<u8, ProtocolError> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn boolean(&mut self) -> Result<bool, ProtocolError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(ProtocolError::Malformed("boolean encoding")),
        }
    }

    pub(crate) fn u16(&mut self) -> Result<u16, ProtocolError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    pub(crate) fn u32(&mut self) -> Result<u32, ProtocolError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    pub(crate) fn u64(&mut self) -> Result<u64, ProtocolError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    pub(crate) fn f32(&mut self) -> Result<f32, ProtocolError> {
        let v = f32::from_bits(self.u32()?);
        if v.is_finite() {
            Ok(v)
        } else {
            Err(ProtocolError::Malformed("nonfinite f32"))
        }
    }

    pub(crate) fn count(&mut self, max: usize) -> Result<usize, ProtocolError> {
        let n = self.u32()? as usize;
        if n > max {
            Err(ProtocolError::Limit("count"))
        } else {
            Ok(n)
        }
    }

    pub(crate) fn string(&mut self) -> Result<String, ProtocolError> {
        let n = self.count(crate::MAX_FIELD_BYTES)?;
        std::str::from_utf8(self.take(n)?)
            .map(str::to_owned)
            .map_err(|_| ProtocolError::Malformed("utf8"))
    }

    /// Decode component or protocol text straight into one shared immutable allocation.
    pub(crate) fn text(&mut self) -> Result<std::sync::Arc<str>, ProtocolError> {
        let n = self.count(crate::MAX_FIELD_BYTES)?;
        std::str::from_utf8(self.take(n)?)
            .map(std::sync::Arc::from)
            .map_err(|_| ProtocolError::Malformed("utf8"))
    }

    pub(crate) fn bytes(&mut self) -> Result<Vec<u8>, ProtocolError> {
        self.bytes_bounded(crate::MAX_FIELD_BYTES)
    }

    pub(crate) fn bytes_bounded(&mut self, max: usize) -> Result<Vec<u8>, ProtocolError> {
        let n = self.count(max)?;
        Ok(self.take(n)?.to_vec())
    }
}
