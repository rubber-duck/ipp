use std::collections::BTreeSet;
use std::fmt::Write as _;

use crate::binary_reader::Reader;
use crate::model::{GuiPaintKey, GuiSkinLook, GuiSkinLookRow, GuiSkinToken};
use crate::typescript::GUI_PAINT_TEMPLATE;
use crate::typescript_names::{js_string, member_identifier};

pub(super) fn read(reader: &mut Reader<'_>) -> Result<Vec<GuiPaintKey>, String> {
    let count = reader.u16()?;

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

fn invalid_look() -> String {
    "invalid GUI skin look".to_owned()
}

/// Read the built-in skin looks that follow the paint keys: named tables of
/// appearance rows and motion rows whose parts are exported keys and whose
/// properties are numeric lanes.
pub(super) fn read_looks(
    reader: &mut Reader<'_>,
    keys: &[GuiPaintKey],
) -> Result<Vec<GuiSkinLook>, String> {
    let indices: BTreeSet<u32> = keys.iter().map(|key| key.index).collect();
    let mut names = BTreeSet::new();
    let mut looks = Vec::new();
    for _ in 0..reader.u16()? {
        let name = reader.string()?;
        let plain = name.starts_with(|c: char| c.is_ascii_lowercase())
            && name.chars().all(|c| c.is_ascii_alphanumeric());
        if !plain || !names.insert(name.clone()) {
            return Err(invalid_look());
        }
        let em = f32::from_le_bytes(reader.take(4)?.try_into().unwrap());
        if !em.is_finite() || em < 0.0 {
            return Err(invalid_look());
        }

        let rows = read_rows(reader, &indices)?;
        let motion = read_rows(reader, &indices)?;
        looks.push(GuiSkinLook {
            name,
            em,
            rows,
            motion,
        });
    }
    Ok(looks)
}

/// One table of look rows, each naming an exported key once.
fn read_rows(
    reader: &mut Reader<'_>,
    indices: &BTreeSet<u32>,
) -> Result<Vec<GuiSkinLookRow>, String> {
    let mut parts = BTreeSet::new();
    let mut rows = Vec::new();
    for _ in 0..reader.u16()? {
        let part = reader.u32()?;
        if !indices.contains(&part) || !parts.insert(part) {
            return Err(invalid_look());
        }
        let mut seen = BTreeSet::new();
        let mut properties = Vec::new();
        for _ in 0..reader.u8()? {
            let property = reader.string()?;
            member_identifier(&property)?;
            let lanes = reader.u8()?;
            if property == "part" || !seen.insert(property.clone()) || ![1, 2, 4].contains(&lanes) {
                return Err(invalid_look());
            }
            let mut values = Vec::with_capacity(usize::from(lanes));
            for _ in 0..lanes {
                let value = f32::from_le_bytes(reader.take(4)?.try_into().unwrap());
                if !value.is_finite() {
                    return Err(invalid_look());
                }
                values.push(value);
            }
            properties.push((property, values));
        }
        rows.push(GuiSkinLookRow {
            part,
            properties,
        });
    }
    Ok(rows)
}

/// Read the design-language tokens that follow the looks: unique names, each
/// a number (one finite lane) or a colour (four).
pub(super) fn read_tokens(reader: &mut Reader<'_>) -> Result<Vec<GuiSkinToken>, String> {
    let invalid = || "invalid GUI skin token".to_owned();
    let mut names = BTreeSet::new();
    let mut tokens = Vec::new();
    for _ in 0..reader.u16()? {
        let name = reader.string()?;
        member_identifier(&name)?;
        let lanes = reader.u8()?;
        if !names.insert(name.clone()) || ![1, 4].contains(&lanes) {
            return Err(invalid());
        }
        let mut values = Vec::with_capacity(usize::from(lanes));
        for _ in 0..lanes {
            let value = f32::from_le_bytes(reader.take(4)?.try_into().unwrap());
            if !value.is_finite() {
                return Err(invalid());
            }
            values.push(value);
        }
        tokens.push(GuiSkinToken {
            name,
            lanes: values,
        });
    }
    Ok(tokens)
}

pub(super) fn render(
    out: &mut String,
    keys: &[GuiPaintKey],
    looks: &[GuiSkinLook],
    tokens: &[GuiSkinToken],
) {
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
    render_looks(out, looks);
    render_tokens(out, tokens);
    out.push_str(GUI_PAINT_TEMPLATE);
}

/// `GUI_SKIN_LOOKS`: each look's `em`, its appearance rows as `GuiTheme.parts`
/// row literals and its motion rows as `GuiThemeMotion.parts` row literals.
fn render_looks(out: &mut String, looks: &[GuiSkinLook]) {
    out.push_str(
        "/** Built-in skin looks: each control kind's default look and named variants, as a `GuiTheme` `em` and `parts` rows and `GuiThemeMotion` `motion` rows. */\n",
    );
    out.push_str("export const GUI_SKIN_LOOKS = freezeContract({\n");
    for look in looks {
        writeln!(
            out,
            "  {}: {{ em: {}, parts: [",
            js_string(&look.name),
            look.em
        )
        .unwrap();
        render_rows(out, &look.rows);
        out.push_str("  ], motion: [\n");
        render_rows(out, &look.motion);
        out.push_str("  ] },\n");
    }
    out.push_str("} as const);\n");
}

fn render_rows(out: &mut String, rows: &[GuiSkinLookRow]) {
    for row in rows {
        write!(out, "    {{ part: {}", row.part).unwrap();
        for (name, values) in &row.properties {
            let values: Vec<_> = values.iter().map(|value| format!("{value}")).collect();
            if values.len() == 1 {
                write!(out, ", {name}: {}", values[0]).unwrap();
            } else {
                write!(out, ", {name}: [{}]", values.join(", ")).unwrap();
            }
        }
        out.push_str(" },\n");
    }
}

/// `GUI_SKIN_TOKENS`: each token as a number or a colour literal.
fn render_tokens(out: &mut String, tokens: &[GuiSkinToken]) {
    out.push_str(
        "/** The design language's tokens: role colours as linear RGBA, and lengths in logical units at `em`, the font size the built-in looks are drawn at. */\n",
    );
    out.push_str("export const GUI_SKIN_TOKENS = freezeContract({\n");
    for token in tokens {
        let values: Vec<_> = token.lanes.iter().map(|value| format!("{value}")).collect();
        if values.len() == 1 {
            writeln!(out, "  {}: {},", js_string(&token.name), values[0]).unwrap();
        } else {
            writeln!(
                out,
                "  {}: [{}],",
                js_string(&token.name),
                values.join(", ")
            )
            .unwrap();
        }
    }
    out.push_str("} as const);\n");
}

#[cfg(test)]
#[path = "paint_keys_tests.rs"]
mod tests;
