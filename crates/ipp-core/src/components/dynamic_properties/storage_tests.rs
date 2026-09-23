//! Allocation invariants of [`DynamicProperties`] numeric storage.

use super::*;

/// First-fit placement over the sorted occupied spans: the previous
/// allocator, kept as the reference layout.
fn reference_offset(properties: &DynamicProperties, size: usize) -> usize {
    if size == 0 {
        return 0;
    }

    let mut occupied: Vec<_> = properties
        .descriptors()
        .values()
        .filter(|d| d.kind.byte_len() > 0)
        .map(|d| (d.offset as usize, d.kind.byte_len()))
        .collect();
    occupied.sort_unstable();

    let mut offset = 0;
    for (start, length) in occupied {
        if start >= offset + size {
            break;
        }
        offset = start + length;
    }
    offset
}

fn assert_consistent(properties: &DynamicProperties) {
    let end = properties
        .descriptors()
        .values()
        .map(|d| d.offset as usize + d.kind.byte_len())
        .max()
        .unwrap_or(0);
    assert_eq!(
        properties.buffer().len(),
        end,
        "buffer ends at its last property"
    );

    let mut spans: Vec<_> = properties
        .descriptors()
        .values()
        .filter(|d| d.kind.byte_len() > 0)
        .map(|d| (d.offset as usize, d.kind.byte_len()))
        .collect();
    spans.sort_unstable();
    for pair in spans.windows(2) {
        assert!(pair[0].0 + pair[0].1 <= pair[1].0, "properties overlap");
    }
}

#[test]
fn inserts_and_removals_keep_first_fit_layout_and_values() {
    let values = [
        DynamicValue::F32(1.5),
        DynamicValue::Vec2([2.0, 3.0]),
        DynamicValue::Vec4([0.1, 0.2, 0.3, 0.4]),
        DynamicValue::Bool(true),
        DynamicValue::Mat3([0.5; 9]),
    ];
    let mut properties = DynamicProperties::default();
    let mut expected = BTreeMap::new();
    let mut seed = 0x2545_f491_u32;

    for step in 0..4_000 {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let name = format!("lane_{}", seed % 97);
        let value = values[(seed / 97) as usize % values.len()].clone();

        if seed.is_multiple_of(5) {
            properties.remove(&name);
            expected.remove(&name);
        } else {
            let retyped = properties
                .descriptors()
                .get(&name)
                .is_none_or(|descriptor| descriptor.kind != value.kind());
            let previous_key = properties.key(&name);
            let reference = if retyped {
                let mut model = properties.clone();
                model.remove(&name);
                Some(reference_offset(&model, value.kind().byte_len()))
            } else {
                None
            };

            let key = properties.set(&name, value.clone()).unwrap();

            if let Some(reference) = reference {
                assert_eq!(
                    properties.descriptors()[&name].offset as usize,
                    reference,
                    "step {step} placed {name} away from the first fit"
                );
            } else {
                assert_eq!(Some(key), previous_key, "value edits keep identities");
            }
            expected.insert(name, value);
        }

        assert_consistent(&properties);
    }

    for (name, value) in &expected {
        assert_eq!(properties.get(name).as_ref(), Some(value));
    }
    assert_eq!(properties.descriptors().len(), expected.len());
}
