use super::*;

fn export(keys: &[(u32, &str, &str, &str)]) -> Vec<u8> {
    let mut bytes = (keys.len() as u16).to_le_bytes().to_vec();
    for (index, part, state, variant) in keys {
        bytes.extend(index.to_le_bytes());
        for name in [part, state, variant] {
            bytes.extend((name.len() as u32).to_le_bytes());
            bytes.extend(name.as_bytes());
        }
    }
    bytes
}

#[test]
fn paint_keys_follow_compiled_indices_not_generator_arithmetic() {
    let bytes = export(&[
        (45, "background", "", ""),
        (987, "fill", "pressed", "checked"),
    ]);
    let keys = read(&mut Reader {
        bytes: &bytes,
        at: 0,
    })
    .unwrap();
    assert_eq!(keys[0].index, 45);
    assert_eq!(keys[1].index, 987);
    let mut output = String::new();
    render(&mut output, &keys, &[], &[]);
    assert!(output.contains("index: 45, part: \"background\", state: null, variant: null"));
    assert!(
        output.contains("index: 987, part: \"fill\", state: \"pressed\", variant: \"checked\"")
    );
}

#[test]
fn paint_keys_are_unambiguous() {
    for invalid in [
        vec![(1, "fill", "", ""), (1, "background", "", "")],
        vec![(1, "fill", "", ""), (2, "fill", "", "")],
        vec![(1, "fill", "", "checked")],
        vec![(1, "", "", "")],
        vec![(1, "fill", "bad-name", "")],
    ] {
        let bytes = export(&invalid);
        assert!(
            read(&mut Reader {
                bytes: &bytes,
                at: 0
            })
            .is_err()
        );
    }
    let mut output = String::new();
    render(&mut output, &[], &[], &[]);
    assert!(output.is_empty());
}

/// One look row: a part index and its `(name, lanes)` properties.
type LookRow<'a> = (u32, Vec<(&'a str, Vec<f32>)>);

