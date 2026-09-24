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
fn theme_part_layout_is_the_part_properties_then_the_theme_key() {
    let layout = GuiThemePartRow::LAYOUT;
    let names: Vec<_> = layout.properties.iter().map(|p| p.name).collect();

    assert_eq!(names[..23], PART_PROPERTIES);
    assert_eq!(names[23], "theme");
    assert_eq!(layout.property_count(), GuiThemePartRow::THEME + 1);
    assert!(!layout.properties[GuiThemePartRow::THEME as usize].optional);
    for (index, property) in GuiPartProperty::ALL.into_iter().enumerate() {
        assert_eq!(property.index(), index as u32);
        assert_eq!(GuiPartProperty::from_index(index as u32), Some(property));
        assert_eq!(property.name(), PART_PROPERTIES[index]);
        assert!(layout.properties[index].optional);
        assert_eq!(property.appearance(), index < 18);
    }
    assert_eq!(GuiPartProperty::from_index(23), None);
}

#[test]
fn part_row_shares_appearance_indices_then_channels_then_keys() {
    let layout = GuiPartRow::LAYOUT;
    let names: Vec<_> = layout.properties.iter().map(|p| p.name).collect();

    assert_eq!(names[..18], PART_PROPERTIES[..18]);
    assert_eq!(
        names[18..],
        [
            "live_color",
            "live_opacity",
            "live_scale",
            "live_align_x",
            "node",
            "part"
        ]
    );
    assert_eq!(layout.property_count(), GuiPartRowProperty::COUNT);
    for index in 0..18 {
        assert_eq!(
            layout.properties[index].kind,
            GuiThemePartRow::LAYOUT.properties[index].kind
        );
    }
    for channel in GuiPartChannel::ALL {
        assert_eq!(
            layout.properties[channel.index() as usize].kind,
            channel.property().kind()
        );
        assert_eq!(
            GuiPartRowProperty::from_index(channel.index()),
            Some(GuiPartRowProperty::Channel(channel))
        );
    }
    assert_eq!(
        GuiPartRowProperty::from_index(GuiPartRow::NODE),
        Some(GuiPartRowProperty::Key)
    );
    assert_eq!(
        GuiPartRowProperty::from_index(GuiPartRowProperty::COUNT),
        None
    );
    assert!(!GuiPartRowProperty::Key.numeric_animatable());
    assert!(!GuiPartRowProperty::Override(GuiPartProperty::Asset).numeric_animatable());
    assert!(GuiPartRowProperty::Channel(GuiPartChannel::Scale).numeric_animatable());
}

