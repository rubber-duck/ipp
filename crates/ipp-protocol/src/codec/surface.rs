//! Bounded Surface edit framing. Collection persistence is owned by ipp-core.

use super::{ProtocolError, Reader};
use ipp_core::services::asset_management::{AssetSource, AssetTypeId};
use ipp_core::systems::surface::{
    PositionedGlyph, SurfaceCommand, SurfaceItemContent, SurfaceItemId, SurfaceItemPatch,
    SurfaceItemStyle,
};

impl Reader<'_> {
    pub(super) fn surface_command(&mut self) -> Result<SurfaceCommand, ProtocolError> {
        let bytes = self.bytes()?;
        let mut r = Reader {
            bytes: &bytes,
            at: 0,
        };
        if r.u8()? != 1 {
            return Err(ProtocolError::Malformed("Surface edit version"));
        }
        let action = r.u8()?;
        let entity = ipp_core::EntityId::from_bits(r.u64()?);
        let id = SurfaceItemId(r.u32()?);
        let command = match action {
            1 => SurfaceCommand::Insert {
                entity,
                id,
                index: r.u32()?,
                content: r.surface_content()?,
                style: SurfaceItemStyle {
                    position: r.surface_vector()?,
                    scale: r.surface_vector()?,
                    color: r.surface_vector()?,
                    opacity: r.f32()?,
                    font_size: r.f32()?,
                    asset: r.surface_asset()?,
                },
            },
            2 => {
                let mask = r.u8()?;
                if mask & !127 != 0 {
                    return Err(ProtocolError::Malformed("Surface edit mask"));
                }
                SurfaceCommand::Update {
                    entity,
                    id,
                    patch: SurfaceItemPatch {
                        content: (mask & 1 != 0).then(|| r.surface_content()).transpose()?,
                        position: (mask & 2 != 0).then(|| r.surface_vector()).transpose()?,
                        scale: (mask & 4 != 0).then(|| r.surface_vector()).transpose()?,
                        color: (mask & 8 != 0).then(|| r.surface_vector()).transpose()?,
                        opacity: (mask & 16 != 0).then(|| r.f32()).transpose()?,
                        font_size: (mask & 32 != 0).then(|| r.f32()).transpose()?,
                        asset: (mask & 64 != 0).then(|| r.surface_asset()).transpose()?,
                    },
                }
            }
            3 => SurfaceCommand::Remove {
                entity,
                id,
            },
            4 => SurfaceCommand::Move {
                entity,
                id,
                index: r.u32()?,
            },
            _ => return Err(ProtocolError::Malformed("Surface edit action")),
        };
        if r.at != bytes.len() {
            return Err(ProtocolError::Malformed("trailing Surface edit bytes"));
        }
        Ok(command)
    }

    fn surface_vector<const N: usize>(&mut self) -> Result<[f32; N], ProtocolError> {
        let mut values = [0.0; N];
        for value in &mut values {
            *value = self.f32()?;
        }
        Ok(values)
    }

    fn surface_asset(&mut self) -> Result<Option<AssetSource>, ProtocolError> {
        if !self.boolean()? {
            return Ok(None);
        }
        Ok(Some(AssetSource {
            kind: AssetTypeId(self.u16()?),
            variant: self.u32()?,
            uri: self.string()?,
        }))
    }

    fn surface_content(&mut self) -> Result<SurfaceItemContent, ProtocolError> {
        Ok(match self.u8()? {
            1 => SurfaceItemContent::Label(self.string()?),
            2 => {
                let count = self.count(65_536)?;
                if count > (self.bytes.len() - self.at) / 13 {
                    return Err(ProtocolError::Malformed("truncated Surface glyph run"));
                }
                let mut glyphs = Vec::with_capacity(count);
                for _ in 0..count {
                    glyphs.push(PositionedGlyph {
                        glyph_id: self.u32()?,
                        position: self.surface_vector()?,
                        color: if self.boolean()? {
                            Some(self.surface_vector()?)
                        } else {
                            None
                        },
                    });
                }
                SurfaceItemContent::GlyphRun(glyphs)
            }
            3 => SurfaceItemContent::Drawing,
            4 => SurfaceItemContent::Bitmap {
                size: self.surface_vector()?,
            },
            _ => return Err(ProtocolError::Malformed("Surface content tag")),
        })
    }
}

#[cfg(test)]
#[path = "surface_tests.rs"]
mod tests;
