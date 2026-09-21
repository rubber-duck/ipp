use crate::components::schema::{ContractSink, FieldError, FieldKind, FieldValue, SchemaField};

const CODEC_VERSION: u8 = 1;
const MAX_ITEMS: usize = 65_536;
const MAX_GLYPHS: usize = 1_000_000;
const MAX_TEXT_BYTES: usize = 16 * 1024 * 1024;

/// Stable component-local item identity. Zero is never valid and identities are never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SurfaceItemId(pub u32);

/// One client-positioned glyph. Coordinates are in the Surface's local metre space.
#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(missing_docs)]
pub struct PositionedGlyph {
    pub glyph_id: u32,
    pub position: [f32; 2],
    pub color: Option<[f32; 4]>,
}

/// Structural item content. Animatable style and asset selections live in DynamicProperties.
#[derive(Clone, Debug, PartialEq)]
#[allow(missing_docs)]
pub enum SurfaceItemContent {
    /// Basic single-font layout with explicit line breaks and font-provided basic kerning.
    Label(String),
    /// Client-shaped glyph identities and positions; the runtime does not reshape them.
    GlyphRun(Vec<PositionedGlyph>),
    /// One immutable quadratic drawing asset.
    Drawing,
    /// One immutable RGBA bitmap with a local-metre display size.
    Bitmap {
        size: [f32; 2],
    },
}

/// One durable collection member. Painter order is the collection order.
#[derive(Clone, Debug, PartialEq)]
#[allow(missing_docs)]
pub struct SurfaceItem {
    pub id: SurfaceItemId,
    pub content: SurfaceItemContent,
}

/// Initial or inspected authoritative item style.
#[derive(Clone, Debug, PartialEq)]
#[allow(missing_docs)]
pub struct SurfaceItemStyle {
    pub position: [f32; 2],
    pub scale: [f32; 2],
    pub color: [f32; 4],
    pub opacity: f32,
    pub font_size: f32,
    pub asset: Option<crate::services::asset_management::AssetSource>,
}

impl Default for SurfaceItemStyle {
    fn default() -> Self {
        Self {
            position: [0.0; 2],
            scale: [1.0; 2],
            color: [1.0; 4],
            opacity: 1.0,
            font_size: 0.1,
            asset: None,
        }
    }
}

/// Partial item edit. Content edits retain identity and every existing binding.
#[derive(Clone, Debug, Default, PartialEq)]
#[allow(missing_docs)]
pub struct SurfaceItemPatch {
    pub content: Option<SurfaceItemContent>,
    pub position: Option<[f32; 2]>,
    pub scale: Option<[f32; 2]>,
    pub color: Option<[f32; 4]>,
    pub opacity: Option<f32>,
    pub font_size: Option<f32>,
    pub asset: Option<Option<crate::services::asset_management::AssetSource>>,
}

/// Typed storage with a portable field encoding for generated transports and persistence.
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceItems {
    pub(super) values: Vec<SurfaceItem>,
    pub(super) next_id: u32,
}

impl Default for SurfaceItems {
    fn default() -> Self {
        Self {
            values: Vec::new(),
            next_id: 1,
        }
    }
}

impl SurfaceItems {
    pub(super) fn as_slice(&self) -> &[SurfaceItem] {
        &self.values
    }

    pub(super) fn as_mut_vec(&mut self) -> &mut Vec<SurfaceItem> {
        &mut self.values
    }

    pub(super) fn validate(&self) -> Result<(), FieldError> {
        if self.values.len() > MAX_ITEMS || self.next_id == 0 {
            return Err(FieldError::WrongType);
        }
        let mut ids = std::collections::BTreeSet::new();
        for item in &self.values {
            if item.id.0 == 0 || item.id.0 >= self.next_id || !ids.insert(item.id) {
                return Err(FieldError::WrongType);
            }
            validate_content(&item.content)?;
        }
        Ok(())
    }

    fn encode(&self) -> Vec<u8> {
        let mut output = vec![CODEC_VERSION];
        put_u32(&mut output, self.next_id);
        put_u32(&mut output, self.values.len() as u32);
        for item in &self.values {
            put_u32(&mut output, item.id.0);
            match &item.content {
                SurfaceItemContent::Label(text) => {
                    output.push(1);
                    put_u32(&mut output, text.len() as u32);
                    output.extend(text.as_bytes());
                }
                SurfaceItemContent::GlyphRun(glyphs) => {
                    output.push(2);
                    put_u32(&mut output, glyphs.len() as u32);
                    for glyph in glyphs {
                        put_u32(&mut output, glyph.glyph_id);
                        put_f32s(&mut output, &glyph.position);
                        output.push(u8::from(glyph.color.is_some()));
                        if let Some(color) = glyph.color {
                            put_f32s(&mut output, &color);
                        }
                    }
                }
                SurfaceItemContent::Drawing => output.push(3),
                SurfaceItemContent::Bitmap {
                    size,
                } => {
                    output.push(4);
                    put_f32s(&mut output, size);
                }
            }
        }
        output
    }