/// Built-in look bytes: `(name, rows)` of each look, then `motion` rows of
/// every look.
fn looks_export(looks: &[(&str, Vec<LookRow<'_>>)]) -> Vec<u8> {
    looks_with_motion(looks, &[])
}

fn looks_with_motion(looks: &[(&str, Vec<LookRow<'_>>)], motion: &[LookRow<'_>]) -> Vec<u8> {
    let string = |bytes: &mut Vec<u8>, value: &str| {
        bytes.extend((value.len() as u32).to_le_bytes());
        bytes.extend(value.as_bytes());
    };
    let table = |bytes: &mut Vec<u8>, rows: &[LookRow<'_>]| {
        bytes.extend((rows.len() as u16).to_le_bytes());
        for (part, properties) in rows {
            bytes.extend(part.to_le_bytes());
            bytes.push(properties.len() as u8);
            for (property, lanes) in properties {
                string(bytes, property);
                bytes.push(lanes.len() as u8);
                for lane in lanes {
                    bytes.extend(lane.to_le_bytes());
                }
            }
        }
    };
    let mut bytes = (looks.len() as u16).to_le_bytes().to_vec();
    for (name, rows) in looks {
        string(&mut bytes, name);
        bytes.extend(14.5_f32.to_le_bytes());
        table(&mut bytes, rows);
        table(&mut bytes, motion);
    }
    bytes
}

fn keys() -> Vec<GuiPaintKey> {
    let bytes = export(&[(0, "background", "", ""), (7, "background", "hovered", "")]);
    read(&mut Reader {
        bytes: &bytes,
        at: 0,
    })
    .unwrap()
}

#[test]
fn skin_looks_render_as_frozen_theme_row_literals() {
    let bytes = looks_with_motion(
        &[
            (
                "switch",
                vec![
                    (
                        0,
                        vec![
                            ("color", vec![0.0, 0.006_512_091, 0.5, 1.0]),
                            ("border_width", vec![1.25]),
                        ],
                    ),
                    (7, vec![("scale", vec![1.5, 1.5])]),
                ],
            ),
            ("textInput", vec![]),
        ],
        &[(7, vec![("duration", vec![0.16]), ("easing", vec![2.0])])],
    );
    let looks = read_looks(
        &mut Reader {
            bytes: &bytes,
            at: 0,
        },
        &keys(),
    )
    .unwrap();
    let mut output = String::new();
    render(&mut output, &keys(), &looks, &[]);
    assert!(output.contains("export const GUI_SKIN_LOOKS = freezeContract({"));
    // Reserved words stay usable as quoted names, and lanes keep the
    // shortest text that round-trips their f32 value.
    assert!(output.contains("  \"switch\": { em: 14.5, parts: [\n"));
    assert!(
        output.contains("    { part: 0, color: [0, 0.006512091, 0.5, 1], border_width: 1.25 },")
    );
    assert!(output.contains("    { part: 7, scale: [1.5, 1.5] },\n  ], motion: [\n"));
    assert!(output.contains("    { part: 7, duration: 0.16, easing: 2 },\n  ] },"));
    assert!(output.contains("  \"textInput\": { em: 14.5, parts: [\n  ], motion: [\n"));
    assert!(output.contains("export function guiSkinLookTable("));
    assert!(output.contains("export function guiSkinLookMotionTable("));
}

#[test]
fn skin_looks_name_known_keys_once_with_numeric_lanes() {
    let keys = keys();
    for invalid in [
        looks_export(&[("Upper", vec![])]),
        looks_export(&[("a-b", vec![])]),
        looks_export(&[("twice", vec![]), ("twice", vec![])]),
        looks_export(&[("unknown", vec![(3, vec![])])]),
        looks_export(&[("repeated", vec![(0, vec![]), (0, vec![])])]),
        looks_export(&[("key", vec![(0, vec![("part", vec![1.0])])])]),
        looks_export(&[("lanes", vec![(0, vec![("color", vec![1.0, 1.0, 1.0])])])]),
        looks_export(&[("finite", vec![(0, vec![("opacity", vec![f32::NAN])])])]),
        looks_export(&[(
            "duplicate",
            vec![(0, vec![("opacity", vec![1.0]), ("opacity", vec![0.5])])],
        )]),
    ] {
        assert!(
            read_looks(
                &mut Reader {
                    bytes: &invalid,
                    at: 0,
                },
                &keys,
            )
            .is_err()
        );
    }
}

/// Token bytes: `(name, lanes)` of each token.
fn tokens_export(tokens: &[(&str, Vec<f32>)]) -> Vec<u8> {
    let mut bytes = (tokens.len() as u16).to_le_bytes().to_vec();
    for (name, lanes) in tokens {
        bytes.extend((name.len() as u32).to_le_bytes());
        bytes.extend(name.as_bytes());
        bytes.push(lanes.len() as u8);
        for lane in lanes {
            bytes.extend(lane.to_le_bytes());
        }
    }
    bytes
}

#[test]
fn skin_tokens_render_as_frozen_numbers_and_colours() {
    let bytes = tokens_export(&[
        ("em", vec![16.0]),
        ("accent", vec![0.0, 0.904_661_2, 0.964_686_3, 1.0]),
        ("lineWidth", vec![1.25]),
    ]);
    let tokens = read_tokens(&mut Reader {
        bytes: &bytes,
        at: 0,
    })
    .unwrap();
    let mut output = String::new();
    render(&mut output, &keys(), &[], &tokens);
    assert!(output.contains("export const GUI_SKIN_TOKENS = freezeContract({\n"));
    assert!(output.contains("  \"em\": 16,\n"));
    assert!(output.contains("  \"accent\": [0, 0.9046612, 0.9646863, 1],\n"));
    assert!(output.contains("  \"lineWidth\": 1.25,\n"));
}

#[test]
fn skin_tokens_are_unique_numbers_or_colours() {
    for invalid in [
        tokens_export(&[("twice", vec![1.0]), ("twice", vec![2.0])]),
        tokens_export(&[("pair", vec![1.0, 2.0])]),
        tokens_export(&[("rgb", vec![1.0, 1.0, 1.0])]),
        tokens_export(&[("none", vec![])]),
        tokens_export(&[("finite", vec![f32::INFINITY])]),
        tokens_export(&[("a-b", vec![1.0])]),
    ] {
        assert!(
            read_tokens(&mut Reader {
                bytes: &invalid,
                at: 0,
            })
            .is_err()
        );
    }
}
