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

/// Accepted source and decoded sizes, excluding container overhead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextureStats {
    /// Encoded source byte count.
    pub source_bytes: u32,
    /// Retained RGBA8 pixel byte count.
    pub pixel_bytes: u32,
    /// Width in texels.
    pub width: u32,
    /// Height in texels.
    pub height: u32,
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
    pub fn decode(bytes: &[u8]) -> Result<(Self, TextureStats), ErrorReason> {
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

        Ok((
            Self {
                width,
                height,
                pixels,
            },
            TextureStats {
                source_bytes: pixel_bytes + 16,
                pixel_bytes,
                width,
                height,
            },
        ))
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
    // Stats and the encoded length use u32. RenderDevice limits are checked by GL.
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
    super::BufferedAssetLoader::new(move |bytes| {
        TextureAsset::decode(bytes)
            .map(|(data, _)| data)
            .map_err(|error| error.to_string())
    })
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
    header: [u8; 16],
    header_bytes: usize,
    info: Option<TextureHeader>,
    received: u32,
}

impl TextureDecoder {
    /// Validate decoded dimensions before allocating a representation.
    pub fn new() -> Self {
        Self {
            header: [0; 16],
            header_bytes: 0,
            info: None,
            received: 0,
        }
    }

    /// Read and validate only the fixed-size header.
    pub fn poll_header(
        &mut self,
        reader: &mut dyn crate::services::asset_management::DataReader,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<TextureHeader, String>> {
        use std::task::Poll;
        if let Some(info) = self.info {
            return Poll::Ready(Ok(info));
        }
        while self.header_bytes < 16 {
            match reader.poll_read(cx, &mut self.header[self.header_bytes..]) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Ready(Ok(0)) => return Poll::Ready(Err("Incomplete IPPT header".into())),
                Poll::Ready(Ok(n)) if n <= 16 - self.header_bytes => self.header_bytes += n,
                Poll::Ready(Ok(_)) => return Poll::Ready(Err("Invalid reader byte count".into())),
            }
        }
        let read = |offset| {
            u32::from_le_bytes(
                self.header[offset..offset + 4]
                    .try_into()
                    .expect("fixed header"),
            )
        };
        if &self.header[..4] != b"IPPT" || read(4) != 3 {
            return Poll::Ready(Err("InvalidAsset: unsupported IPPT header".into()));
        }
        let width = read(8);
        let height = read(12);
        let pixels = match pixel_bytes(width, height) {
            Ok(bytes) => bytes,
            Err(error) => return Poll::Ready(Err(error.to_string())),
        };
        let info = TextureHeader {
            width,
            height,
            pixel_bytes: pixels,
        };
        self.info = Some(info);
        Poll::Ready(Ok(info))
    }

    /// Read packed pixels and verify exact EOF after the declared payload.
    pub fn poll_pixels(
        &mut self,
        reader: &mut dyn crate::services::asset_management::DataReader,
        cx: &mut std::task::Context<'_>,
        output: &mut [u8],
    ) -> std::task::Poll<Result<usize, String>> {
        use std::task::Poll;
        let Some(info) = self.info else {
            return Poll::Ready(Err("Texture header has not been read".into()));
        };
        if self.received == info.pixel_bytes {
            return match reader.poll_read(cx, &mut [0]) {
                Poll::Ready(Ok(0)) => Poll::Ready(Ok(0)),
                Poll::Ready(Ok(_)) => {
                    Poll::Ready(Err("InvalidAsset: trailing texture bytes".into()))
                }
                result => result,
            };
        }
        let length = output
            .len()
            .min((info.pixel_bytes - self.received) as usize);
        if length == 0 {
            return Poll::Ready(Err("Texture consumer supplied an empty buffer".into()));
        }
        match reader.poll_read(cx, &mut output[..length]) {
            Poll::Ready(Ok(0)) => {
                Poll::Ready(Err("InvalidAsset: incomplete texture pixels".into()))
            }
            Poll::Ready(Ok(n)) if n <= length => {
                self.received += n as u32;
                Poll::Ready(Ok(n))
            }
            Poll::Ready(Ok(_)) => Poll::Ready(Err("Invalid reader byte count".into())),
            result => result,
        }
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
