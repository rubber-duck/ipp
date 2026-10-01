//! Check the core contract version and baseline texture registration.

#[test]
fn baseline_texture_registration_and_contract_version() {
    let mut bytes = vec![];
    ipp_core::components::registry::write_contract(&mut bytes);
    assert_eq!(u16::from_le_bytes(bytes[..2].try_into().unwrap()), 7);

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
                    FieldValue::String(std::sync::Arc::<str>::default())
                ),
                (offset_of!(UnlitTexture, variant) as u32, FieldValue::U32(0)),
            ]
        );
        for (offset, field) in value.fields() {
            ComponentValue::validate_field(6, offset, field.kind()).unwrap();
        }
    }
}
