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
    render(&mut output, &keys);
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
    render(&mut output, &[]);
    assert!(output.is_empty());
}
