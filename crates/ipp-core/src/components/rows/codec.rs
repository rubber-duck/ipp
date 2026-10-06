//! Per-row and per-value encoding used by the `Rows<R>` table encoding.

use super::layout::MAX_ROW_TEXT_BYTES;
use super::property::SchemaRow;
use crate::components::dynamic_properties::{DynamicPropertyKind, DynamicValue};
use crate::components::schema::FieldError;
use crate::services::asset_management::{AssetSource, AssetTypeId};

/// Append one row in the table's per-row encoding: a presence mask of
/// [`RowsLayout::mask_bytes`] bytes, then the present values in layout order
/// (see [`Rows::encode`]).
///
/// [`RowsLayout::mask_bytes`]: super::RowsLayout::mask_bytes
/// [`Rows::encode`]: super::Rows::encode
pub fn encode_row<R: SchemaRow>(row: &R, bytes: &mut Vec<u8>) {
    let layout = R::LAYOUT;
    let mask_start = bytes.len();
    bytes.resize(mask_start + layout.mask_bytes(), 0);

    for index in 0..layout.property_count() {
        let Some(value) = row.property(index).expect("layout property") else {
            continue;
        };

        bytes[mask_start + index as usize / 8] |= 1 << (index % 8);
        encode_row_value(&value, bytes);
    }
}

/// Decode and validate one row written by [`encode_row`], consuming it from
/// `bytes`. Required properties must be present; undeclared mask bits,
/// non-finite values, invalid asset sources and invalid or over-long text are
/// rejected.
pub fn decode_row<R: SchemaRow>(bytes: &mut &[u8]) -> Result<R, FieldError> {
    let layout = R::LAYOUT;
    let mask = take(bytes, layout.mask_bytes())?;
    if layout.properties.len() % 8 != 0
        && mask[mask.len() - 1] >> (layout.properties.len() % 8) != 0
    {
        return Err(FieldError::WrongType);
    }

    let mut row = R::default();
    for (index, property) in layout.properties.iter().enumerate() {
        if mask[index / 8] & (1 << (index % 8)) == 0 {
            if !property.optional {
                return Err(FieldError::WrongType);
            }

            row.clear_property(index as u32)?;
        } else {
            row.set_property(index as u32, decode_row_value(property.kind, bytes)?)?;
        }
    }

    Ok(row)
}

fn take<'a>(bytes: &mut &'a [u8], length: usize) -> Result<&'a [u8], FieldError> {
    let (head, tail) = bytes
        .split_at_checked(length)
        .ok_or(FieldError::WrongType)?;
    *bytes = tail;
    Ok(head)
}

pub(super) fn take_u32(bytes: &mut &[u8]) -> Result<u32, FieldError> {
    Ok(u32::from_le_bytes(
        take(bytes, 4)?.try_into().expect("four bytes"),
    ))
}

/// Append one untagged row property value in the table encoding.
pub fn encode_row_value(value: &DynamicValue, bytes: &mut Vec<u8>) {
    match value {
        DynamicValue::I32(value) => bytes.extend(value.to_le_bytes()),
        DynamicValue::U32(value) => bytes.extend(value.to_le_bytes()),
        DynamicValue::Bool(value) => bytes.extend(u32::from(*value).to_le_bytes()),
        DynamicValue::Asset(asset) => {
            bytes.extend(asset.kind.0.to_le_bytes());
            bytes.extend(asset.variant.to_le_bytes());
            bytes.extend((asset.uri.len() as u32).to_le_bytes());
            bytes.extend(asset.uri.as_bytes());
        }
        DynamicValue::Text(text) => {
            bytes.extend((text.len() as u32).to_le_bytes());
            bytes.extend(text.as_bytes());
        }
        value => {
            for lane in value.floats().expect("numeric row value") {
                bytes.extend(lane.to_le_bytes());
            }
        }
    }
}

/// Decode and validate one untagged row property value of `kind`. Text is
/// checked for UTF-8 here and against its byte bound by the row write.
pub fn decode_row_value(
    kind: DynamicPropertyKind,
    bytes: &mut &[u8],
) -> Result<DynamicValue, FieldError> {
    fn floats<const N: usize>(bytes: &mut &[u8]) -> Result<[f32; N], FieldError> {
        let data = take(bytes, N * 4)?;
        Ok(std::array::from_fn(|lane| {
            f32::from_le_bytes(data[lane * 4..lane * 4 + 4].try_into().expect("four bytes"))
        }))
    }

    let value = match kind {
        DynamicPropertyKind::F32 => DynamicValue::F32(floats::<1>(bytes)?[0]),
        DynamicPropertyKind::I32 => DynamicValue::I32(take_u32(bytes)? as i32),
        DynamicPropertyKind::U32 => DynamicValue::U32(take_u32(bytes)?),
        DynamicPropertyKind::Bool => DynamicValue::Bool(match take_u32(bytes)? {
            0 => false,
            1 => true,
            _ => return Err(FieldError::WrongType),
        }),
        DynamicPropertyKind::Vec2 => DynamicValue::Vec2(floats(bytes)?),
        DynamicPropertyKind::Vec3 => DynamicValue::Vec3(floats(bytes)?),
        DynamicPropertyKind::Vec4 => DynamicValue::Vec4(floats(bytes)?),
        DynamicPropertyKind::Asset => {
            let kind = u16::from_le_bytes(take(bytes, 2)?.try_into().expect("two bytes"));
            let variant = take_u32(bytes)?;
            let length = take_u32(bytes)? as usize;
            let uri =
                std::str::from_utf8(take(bytes, length)?).map_err(|_| FieldError::WrongType)?;
            DynamicValue::Asset(AssetSource {
                kind: AssetTypeId(kind),
                uri: uri.into(),
                variant,
            })
        }
        DynamicPropertyKind::Text => {
            let length = take_u32(bytes)? as usize;
            if length > MAX_ROW_TEXT_BYTES as usize {
                return Err(FieldError::TextTooLong);
            }

            let text =
                std::str::from_utf8(take(bytes, length)?).map_err(|_| FieldError::WrongType)?;
            DynamicValue::Text(text.into())
        }
        DynamicPropertyKind::Mat2 | DynamicPropertyKind::Mat3 | DynamicPropertyKind::Mat4 => {
            return Err(FieldError::WrongType);
        }
    };

    value.validate_representation()?;
    Ok(value)
}
