//! Immutable static font metrics and quadratic glyph outlines.

use super::{Asset, AssetLoader, AssetTypeId, BufferedAssetLoader, quadratic::Decoder};
use crate::ErrorReason;

/// Compiled identity for portable IPPF font assets.
pub const FONT_TYPE: AssetTypeId = AssetTypeId(17);

/// Metrics and closed outline contours for one original glyph ID.
#[derive(Clone, Debug, PartialEq)]
pub struct FontGlyph {
    /// Horizontal advance in font units.
    pub advance: f32,
    /// Horizontal bearing in font units.
    pub left_side_bearing: f32,
    /// Outline bounds `[min_x, min_y, max_x, max_y]` in font units.
    pub bounds: [f32; 4],
    /// Decomposed closed glyph contours.
    pub contours: Vec<super::quadratic::QuadraticContour>,
}

/// One sorted Unicode scalar to original glyph ID mapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontCmapEntry {
    /// Unicode scalar value.
    pub codepoint: u32,
    /// Index into [`FontAsset::glyphs`].
    pub glyph_id: u32,
}

/// One sorted horizontal pair adjustment in font units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontKerningPair {
    /// Original left glyph ID.
    pub left: u32,
    /// Original right glyph ID.
    pub right: u32,
    /// Added horizontal advance in font units.
    pub adjustment: f32,
}

/// Immutable decoded static TrueType data for headless layout and rendering.
#[derive(Debug)]
pub struct FontAsset {
    units_per_em: u32,
    ascender: f32,
    descender: f32,
    line_gap: f32,
    glyphs: Vec<FontGlyph>,
    cmap: Vec<FontCmapEntry>,
    kerning: Vec<FontKerningPair>,
}

impl FontAsset {
    /// Font coordinate units per em.
    pub fn units_per_em(&self) -> u32 {
        self.units_per_em
    }

    /// Typographic ascender in font units.
    pub fn ascender(&self) -> f32 {
        self.ascender
    }

    /// Typographic descender in font units.
    pub fn descender(&self) -> f32 {
        self.descender
    }

    /// Additional baseline gap in font units.
    pub fn line_gap(&self) -> f32 {
        self.line_gap
    }

    /// Glyphs in original glyph ID order, including `.notdef` at zero.
    pub fn glyphs(&self) -> &[FontGlyph] {
        &self.glyphs
    }

    /// Resolve an original glyph ID.
    pub fn glyph(&self, glyph_id: u32) -> Option<&FontGlyph> {
        self.glyphs.get(glyph_id as usize)
    }

    /// Sorted Unicode cmap entries.
    pub fn cmap(&self) -> &[FontCmapEntry] {
        &self.cmap
    }

    /// Sorted horizontal kerning pairs.
    pub fn kerning_pairs(&self) -> &[FontKerningPair] {
        &self.kerning
    }

    /// Resolve a Unicode scalar to its original glyph ID.
    pub fn glyph_id(&self, codepoint: char) -> Option<u32> {
        self.cmap
            .binary_search_by_key(&(codepoint as u32), |entry| entry.codepoint)
            .ok()
            .map(|index| self.cmap[index].glyph_id)
    }

    /// Find a pair adjustment, returning zero for an absent pair.
    pub fn kerning(&self, left: u32, right: u32) -> f32 {
        self.kerning
            .binary_search_by_key(&(left, right), |pair| (pair.left, pair.right))
            .ok()
            .map_or(0.0, |index| self.kerning[index].adjustment)
    }

