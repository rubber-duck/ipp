use std::collections::BTreeSet;
use std::fmt::Write as _;

use crate::binary_reader::Reader;
use crate::model::GuiPaintKey;
use crate::typescript_names::{js_string, member_identifier};

pub(super) fn read(reader: &mut Reader<'_>, gui: bool) -> Result<Vec<GuiPaintKey>, String> {
    let count = reader.u16()?;
    if (count != 0) != gui {
        return Err("GUI paint key capability mismatch".into());
    }

    let mut keys = Vec::with_capacity(usize::from(count));
    let mut indices = BTreeSet::new();
    let mut identities = BTreeSet::new();
    for _ in 0..count {
        let index = reader.u32()?;
        let part = reader.string()?;
        let state = reader.string()?;
        let variant = reader.string()?;
        member_identifier(&part)?;
        for qualifier in [&state, &variant]
            .into_iter()
            .filter(|name| !name.is_empty())
        {
            member_identifier(qualifier)?;
        }
        if !indices.insert(index)
            || !identities.insert((part.clone(), state.clone(), variant.clone()))
            || (state.is_empty() && !variant.is_empty())
        {
            return Err("invalid GUI paint key".into());
        }

        keys.push(GuiPaintKey {
            index,
            part,
            state,
            variant,
        });
    }

    Ok(keys)
}

pub(super) fn render(out: &mut String, keys: &[GuiPaintKey]) {
    if keys.is_empty() {
        return;
    }

    out.push_str("export const GUI_PAINT_PART_KEYS = freezeContract([\n");
    for key in keys {
        let qualifier = |name: &str| {
            if name.is_empty() {
                "null".into()
            } else {
                js_string(name)
            }
        };
        writeln!(
            out,
            "  {{ index: {}, part: {}, state: {}, variant: {} }},",
            key.index,
            js_string(&key.part),
            qualifier(&key.state),
            qualifier(&key.variant)
        )
        .unwrap();
    }
    out.push_str("] as const);\n");
    out.push_str(include_str!("gui-paint.template.ts"));
}

#[cfg(test)]
#[path = "paint_keys_tests.rs"]
mod tests;
