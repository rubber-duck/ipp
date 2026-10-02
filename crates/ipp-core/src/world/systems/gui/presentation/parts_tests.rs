use super::*;
use crate::DynamicValue;

const PART_PROPERTIES: [&str; 32] = [
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
    "glow_inner_radius",
    "corner_cut",
    "corner_accent",
    "corner_accent_width",
    "shape",
    "stroke_a",
    "stroke_b",
    "arc_start",
    "arc_sweep",
    "arc_dashes",
    "fill_hue",
    "checker_size",
    "checker_color0",
    "checker_color1",
];

#[test]
fn part_properties_keep_their_names_and_indices() {
    for (index, property) in GuiPartProperty::ALL.into_iter().enumerate() {
        assert_eq!(property.index(), index as u32);
        assert_eq!(GuiPartProperty::from_index(index as u32), Some(property));
        assert_eq!(property.name(), PART_PROPERTIES[index]);
    }
    assert_eq!(GuiPartProperty::from_index(32), None);
}

#[test]
fn part_identities_enumerate_every_base_state_and_variant_once() {
    assert_eq!(GuiPartId::COUNT, 234);
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
    for mode in [0.0, 1.0, 2.0, 3.0, 4.0] {
        assert!(ok(P::FillMode, DynamicValue::F32(mode)));
    }
    assert!(!ok(P::FillMode, DynamicValue::F32(5.0)));
    assert!(!ok(P::FillMode, DynamicValue::F32(3.5)));
    // A hue takes any finite value, so a clip may turn it past a whole turn.
    for hue in [-2.25, 0.0, 0.5, 1.0, 17.0] {
        assert!(ok(P::FillHue, DynamicValue::F32(hue)));
    }
    assert!(!ok(P::FillHue, DynamicValue::F32(f32::NAN)));
    assert!(!ok(P::FillHue, DynamicValue::Vec2([0.0, 1.0])));
    assert!(ok(P::CheckerSize, DynamicValue::F32(0.0)));
    assert!(ok(P::CheckerSize, DynamicValue::F32(6.0)));
    assert!(!ok(P::CheckerSize, DynamicValue::F32(-1.0)));
    assert!(!ok(P::CheckerSize, DynamicValue::F32(f32::INFINITY)));
    assert!(ok(
        P::CheckerColor0,
        DynamicValue::Vec4([0.6, 0.6, 0.6, 1.0])
    ));
    assert!(ok(P::CheckerColor1, DynamicValue::Vec4([0.0; 4])));
    assert!(!ok(
        P::CheckerColor1,
        DynamicValue::Vec4([0.0, 0.0, 1.5, 1.0])
    ));
    assert!(!ok(P::CheckerColor0, DynamicValue::F32(1.0)));
    assert!(ok(P::GlowInnerRadius, DynamicValue::F32(0.0)));
    assert!(!ok(P::GlowInnerRadius, DynamicValue::F32(-0.5)));
    assert!(ok(P::CornerCut, DynamicValue::Vec4([0.0, 4.0, 1e6, 0.0])));
    assert!(!ok(P::CornerCut, DynamicValue::Vec4([0.0, -4.0, 0.0, 0.0])));
    assert!(!ok(P::CornerCut, DynamicValue::Vec2([1.0, 1.0])));
    assert!(ok(
        P::CornerAccent,
        DynamicValue::Vec4([12.0, 0.0, 12.0, 0.0])
    ));
    assert!(!ok(
        P::CornerAccent,
        DynamicValue::Vec4([f32::INFINITY, 0.0, 0.0, 0.0])
    ));
    assert!(ok(P::CornerAccentWidth, DynamicValue::F32(3.0)));
    assert!(!ok(P::CornerAccentWidth, DynamicValue::F32(-3.0)));
    assert!(ok(P::Shape, DynamicValue::F32(1.0)));
    assert!(ok(P::Shape, DynamicValue::F32(2.0)));
    assert!(!ok(P::Shape, DynamicValue::F32(0.5)));
    assert!(!ok(P::Shape, DynamicValue::F32(3.0)));
    // Angles take any finite value: a looping or additive clip may run past a
    // turn, and a negative sweep runs counter-clockwise.
    for angle in [-7.25, 0.0, 0.999, 1.0, 1e6] {
        assert!(ok(P::ArcStart, DynamicValue::F32(angle)));
        assert!(ok(P::ArcSweep, DynamicValue::F32(angle)));
    }
    assert!(!ok(P::ArcStart, DynamicValue::F32(f32::INFINITY)));
    assert!(!ok(P::ArcSweep, DynamicValue::F32(f32::NAN)));
    assert!(!ok(P::ArcStart, DynamicValue::Vec2([0.0, 1.0])));
    assert!(ok(P::ArcDashes, DynamicValue::Vec2([48.0, 0.25])));
    assert!(ok(P::ArcDashes, DynamicValue::Vec2([0.0, 1.0])));
    assert!(ok(P::ArcDashes, DynamicValue::Vec2([7.5, 0.0])));
    assert!(!ok(P::ArcDashes, DynamicValue::Vec2([-1.0, 0.5])));
    assert!(!ok(P::ArcDashes, DynamicValue::Vec2([8.0, 1.5])));
    assert!(!ok(P::ArcDashes, DynamicValue::Vec2([8.0, -0.5])));
    assert!(!ok(P::ArcDashes, DynamicValue::F32(8.0)));
    assert!(ok(P::StrokeA, DynamicValue::Vec4([0.0, 0.5, 1.0, 1.0])));
    assert!(!ok(P::StrokeA, DynamicValue::Vec4([0.0, 0.5, 1.0, 1.01])));
    assert!(!ok(P::StrokeB, DynamicValue::Vec4([-0.01, 0.5, 1.0, 1.0])));
}