    /// Validate and decode one complete IPPF version 1 payload.
    pub fn decode(bytes: &[u8]) -> Result<Self, ErrorReason> {
        if bytes.len() < 36 || &bytes[..4] != b"IPPF" {
            return Err(ErrorReason::InvalidAsset);
        }
        let mut decoder = Decoder::new(bytes, 4);
        if decoder.u32()? != 1 {
            return Err(ErrorReason::InvalidAsset);
        }
        let units_per_em = decoder.u32()?;
        if units_per_em == 0 {
            return Err(ErrorReason::InvalidAsset);
        }
        let ascender = decoder.f32()?;
        let descender = decoder.f32()?;
        let line_gap = decoder.f32()?;
        let glyph_count = decoder.u32()?;
        let cmap_count = decoder.u32()?;
        let kerning_count = decoder.u32()?;
        if glyph_count == 0
            || glyph_count > 1_000_000
            || glyph_count as usize > decoder.remaining() / 28
        {
            return Err(ErrorReason::InvalidAsset);
        }
        let mut glyphs = Vec::new();
        glyphs
            .try_reserve_exact(glyph_count as usize)
            .map_err(|_| ErrorReason::Capacity)?;
        for _ in 0..glyph_count {
            let advance = decoder.f32()?;
            let left_side_bearing = decoder.f32()?;
            let bounds = decoder.bounds()?;
            let contour_count = decoder.u32()?;
            glyphs.push(FontGlyph {
                advance,
                left_side_bearing,
                bounds,
                contours: decoder.contours(contour_count)?,
            });
        }
        let mut cmap = Vec::new();
        if cmap_count as usize > decoder.remaining() / 8 {
            return Err(ErrorReason::InvalidAsset);
        }
        cmap.try_reserve_exact(cmap_count as usize)
            .map_err(|_| ErrorReason::Capacity)?;
        for _ in 0..cmap_count {
            let entry = FontCmapEntry {
                codepoint: decoder.u32()?,
                glyph_id: decoder.u32()?,
            };
            if entry.codepoint > 0x10ffff
                || entry.glyph_id >= glyph_count
                || cmap
                    .last()
                    .is_some_and(|previous: &FontCmapEntry| previous.codepoint >= entry.codepoint)
            {
                return Err(ErrorReason::InvalidAsset);
            }
            cmap.push(entry);
        }
        let mut kerning = Vec::new();
        if kerning_count as usize > decoder.remaining() / 12 {
            return Err(ErrorReason::InvalidAsset);
        }
        kerning
            .try_reserve_exact(kerning_count as usize)
            .map_err(|_| ErrorReason::Capacity)?;
        for _ in 0..kerning_count {
            let pair = FontKerningPair {
                left: decoder.u32()?,
                right: decoder.u32()?,
                adjustment: decoder.f32()?,
            };
            if pair.left >= glyph_count
                || pair.right >= glyph_count
                || kerning.last().is_some_and(|previous: &FontKerningPair| {
                    (previous.left, previous.right) >= (pair.left, pair.right)
                })
            {
                return Err(ErrorReason::InvalidAsset);
            }
            kerning.push(pair);
        }
        if decoder.position() != bytes.len() {
            return Err(ErrorReason::InvalidAsset);
        }
        Ok(Self {
            units_per_em,
            ascender,
            descender,
            line_gap,
            glyphs,
            cmap,
            kerning,
        })
    }
}

impl Asset for FontAsset {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn decoded(&self) -> &dyn std::any::Any {
        self
    }

    fn resident_bytes(&self) -> usize {
        self.glyphs
            .iter()
            .map(|glyph| {
                glyph
                    .contours
                    .iter()
                    .map(|contour| std::mem::size_of_val(contour.segments.as_slice()))
                    .sum::<usize>()
            })
            .sum::<usize>()
            + std::mem::size_of_val(self.glyphs.as_slice())
            + std::mem::size_of_val(self.cmap.as_slice())
            + std::mem::size_of_val(self.kerning.as_slice())
    }
}

/// Construct a streaming buffered CPU loader.
pub fn cpu_font_loader() -> impl AssetLoader<Data = FontAsset> {
    BufferedAssetLoader::new(|bytes| FontAsset::decode(bytes).map_err(|error| error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_font() -> Vec<u8> {
        let mut bytes = b"IPPF".to_vec();
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&1000_u32.to_le_bytes());
        for value in [800.0_f32, -200.0, 100.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in [1_u32, 1, 0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in [500.0_f32, 10.0, 0.0, 0.0, 0.0, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&65_u32.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes
    }

    #[test]
    fn decodes_original_glyph_identity_and_metrics() {
        let font = FontAsset::decode(&minimal_font()).unwrap();

        assert_eq!(font.glyph_id('A'), Some(0));
        assert_eq!(font.glyph(0).unwrap().advance, 500.0);
        assert_eq!(font.kerning(0, 0), 0.0);
    }

    #[test]
    fn rejects_trailing_and_nonfinite_data() {
        let mut trailing = minimal_font();
        trailing.push(0);
        assert!(FontAsset::decode(&trailing).is_err());

        let mut nonfinite = minimal_font();
        nonfinite[12..16].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(FontAsset::decode(&nonfinite).is_err());
    }
}
