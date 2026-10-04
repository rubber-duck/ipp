//! Immutable RGBA8 CPU textures with checked dimensions and owned storage.

use crate::ErrorReason;

/// Exact texture identity, distinct from a mesh with the same numeric fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TextureKey {
    /// Nonzero logical texture identity.
    pub asset: u64,
    /// Per-entity texture variant.
    pub variant: u32,
}

/// Exclusive IPPT version 3 payload handoff to the mutation boundary.
#[derive(Debug)]
pub struct TextureUpload {
    /// Caller-supplied correlation identity.
    pub id: u64,
    /// Exact texture identity and variant.
    pub key: TextureKey,
    /// Owned encoded source bytes.
    pub bytes: Vec<u8>,
}

/// Immutable CPU pixels retained for rendering and context recovery.
#[derive(Debug)]
pub struct TextureAsset {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl TextureAsset {
    /// Width in texels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in texels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Top-left-row-first RGBA8 with sRGB colour, linear straight alpha and no padding.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Validate the complete encoded payload and decode its present attributes.
    pub fn decode(bytes: &[u8]) -> Result<Self, ErrorReason> {
        if bytes.len() < 16 || &bytes[..4] != b"IPPT" {
            return Err(ErrorReason::InvalidAsset);
        }

        let read = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        if read(4) != 3 {
            return Err(ErrorReason::InvalidAsset);
        }
        let width = read(8);
        let height = read(12);
        let pixel_bytes = pixel_bytes(width, height)?;
        if bytes.len() != pixel_bytes as usize + 16 {
            return Err(ErrorReason::InvalidAsset);
        }

        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(pixel_bytes as usize)
            .map_err(|_| ErrorReason::Capacity)?;
        pixels.extend_from_slice(&bytes[16..]);

        Ok(Self {
            width,
            height,
            pixels,
        })
    }
}

pub(crate) fn pixel_bytes(width: u32, height: u32) -> Result<u32, ErrorReason> {
    if width == 0 || height == 0 {
        return Err(ErrorReason::InvalidAsset);
    }

    let bytes = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(ErrorReason::InvalidAsset)?;
    // The encoded length uses u32. RenderDevice limits are checked by GL.
    if bytes.checked_add(16).is_none() {
        return Err(ErrorReason::InvalidAsset);
    }
    Ok(bytes)
}

/// Compiled texture type identity, independent of factory and source scheme.
pub const TEXTURE_TYPE: crate::services::asset_management::AssetTypeId =
    crate::services::asset_management::AssetTypeId(2);

impl crate::services::asset_management::Asset for TextureAsset {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn decoded(&self) -> &dyn std::any::Any {
        self
    }

    fn resident_bytes(&self) -> usize {
        self.pixels.len()
    }
}

/// Construct a headless decoder; graphics Hosts may register their own loader.
pub fn cpu_texture_loader() -> impl super::AssetLoader<Data = TextureAsset> {
    super::AsyncAssetLoader::decode(|mut reader| async move {
        TextureAsset::decode_reader(&mut *reader).await
    })
}

impl TextureAsset {
    /// Decode private CPU pixels without retaining an encoded source copy.
    pub async fn decode_reader(reader: &mut dyn super::IoReader) -> Result<Self, String> {
        let mut decoder = TextureDecoder::new();
        let info = decoder.read_header(reader).await?;
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(info.pixel_bytes as usize)
            .map_err(|error| error.to_string())?;
        let mut offset = 0;
        let mut budget = super::decode::DecodeBudget::default();
        while offset < info.pixel_bytes as usize {
            let end = (info.pixel_bytes as usize).min(offset + (64 << 10));
            pixels.resize(end, 0);
            let count = decoder
                .read_pixels(reader, &mut pixels[offset..end])
                .await?;
            offset += count;
            budget.advance(count).await;
        }
        decoder.read_pixels(reader, &mut []).await?;
        Ok(Self {
            width: info.width,
            height: info.height,
            pixels,
        })
    }
}

/// Validated streaming RGBA8 dimensions and payload size.
#[derive(Clone, Copy, Debug)]
pub struct TextureHeader {
    /// Width in texels.
    pub width: u32,
    /// Height in texels.
    pub height: u32,
    /// Exact packed RGBA8 payload size.
    pub pixel_bytes: u32,
}

/// Incremental IPPT framing. Consumers choose CPU storage or GPU row uploads.
#[derive(Default)]
pub struct TextureDecoder {
    info: Option<TextureHeader>,
    received: u32,
}

impl TextureDecoder {
    /// Validate decoded dimensions before allocating a representation.
    pub fn new() -> Self {
        Self::default()
    }

    /// Read and validate the fixed header directly from one scoped input window.
    pub async fn read_header(
        &mut self,
        reader: &mut dyn super::IoReader,
    ) -> Result<TextureHeader, String> {
        if let Some(info) = self.info {
            return Ok(info);
        }
        let mut input = super::decode::AssetReader::new(reader);
        let header = input.array::<16>().await?;
        let read = |offset| u32::from_le_bytes(header[offset..offset + 4].try_into().unwrap());
        if &header[..4] != b"IPPT" || read(4) != 3 {
            return Err("InvalidAsset: unsupported IPPT header".into());
        }
        let width = read(8);
        let height = read(12);
        let info = TextureHeader {
            width,
            height,
            pixel_bytes: pixel_bytes(width, height).map_err(|error| error.to_string())?,
        };
        self.info = Some(info);
        Ok(info)
    }

    /// Copy into the consumer's final storage and verify exact final EOF.
    pub async fn read_pixels(
        &mut self,
        reader: &mut dyn super::IoReader,
        output: &mut [u8],
    ) -> Result<usize, String> {
        let info = self.info.ok_or("Texture header has not been read")?;
        if self.received == info.pixel_bytes {
            super::decode::AssetReader::new(reader).finish().await?;
            return Ok(0);
        }
        let length = output
            .len()
            .min((info.pixel_bytes - self.received) as usize)
            .min(64 << 10);
        let minimum = std::num::NonZeroUsize::new(length)
            .ok_or("Texture consumer supplied an empty buffer")?;
        let window = reader.read(minimum).await?;
        let count = window.bytes().len().min(length);
        if count == 0 {
            return Err("InvalidAsset: incomplete texture pixels".into());
        }
        output[..count].copy_from_slice(&window.bytes()[..count]);
        window.consume(count)?;
        self.received += count as u32;
        Ok(count)
    }
}

impl super::writer::AssetEncoder for TextureAsset {
    fn encode_asset(&self, max_bytes: usize) -> Result<Vec<u8>, String> {
        let length = self
            .pixels
            .len()
            .checked_add(16)
            .ok_or("Texture output overflow")?;
        if length > max_bytes {
            return Err("Texture output byte budget exhausted".into());
        }
        let mut bytes = Vec::with_capacity(length);
        bytes.extend_from_slice(b"IPPT");
        for value in [3, self.width, self.height] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&self.pixels);
        Ok(bytes)
    }
}
