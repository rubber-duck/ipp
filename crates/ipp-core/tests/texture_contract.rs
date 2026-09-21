//! Check the selected core capability contract independently of workspace unification.

#[test]
fn baseline_texture_registration_and_optional_feature_contract() {
    let mut bytes = vec![];
    ipp_core::components::registry::write_contract(&mut bytes);
    assert_eq!(u16::from_le_bytes(bytes[..2].try_into().unwrap()), 4);

    let mut cursor = 2;
    for _ in 0..2 {
        let length = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().unwrap()) as usize;
        cursor += 4 + length;
    }
    cursor += 1;
    let feature_count = bytes[cursor] as usize;
    cursor += 1;
    let mut features = std::collections::BTreeMap::new();
    for _ in 0..feature_count {
        let id = bytes[cursor];
        let enabled = bytes[cursor + 1] == 1;
        cursor += 2;
        let length = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().unwrap()) as usize;
        cursor += 4;
        let name = std::str::from_utf8(&bytes[cursor..cursor + length]).unwrap();
        cursor += length;
        assert!(features.insert(name, (id, enabled)).is_none());
    }
    assert_eq!(features.len(), 7);
    assert_eq!(features["surfaces"], (18, cfg!(feature = "surfaces")));
    assert_eq!(features["gui"], (19, cfg!(feature = "gui")));
    assert_eq!(features["particles"], (17, cfg!(feature = "particles")));
    assert_eq!(
        features["skeletal-animation"],
        (16, cfg!(feature = "skeletal-animation"))
    );
    assert!(!features.contains_key("textures"));
    assert!(!features.contains_key("skeleton"));
    assert!(!features.contains_key("skinning"));
    assert_eq!(
        features["builtin-assets"],
        (11, cfg!(feature = "builtin-assets"))
    );

    {
        use ipp_core::{ComponentValue, components::UnlitTexture, components::schema::FieldValue};
        use std::mem::offset_of;

        let value = ComponentValue::UnlitTexture(UnlitTexture::default());
        assert_eq!(value.type_id(), 6);
        assert_eq!(ipp_core::components::registry::create(6).unwrap(), value);
        assert_eq!(
            value.fields(),
            vec![
                (
                    offset_of!(UnlitTexture, source) as u32,
                    FieldValue::String(String::new())
                ),
                (offset_of!(UnlitTexture, variant) as u32, FieldValue::U32(0)),
            ]
        );
        for (offset, field) in value.fields() {
            ComponentValue::validate_field(6, offset, field.kind()).unwrap();
        }
    }
}