#[test]
fn part_identities_enumerate_every_base_state_and_variant_once() {
    assert_eq!(GuiPartId::COUNT, 65);
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

#[test]
fn part_rows_open_and_close_live_channels_without_touching_overrides() {
    let mut row = GuiPartRow::keyed(3, GuiPrimitivePart::Icon);
    assert_eq!(row.part, 3);
    assert!(!row.has_overrides() && !row.has_channels());

    row.color = Some([1.0, 0.0, 0.0, 1.0]);
    let base = GuiThemePartRow {
        opacity: Some(0.5),
        ..GuiThemePartRow::for_theme(1)
    };
    row.open_channels(Some(&base));
    assert_eq!(
        (row.live_color, row.live_opacity),
        (Some([1.0; 4]), Some(0.5))
    );
    assert!(row.has_overrides() && row.has_channels());
    row.validate().unwrap();

    row.close_channels();
    assert!(row.has_overrides() && !row.has_channels());
    assert_eq!(row.color, Some([1.0, 0.0, 0.0, 1.0]));

    let mut unkeyed = GuiPartRow::default();
    unkeyed.open_channels(None);
    assert!(unkeyed.validate().is_err());
}

#[test]
fn patches_apply_by_part_property_index() {
    let patch = GuiPartPatch::default()
        .set(GuiPartProperty::Opacity, DynamicValue::F32(0.25))
        .clear(GuiPartProperty::Color);
    let mut theme = GuiThemePartRow {
        color: Some([1.0; 4]),
        ..GuiThemePartRow::for_theme(9)
    };
    patch.apply(&mut theme).unwrap();
    assert_eq!(
        (theme.color, theme.opacity, theme.theme),
        (None, Some(0.25), 9)
    );

    let mut part = GuiPartRow::keyed(2, GuiPrimitivePart::Background);
    patch.apply(&mut part).unwrap();
    assert_eq!((part.color, part.opacity), (None, Some(0.25)));

    let invalid = GuiPartPatch::default().set(GuiPartProperty::Opacity, DynamicValue::F32(2.0));
    assert_eq!(invalid.validate(), Err(ErrorReason::InvalidValue));
}

mod roots {
    use super::*;
    use crate::components::schema::ComponentLifecycle;
    use crate::systems::gui::test_support::{
        apply_writes, ensure_themed_nodes, set_node_theme, set_theme_part,
    };
    use crate::systems::gui::{GuiNodeId, GuiRoot};

    fn motion() -> DynamicValue {
        DynamicValue::Asset(AssetSource {
            kind: crate::systems::animation::ANIMATION_TYPE,
            uri: "fade.ippa".into(),
            variant: 0,
        })
    }

    fn channels(root: &GuiRoot, node: u32, part: GuiPrimitivePart) -> Option<GuiPartRow> {
        root.part_row(GuiNodeId(node), part)
            .map(|(_, row)| row.clone())
    }

    #[test]
    fn theme_motion_opens_and_closes_channels_of_referencing_nodes_only() {
        let mut root = GuiRoot::default();
        ensure_themed_nodes(&mut root, 3);
        set_node_theme(&mut root, GuiNodeId(3), Some(1));
        let base = [0.2, 0.4, 0.6, 1.0];
        set_theme_part(
            &mut root,
            1,
            "background",
            "color",
            Some(DynamicValue::Vec4(base)),
        );

        set_theme_part(&mut root, 1, "background_hovered", "motion", Some(motion()));
        for node in [1, 3] {
            let row = channels(&root, node, GuiPrimitivePart::Background).unwrap();
            assert!(row.has_channels() && !row.has_overrides());
            // Opened channels start from the part's base appearance.
            assert_eq!(row.live_color, Some(base));
        }
        assert_eq!(channels(&root, 2, GuiPrimitivePart::Background), None);
        assert_eq!(channels(&root, 1, GuiPrimitivePart::Icon), None);
        root.validate_complete().unwrap();

        // An override keeps its row when motion goes; channel-only rows go.
        let writes = root
            .part_override_writes(
                GuiNodeId(3),
                GuiPrimitivePart::Background,
                &GuiPartPatch::default().set(GuiPartProperty::Opacity, DynamicValue::F32(0.5)),
            )
            .unwrap();
        apply_writes(&mut root, writes);
        set_theme_part(&mut root, 1, "background_hovered", "motion", None);
        assert_eq!(channels(&root, 1, GuiPrimitivePart::Background), None);
        let kept = channels(&root, 3, GuiPrimitivePart::Background).unwrap();
        assert!(!kept.has_channels() && kept.opacity == Some(0.5));

        // Pointing a node at an animated theme opens its channels; clearing
        // the reference closes them.
        set_theme_part(&mut root, 2, "icon", "motion", Some(motion()));
        assert!(
            channels(&root, 2, GuiPrimitivePart::Icon)
                .unwrap()
                .has_channels()
        );
        set_node_theme(&mut root, GuiNodeId(2), None);
        assert_eq!(channels(&root, 2, GuiPrimitivePart::Icon), None);
        root.validate_complete().unwrap();
    }

    #[test]
    fn removed_nodes_take_their_part_rows_and_themes_allocate_fresh_slots() {
        let mut root = GuiRoot::default();
        ensure_themed_nodes(&mut root, 2);
        set_theme_part(&mut root, 2, "focusRing", "motion", Some(motion()));
        assert!(channels(&root, 2, GuiPrimitivePart::FocusRing).is_some());
        root.remove_node(GuiNodeId(2)).unwrap();
        assert!(root.part_state().is_empty());

        let first = root.theme_slot(2).unwrap();
        let removal = root.theme_removal_write(2).unwrap();
        apply_writes(&mut root, vec![removal]);
        assert_eq!(root.theme_slot(2), None);
        assert!(root.theme_removal_write(2).is_err());
        set_theme_part(
            &mut root,
            2,
            "background",
            "opacity",
            Some(DynamicValue::F32(1.0)),
        );
        assert!(root.theme_slot(2).unwrap() > first);
        root.validate_complete().unwrap();
    }

    #[test]
    fn restored_tables_rebuild_their_lookups() {
        use crate::components::schema::SchemaComponent;

        let mut root = GuiRoot::default();
        ensure_themed_nodes(&mut root, 3);
        set_theme_part(&mut root, 3, "background", "motion", Some(motion()));
        set_theme_part(
            &mut root,
            1,
            "icon_pressed_checked",
            "color",
            Some(DynamicValue::Vec4([1.0; 4])),
        );

        let mut restored = crate::ComponentValue::create(crate::ComponentValue::GUI_ROOT).unwrap();
        for (offset, value) in root.fields() {
            restored.set_field(offset, value).unwrap();
        }
        let crate::ComponentValue::GuiRoot(restored) = restored else {
            unreachable!("GUI root restores as a GUI root")
        };
        assert_eq!(restored, root);
        assert_eq!(restored.theme_slot(3), root.theme_slot(3));
        assert_eq!(
            channels(&restored, 3, GuiPrimitivePart::Background),
            channels(&root, 3, GuiPrimitivePart::Background)
        );
        ComponentLifecycle::validate(&restored).unwrap();
    }
}
