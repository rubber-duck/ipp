//! Bounded immutable ZIP input. Stored and DEFLATE entries share ordinary readers.

use super::{DataReadOptions, DataReader, DataSource, MemoryDataReader};
use std::{collections::BTreeMap, ops::Range};

struct ZipEntry {
    encoded: Range<usize>,
    decoded_bytes: usize,
    method: u16,
    crc: u32,
}

/// Read-only ZIP source owning an immutable archive.
/// Supports ordinary single-disk ZIP with stored or DEFLATE entries. Encryption,
/// ZIP64 and non-UTF-8 identifiers fail explicitly rather than exposing partial data.
pub struct ZipDataSource {
    prefix: String,
    bytes: Vec<u8>,
    entries: BTreeMap<String, ZipEntry>,
}

impl ZipDataSource {
    /// Validate an archive catalog before registering it with the Host.
    pub fn new(prefix: &str, bytes: Vec<u8>) -> Result<Self, String> {
        let end = (bytes.len().saturating_sub(65557)..bytes.len().saturating_sub(21))
            .rev()
            .find(|&offset| {
                bytes.get(offset..offset + 4) == Some(b"PK\x05\x06")
                    && u16_at(&bytes, offset + 20)
                        .is_ok_and(|comment| offset + 22 + usize::from(comment) == bytes.len())
            })
            .ok_or("ZIP end record is unavailable")?;
        let count = u16_at(&bytes, end + 10)?;
        if count == u16::MAX
            || count > 4096
            || u16_at(&bytes, end + 4)? != 0
            || u16_at(&bytes, end + 6)? != 0
            || u16_at(&bytes, end + 8)? != count
        {
            return Err("ZIP catalog is too large, multi-disk or ZIP64".into());
        }
        let directory = u32_at(&bytes, end + 16)? as usize;
        let directory_len = u32_at(&bytes, end + 12)? as usize;
        if directory.checked_add(directory_len) != Some(end) {
            return Err("Invalid ZIP directory bounds".into());
        }
        let mut cursor = directory;
        let mut entries = BTreeMap::new();
        for _ in 0..count {
            if bytes.get(cursor..cursor + 4) != Some(b"PK\x01\x02") {
                return Err("Invalid ZIP directory entry".into());
            }
            let flags = u16_at(&bytes, cursor + 8)?;
            let method = u16_at(&bytes, cursor + 10)?;
            let crc = u32_at(&bytes, cursor + 16)?;
            let encoded_len = u32_at(&bytes, cursor + 20)? as usize;
            let decoded_bytes = u32_at(&bytes, cursor + 24)? as usize;
            let name_len = usize::from(u16_at(&bytes, cursor + 28)?);
            let extra_len = usize::from(u16_at(&bytes, cursor + 30)?);
            let comment_len = usize::from(u16_at(&bytes, cursor + 32)?);
            let local = u32_at(&bytes, cursor + 42)? as usize;
            if flags & !0x80e != 0
                || !matches!(method, 0 | 8)
                || u16_at(&bytes, cursor + 34)? != 0
                || decoded_bytes == u32::MAX as usize
                || encoded_len == u32::MAX as usize
            {
                return Err("Unsupported ZIP encryption, encoding or ZIP64 entry".into());
            }
            let name = bytes
                .get(cursor + 46..cursor + 46 + name_len)
                .ok_or("Truncated ZIP name")?;
            let name_text = std::str::from_utf8(name).map_err(|_| "ZIP names must be UTF-8")?;
            if name_text.is_empty()
                || name_text.starts_with('/')
                || name_text.split('/').any(|part| part == "..")
                || name_text.contains('\\')
            {
                return Err("Invalid ZIP entry identifier".into());
            }
            if local >= directory
                || bytes.get(local..local + 4) != Some(b"PK\x03\x04")
                || u16_at(&bytes, local + 6)? != flags
                || u16_at(&bytes, local + 8)? != method
                || usize::from(u16_at(&bytes, local + 26)?) != name_len
            {
                return Err("ZIP local header differs from its directory".into());
            }
            if flags & 8 == 0
                && (u32_at(&bytes, local + 14)? != crc
                    || u32_at(&bytes, local + 18)? as usize != encoded_len
                    || u32_at(&bytes, local + 22)? as usize != decoded_bytes)
            {
                return Err("ZIP local sizes or checksum differ from its directory".into());
            }
            let data = local
                .checked_add(30 + name_len + usize::from(u16_at(&bytes, local + 28)?))
                .ok_or("ZIP entry offset overflow")?;
            let data_end = data
                .checked_add(encoded_len)
                .ok_or("ZIP entry size overflow")?;
            if data_end > directory || bytes.get(local + 30..local + 30 + name_len) != Some(name) {
                return Err("Invalid ZIP entry bounds".into());
            }
            let identifier = format!("{prefix}{name_text}");
            if entries
                .insert(
                    identifier,
                    ZipEntry {
                        encoded: data..data_end,
                        decoded_bytes,
                        method,
                        crc,
                    },
                )
                .is_some()
            {
                return Err("Duplicate ZIP entry".into());
            }
            cursor = cursor
                .checked_add(46 + name_len + extra_len + comment_len)
                .ok_or("ZIP directory offset overflow")?;
            if cursor > end {
                return Err("Truncated ZIP directory".into());
            }
        }
        if cursor != end {
            return Err("ZIP directory size mismatch".into());
        }
        Ok(Self {
            prefix: prefix.to_owned(),
            bytes,
            entries,
        })
    }
}

impl DataSource for ZipDataSource {
    fn list(&mut self, identifier: &str) -> Result<Vec<String>, String> {
        if !identifier.starts_with(&self.prefix) {
            return Err("ZIP prefix mismatch".into());
        }
        Ok(self
            .entries
            .keys()
            .filter(|entry| entry.starts_with(identifier))
            .cloned()
            .collect())
    }

    fn open_read(
        &mut self,
        identifier: &str,
        options: DataReadOptions,
    ) -> Result<Box<dyn DataReader>, String> {
        let entry = self
            .entries
            .get(identifier)
            .ok_or("ZIP entry is unavailable")?;
        if options
            .max_bytes
            .is_some_and(|limit| entry.decoded_bytes > limit)
        {
            return Err("ZIP decoded byte budget exhausted".into());
        }
        let encoded = &self.bytes[entry.encoded.clone()];
        let bytes = match entry.method {
            0 => {
                if encoded.len() != entry.decoded_bytes {
                    return Err("ZIP stored size mismatch".into());
                }
                encoded.to_vec()
            }
            8 => miniz_oxide::inflate::decompress_to_vec_with_limit(encoded, entry.decoded_bytes)
                .map_err(|_| "Invalid or oversized ZIP DEFLATE entry")?,
            _ => unreachable!("validated ZIP method"),
        };
        if bytes.len() != entry.decoded_bytes || crc32(&bytes) != entry.crc {
            return Err("ZIP decoded size or checksum mismatch".into());
        }
        Ok(Box::new(MemoryDataReader::new(bytes)))
    }
}

fn u16_at(bytes: &[u8], offset: usize) -> Result<u16, String> {
    Ok(u16::from_le_bytes(
        bytes
            .get(offset..offset + 2)
            .ok_or("Truncated ZIP input")?
            .try_into()
            .expect("fixed field"),
    ))
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..offset + 4)
            .ok_or("Truncated ZIP input")?
            .try_into()
            .expect("fixed field"),
    ))
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}
