use crate::binary_reader::Reader;
use crate::model::{Component, Export, Field, RowLimits, RowProperty, RowsLayout};
use crate::typescript_names::{identifier, js_string, member_identifier};
use crate::wire_contract;

pub(super) fn read_export(bytes: &[u8]) -> Result<Export, String> {
    if bytes.len() < 16 || bytes.len() > 1_048_576 {
        return Err("export size".into());
    }
    if &bytes[..4] != b"IPPB" {
        return Err("export marker".into());
    }
    if bytes[4..8] != crate::WIRE_REVISION.to_le_bytes() {
        return Err("export wire revision".into());
    }

    let expected = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
    let actual = bytes[16..].iter().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    });
    if expected != actual {
        return Err("export hash mismatch".into());
    }

    let mut r = Reader {
        bytes: &bytes[16..],
        at: 0,
    };
    if r.u16()? != 7 {
        return Err("export format".into());
    }

    let arch = r.string()?;
    let os = r.string()?;
    let pointer = r.u8()?;
    if ![32, 64].contains(&pointer) {
        return Err("target pointer width".into());
    }
    let row_limits = read_row_limits(&mut r)?;

    let n = r.u16()?;
    let mut components = Vec::new();
    let mut ids = std::collections::BTreeSet::new();
    let mut names = std::collections::BTreeSet::new();
    for _ in 0..n {
        let id = r.u16()?;
        let name = r.string()?;
        identifier(&name)?;
        if id == 0 || !ids.insert(id) || !names.insert(name.clone()) {
            return Err("duplicate component".into());
        }

        let size = r.u32()?;
        let align = r.u32()?;
        if !align.is_power_of_two() || size % align != 0 {
            return Err("component layout".into());
        }

        let n = r.u16()?;
        let creatable = match r.u8()? {
            0 => false,
            1 => true,
            _ => return Err("creation capability".into()),
        };
        let mut fields = Vec::new();
        let mut offsets = std::collections::BTreeSet::new();
        let mut names = std::collections::BTreeSet::new();
        let mut row_fields = 0u32;
        for _ in 0..n {
            let name = r.string()?;
            identifier(&name)?;
            let offset = r.u32()?;
            let field_size = r.u32()?;
            let field_align = r.u32()?;
            let kind = r.u8()?;
            if offset >= size || !offsets.insert(offset) || !names.insert(name.clone()) {
                return Err("field layout".into());
            }

            let rows = if kind == ROWS_KIND {
                row_fields += 1;
                Some(read_rows_layout(&mut r, row_fields, &row_limits)?)
            } else {
                None
            };

            let default = if !creatable {
                "undefined".into()
            } else {
                match kind {
                    1 => {
                        let v = f32::from_bits(r.u32()?);
                        if !v.is_finite() {
                            return Err("nonfinite default".into());
                        }
                        if v.to_bits() == 0x80000000 {
                            "-0".into()
                        } else {
                            v.to_string()
                        }
                    }
                    2 | 4 => format!("{}n", r.u64()?),
                    3 => r.u32()?.to_string(),
                    5 => js_string(&r.string()?),
                    7 => match r.u8()? {
                        0 => "false".into(),
                        1 => "true".into(),
                        _ => return Err("boolean default".into()),
                    },
                    6 | ROWS_KIND => {
                        let n = r.u32()? as usize;
                        format!("{:?} as const", r.take(n)?)
                    }
                    12 | 13 => match r.u8()? {
                        0 => "null".into(),
                        _ => return Err("runtime reference cannot be a component default".into()),
                    },
                    _ => return Err("unknown field kind".into()),
                }
            };
            if !(1..=ROWS_KIND).contains(&kind) && kind != 12 && kind != 13 {
                return Err("unknown field kind".into());
            }

            if !field_align.is_power_of_two()
                || offset % field_align != 0
                || field_align > align
                || field_size == 0
                || offset.checked_add(field_size).is_none_or(|end| end > size)
            {
                return Err("field exceeds target layout".into());
            }

            fields.push(Field {
                name,
                offset,
                field_size,
                field_align,
                kind,
                default,
                rows,
            });
        }

        let dynamic_properties = match r.u8()? {
            0 => false,
            1 => true,
            _ => return Err("dynamic property capability".into()),
        };
        components.push(Component {
            dynamic_properties,
            id,
            name,
            size,
            align,
            creatable,
            fields,
        });
    }

    let paint_keys = crate::paint_keys::read(&mut r)?;
    let wire = wire_contract::read_wire_contract(&mut r, &components)?;

    if !r.is_complete() {
        return Err("trailing export bytes".into());
    }

    Ok(Export {
        expected,
        arch,
        os,
        pointer,
        components,
        paint_keys,
        row_limits,
        wire,
    })
}

/// Field kind of a schema rows table.
pub(super) const ROWS_KIND: u8 = 8;

/// `DynamicPropertyKind::Text`, the bounded row-only text kind.
pub(super) const ROW_TEXT_KIND: u8 = 13;

/// Rows bounds the core registry exports ahead of its components.
pub(super) fn read_row_limits(r: &mut Reader<'_>) -> Result<RowLimits, String> {
    let limits = RowLimits {
        region_span: r.u32()?,
        fields: r.u8()?,
        properties: r.u16()?,
        text_bytes: r.u32()?,
    };
    // Every region, including the last at `fields` spans, must be addressable in u32.
    if limits.region_span == 0
        || limits.fields == 0
        || limits.properties == 0
        || limits.text_bytes == 0
        || limits
            .region_span
            .checked_mul(u32::from(limits.fields) + 1)
            .is_none()
    {
        return Err("rows limits".into());
    }

    Ok(limits)
}

/// Read the region base and ordered row layout that follow a rows field's kind.
/// `ordinal` is the 1-based position among the component's rows fields.
pub(super) fn read_rows_layout(
    r: &mut Reader<'_>,
    ordinal: u32,
    limits: &RowLimits,
) -> Result<RowsLayout, String> {
    let region_base = r.u32()?;
    if ordinal > u32::from(limits.fields) || region_base != limits.region_span * ordinal {
        return Err("rows region".into());
    }

    let count = r.u16()?;
    if count == 0 || count > limits.properties {
        return Err("rows layout size".into());
    }

    let mut names = std::collections::BTreeSet::new();
    let mut properties = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let name = r.string()?;
        member_identifier(&name)?;
        let kind = r.u8()?;
        let optional = match r.u8()? {
            0 => false,
            1 => true,
            _ => return Err("row property optional flag".into()),
        };
        let rotation = match r.u8()? {
            0 => false,
            1 => true,
            _ => return Err("row property hint".into()),
        };
        // DynamicPropertyKind F32..Vec4, Asset and Text; matrices are not row properties.
        if !(matches!(kind, 1..=7 | 12 | ROW_TEXT_KIND))
            || (rotation && kind != 7)
            || !names.insert(name.clone())
        {
            return Err("row property layout".into());
        }

        // Text properties follow their hint with a nonzero UTF-8 byte bound.
        let max_bytes = if kind == ROW_TEXT_KIND {
            let bound = r.u32()?;
            if bound == 0 || bound > limits.text_bytes {
                return Err("row text bound".into());
            }

            Some(bound)
        } else {
            None
        };

        properties.push(RowProperty {
            name,
            kind,
            optional,
            rotation,
            max_bytes,
        });
    }

    Ok(RowsLayout {
        region_base,
        properties,
    })
}
