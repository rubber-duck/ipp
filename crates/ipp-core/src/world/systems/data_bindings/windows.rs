//! IPPW v1: magic, u16 version, u16 constraint count, followed by constraints.
//! Count: tag 0 + LE u64. Range: tag 1 + LE u16 UTF-8 name length + name + LE
//! f64 width + anchor. Anchors: 0 Latest, 1 HostTime + LE f64 units, 2 Supplied
//! followed by a canonical 5-byte scalar DynamicValue (F32/I32/U32). Empty bytes mean no
//! constraints. The codec bounds 256 constraints, 4096-byte names and 64 KiB.

use crate::{DynamicValue, ErrorReason, services::data::*};

const MAX_BYTES: usize = 65536;
const MAX_WINDOWS: usize = 256;
const MAX_NAME: usize = 4096;

/// Encode validated raw windows into bounded, target-portable authored bytes.
pub fn encode_data_windows(windows: &[DataWindow]) -> Result<Vec<u8>, ErrorReason> {
    validate(windows)?;
    if windows.is_empty() {
        return Ok(Vec::new());
    }
    let mut bytes = b"IPPW\x01\x00".to_vec();
    bytes.extend_from_slice(&(windows.len() as u16).to_le_bytes());
    for window in windows {
        match window {
            DataWindow::Count(count) => {
                bytes.push(0);
                bytes.extend_from_slice(
                    &u64::try_from(*count)
                        .map_err(|_| ErrorReason::InvalidValue)?
                        .to_le_bytes(),
                );
            }
            DataWindow::Range {
                column,
                width,
                anchor,
            } => {
                bytes.push(1);
                bytes.extend_from_slice(&(column.len() as u16).to_le_bytes());
                bytes.extend_from_slice(column.as_bytes());
                bytes.extend_from_slice(&width.to_le_bytes());
                match anchor {
                    DataWindowAnchor::Latest => bytes.push(0),
                    DataWindowAnchor::HostTime {
                        units_per_second,
                    } => {
                        bytes.push(1);
                        bytes.extend_from_slice(&units_per_second.to_le_bytes());
                    }
                    DataWindowAnchor::Supplied(value) => {
                        bytes.push(2);
                        bytes.extend_from_slice(&value.encode());
                    }
                }
            }
        }
        if bytes.len() > MAX_BYTES {
            return Err(ErrorReason::InvalidValue);
        }
    }
    Ok(bytes)
}

/// Decode and validate a complete authored IPPW window field; reject trailing bytes.
pub fn decode_data_windows(bytes: &[u8]) -> Result<Vec<DataWindow>, ErrorReason> {
    if bytes.is_empty() {
        return Ok(vec![]);
    }
    if bytes.len() > MAX_BYTES {
        return Err(ErrorReason::InvalidValue);
    }
    let mut reader = Reader(bytes);
    if reader.take(6)? != b"IPPW\x01\x00" {
        return Err(ErrorReason::InvalidValue);
    }
    let count = usize::from(u16::from_le_bytes(reader.array()?));
    if count > MAX_WINDOWS {
        return Err(ErrorReason::InvalidValue);
    }
    let mut windows = Vec::with_capacity(count);
    for _ in 0..count {
        windows.push(match reader.byte()? {
            0 => DataWindow::Count(
                usize::try_from(u64::from_le_bytes(reader.array()?))
                    .map_err(|_| ErrorReason::InvalidValue)?,
            ),
            1 => {
                let length = usize::from(u16::from_le_bytes(reader.array()?));
                if length > MAX_NAME {
                    return Err(ErrorReason::InvalidValue);
                }
                let column = std::str::from_utf8(reader.take(length)?)
                    .map_err(|_| ErrorReason::InvalidValue)?
                    .to_owned();
                let width = f64::from_le_bytes(reader.array()?);
                let anchor = match reader.byte()? {
                    0 => DataWindowAnchor::Latest,
                    1 => DataWindowAnchor::HostTime {
                        units_per_second: f64::from_le_bytes(reader.array()?),
                    },
                    2 => DataWindowAnchor::Supplied(
                        DynamicValue::decode(reader.take(5)?)
                            .map_err(|_| ErrorReason::InvalidValue)?,
                    ),
                    _ => return Err(ErrorReason::InvalidValue),
                };
                DataWindow::Range {
                    column,
                    width,
                    anchor,
                }
            }
            _ => return Err(ErrorReason::InvalidValue),
        });
    }
    if !reader.0.is_empty() {
        return Err(ErrorReason::InvalidValue);
    }
    validate(&windows)?;
    Ok(windows)
}

fn validate(windows: &[DataWindow]) -> Result<(), ErrorReason> {
    if windows.len() > MAX_WINDOWS
        || windows.iter().any(
            |window| matches!(window, DataWindow::Range { column, .. } if column.len() > MAX_NAME),
        )
    {
        return Err(ErrorReason::InvalidValue);
    }
    DataConsumerRequest {
        name: "validation".into(),
        kind: DataSourceKind::Streaming,
        windows: windows.to_vec(),
    }
    .validate()
    .map_err(|_| ErrorReason::InvalidValue)
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], ErrorReason> {
        let value = self.0.get(..length).ok_or(ErrorReason::InvalidValue)?;
        self.0 = &self.0[length..];
        Ok(value)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], ErrorReason> {
        self.take(N)?
            .try_into()
            .map_err(|_| ErrorReason::InvalidValue)
    }

    fn byte(&mut self) -> Result<u8, ErrorReason> {
        Ok(self.array::<1>()?[0])
    }
}

#[cfg(test)]
#[path = "windows_tests.rs"]
mod tests;
