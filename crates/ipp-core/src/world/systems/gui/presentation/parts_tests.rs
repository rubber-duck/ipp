use super::*;
use crate::DynamicValue;

const PART_PROPERTIES: [&str; 23] = [
    "color",
    "opacity",
    "scale",
    "align_x",
    "asset",
    "corner_radius",
    "border_width",
    "border_color",
    "fill_mode",
    "gradient_start",
    "gradient_end",
    "gradient_color0",
    "gradient_color1",
    "gradient_radius",
    "glow_color",
    "glow_intensity",
    "glow_radius",
    "glow_falloff",
    "motion",
    "duration",
    "easing",
    "track",
    "time",
];

#[test]
fn part_properties_keep_their_names_and_indices() {
    for (index, property) in GuiPartProperty::ALL.into_iter().enumerate() {
        assert_eq!(property.index(), index as u32);
        assert_eq!(GuiPartProperty::from_index(index as u32), Some(property));
        assert_eq!(property.name(), PART_PROPERTIES[index]);
        assert_eq!(property.appearance(), index < 18);
    }
    assert_eq!(GuiPartProperty::from_index(23), None);
}

#[test]
fn part_identities_enumerate_every_base_state_and_variant_once() {
    assert_eq!(GuiPartId::COUNT, 117);
    let mut seen = std::collections::BTreeSet::new();
    for index in 0..GuiPartId::COUNT {
        let id = GuiPartId::from_index(index).unwrap();
        assert_eq!(id.index(), Some(index));
        assert!(id.variant.is_none() || id.state.is_some());
        assert!(seen.insert(id));
    }
    assert_eq!(GuiPartId::from_index(GuiPartId::COUNT), None);
    assert_eq!(
        GuiPartId {
            part: GuiPrimitivePart::Icon,
            state: None,
            variant: Some(GuiPartVariant::Checked),
        }
        .index(),
        None
    );
    assert_eq!(
        GuiPartId::base(GuiPrimitivePart::Fill).index(),
        Some(GuiPartId::QUALIFIERS)
    );
    assert_eq!(
        GuiPartId::variant(
            GuiPrimitivePart::Background,
            GuiSkinState::Disabled,
            GuiPartVariant::Unchecked,
        )
        .index(),
        Some(12)
    );
}

#[test]
fn candidates_run_from_variant_through_state_to_base() {
    let icon = GuiPrimitivePart::Icon;
    let pressed = GuiSkinState::Pressed;

    assert_eq!(
        GuiPartId::candidates(icon, pressed, Some(GuiPartVariant::Checked)).collect::<Vec<_>>(),
        [
            GuiPartId::variant(icon, pressed, GuiPartVariant::Checked),
            GuiPartId::state(icon, pressed),
            GuiPartId::base(icon),
        ]
    );
    assert_eq!(
        GuiPartId::candidates(icon, pressed, None).collect::<Vec<_>>(),
        [GuiPartId::state(icon, pressed), GuiPartId::base(icon)]
    );
}

#[test]
fn part_values_are_checked_by_kind_and_range() {
    use GuiPartProperty as P;

    let ok = |property, value| validate_part_property(property, &value).is_ok();

    assert!(ok(P::Color, DynamicValue::Vec4([0.0, 0.5, 1.0, 1.0])));
    assert!(!ok(P::Color, DynamicValue::Vec4([0.0, 0.5, 1.5, 1.0])));
    assert!(!ok(P::Color, DynamicValue::F32(1.0)));
    assert!(!ok(P::Opacity, DynamicValue::F32(-0.1)));
    assert!(ok(P::AlignX, DynamicValue::F32(-4.0)));
    assert!(!ok(P::CornerRadius, DynamicValue::Vec2([0.1, -0.1])));
    assert!(ok(P::FillMode, DynamicValue::F32(2.0)));
    assert!(!ok(P::FillMode, DynamicValue::F32(3.0)));
    assert!(!ok(P::Easing, DynamicValue::F32(0.5)));
    assert!(ok(P::Track, DynamicValue::F32(4.0)));
    assert!(!ok(P::Track, DynamicValue::F32(1.5)));
    assert!(!ok(P::Duration, DynamicValue::F32(f32::NAN)));
    assert_eq!(skin_motion_base_track(u32::MAX as f32), None);
    assert_eq!(skin_motion_base_track(7.0), Some(7));
}