    fn decode(mut input: &[u8]) -> Result<Self, FieldError> {
        if take(&mut input, 1)?[0] != CODEC_VERSION {
            return Err(FieldError::WrongType);
        }
        let next_id = read_u32(&mut input)?;
        let count = read_u32(&mut input)? as usize;
        if count > MAX_ITEMS {
            return Err(FieldError::WrongType);
        }
        let mut items = Vec::with_capacity(count);
        for _ in 0..count {
            let id = SurfaceItemId(read_u32(&mut input)?);
            let content = match take(&mut input, 1)?[0] {
                1 => {
                    let length = read_u32(&mut input)? as usize;
                    if length > MAX_TEXT_BYTES {
                        return Err(FieldError::WrongType);
                    }
                    SurfaceItemContent::Label(
                        std::str::from_utf8(take(&mut input, length)?)
                            .map_err(|_| FieldError::WrongType)?
                            .into(),
                    )
                }
                2 => {
                    let length = read_u32(&mut input)? as usize;
                    if length > MAX_GLYPHS {
                        return Err(FieldError::WrongType);
                    }
                    let mut glyphs = Vec::with_capacity(length);
                    for _ in 0..length {
                        let glyph_id = read_u32(&mut input)?;
                        let position = read_f32s(&mut input)?;
                        let color = match take(&mut input, 1)?[0] {
                            0 => None,
                            1 => Some(read_f32s(&mut input)?),
                            _ => return Err(FieldError::WrongType),
                        };
                        glyphs.push(PositionedGlyph {
                            glyph_id,
                            position,
                            color,
                        });
                    }
                    SurfaceItemContent::GlyphRun(glyphs)
                }
                3 => SurfaceItemContent::Drawing,
                4 => SurfaceItemContent::Bitmap {
                    size: read_f32s(&mut input)?,
                },
                _ => return Err(FieldError::WrongType),
            };
            items.push(SurfaceItem {
                id,
                content,
            });
        }
        if !input.is_empty() {
            return Err(FieldError::WrongType);
        }
        let items = Self {
            values: items,
            next_id,
        };
        items.validate()?;
        Ok(items)
    }
}

impl SchemaField for SurfaceItems {
    const KIND: FieldKind = FieldKind::Bytes;

    fn to_value(&self) -> FieldValue {
        FieldValue::Bytes(self.encode())
    }

    fn from_value(value: FieldValue) -> Result<Self, FieldError> {
        match value {
            FieldValue::Bytes(bytes) => Self::decode(&bytes),
            _ => Err(FieldError::WrongType),
        }
    }

    fn write_default(&self, sink: &mut impl ContractSink) {
        let bytes = self.encode();
        sink.write(&(bytes.len() as u32).to_le_bytes());
        sink.write(&bytes);
    }

    fn retained_bytes(&self) -> Option<usize> {
        Some(
            self.values.capacity() * std::mem::size_of::<SurfaceItem>()
                + self.values.iter().map(item_bytes).sum::<usize>(),
        )
    }
}

pub(super) fn validate_content(content: &SurfaceItemContent) -> Result<(), FieldError> {
    match content {
        SurfaceItemContent::Label(text) if text.len() <= MAX_TEXT_BYTES => Ok(()),
        SurfaceItemContent::GlyphRun(glyphs) if glyphs.len() <= MAX_GLYPHS => {
            if glyphs.iter().all(|glyph| {
                glyph.position.iter().all(|v| v.is_finite())
                    && glyph.color.is_none_or(|color| {
                        color
                            .iter()
                            .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
                    })
            }) {
                Ok(())
            } else {
                Err(FieldError::NonFinite)
            }
        }
        SurfaceItemContent::Drawing => Ok(()),
        SurfaceItemContent::Bitmap {
            size,
        } if size.iter().all(|v| v.is_finite() && *v > 0.0) => Ok(()),
        _ => Err(FieldError::WrongType),
    }
}

fn item_bytes(item: &SurfaceItem) -> usize {
    match &item.content {
        SurfaceItemContent::Label(text) => text.capacity(),
        SurfaceItemContent::GlyphRun(glyphs) => {
            glyphs.capacity() * std::mem::size_of::<PositionedGlyph>()
        }
        _ => 0,
    }
}

fn put_u32(output: &mut Vec<u8>, value: u32) {
    output.extend(value.to_le_bytes());
}

fn put_f32s<const N: usize>(output: &mut Vec<u8>, values: &[f32; N]) {
    for value in values {
        output.extend(value.to_le_bytes());
    }
}

fn take<'a>(input: &mut &'a [u8], length: usize) -> Result<&'a [u8], FieldError> {
    let value = input.get(..length).ok_or(FieldError::WrongType)?;
    *input = &input[length..];
    Ok(value)
}

fn read_u32(input: &mut &[u8]) -> Result<u32, FieldError> {
    Ok(u32::from_le_bytes(take(input, 4)?.try_into().unwrap()))
}

fn read_f32s<const N: usize>(input: &mut &[u8]) -> Result<[f32; N], FieldError> {
    let mut output = [0.0; N];
    for value in &mut output {
        *value = f32::from_le_bytes(take(input, 4)?.try_into().unwrap());
        if !value.is_finite() {
            return Err(FieldError::NonFinite);
        }
    }
    Ok(output)
}
