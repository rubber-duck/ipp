use crate::binary_reader::Reader;
use crate::model::{Component, Export, Field, TargetFeature};
use crate::typescript_names::{identifier, js_string};
use crate::wire_contract;

pub(super) fn read_export(bytes: &[u8]) -> Result<Export, String> {
    if bytes.len() < 16 || bytes.len() > 1_048_576 {
        return Err("export size".into());
    }
    if &bytes[..4] != b"IPPB" || bytes[4..8] != 2u32.to_le_bytes() {
        return Err("export bootstrap".into());
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
    if r.u16()? != 4 {
        return Err("export format".into());
    }

    let arch = r.string()?;
    let os = r.string()?;
    let pointer = r.u8()?;
    if ![32, 64].contains(&pointer) {
        return Err("target pointer width".into());
    }
    let features = read_target_features(&mut r)?;

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
                    6 => {
                        let n = r.u32()? as usize;
                        format!("{:?} as const", r.take(n)?)
                    }
                    _ => return Err("unknown field kind".into()),
                }
            };
            if !(1..=7).contains(&kind) {
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

    let wire = wire_contract::read_wire_contract(&mut r, &components, &features)?;

    if !r.is_complete() {
        return Err("trailing export bytes".into());
    }

    Ok(Export {
        expected,
        arch,
        os,
        pointer,
        features,
        components,
        wire,
    })
}

pub(super) fn read_target_features(r: &mut Reader<'_>) -> Result<Vec<TargetFeature>, String> {
    let count = r.u8()? as usize;
    if count == 0 {
        return Err("incomplete target features".into());
    }
    let mut features = Vec::with_capacity(count);
    let mut ids = std::collections::BTreeSet::new();
    let mut names = std::collections::BTreeSet::new();
    for _ in 0..count {
        let id = r.u8()?;
        let enabled = match r.u8()? {
            0 => false,
            1 => true,
            _ => return Err("target feature flag".into()),
        };
        let name = r.string()?;
        if id == 0 || name.is_empty() || !ids.insert(id) || !names.insert(name.clone()) {
            return Err("target feature identity".into());
        }
        features.push(TargetFeature {
            id,
            name,
            enabled,
        });
    }
    Ok(features)
}
