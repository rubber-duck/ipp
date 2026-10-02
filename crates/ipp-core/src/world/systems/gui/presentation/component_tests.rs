use super::*;
use crate::components::rows::SchemaRow;
use crate::services::asset_management::drawing::DRAWING_TYPE;
use crate::systems::gui::{GuiPartVariant, GuiSkinState};

fn row(identity: GuiPartId) -> GuiPaintPart {
    GuiPaintPart::keyed(identity).unwrap()
}

fn rows(parts: impl IntoIterator<Item = GuiPaintPart>) -> Rows<GuiPaintPart> {
    let mut rows = Rows::new();
    for part in parts {
        rows.push(part).unwrap();
    }
    rows
}

fn theme(parts: impl IntoIterator<Item = GuiPaintPart>) -> Result<(), ErrorReason> {
    GuiTheme {
        parts: rows(parts),
        ..Default::default()
    }
    .validate()
}

fn skin(parts: impl IntoIterator<Item = GuiPaintPart>) -> Result<(), ErrorReason> {
    GuiSkin {
        parts: rows(parts),
        ..Default::default()
    }
    .validate()
}

fn asset(kind: crate::services::asset_management::AssetTypeId) -> Option<AssetSource> {
    Some(AssetSource {
        kind,
        uri: "skin:///part".into(),
        variant: 0,
    })
}

#[test]
fn paint_rows_expose_the_appearance_properties_in_part_property_order_then_the_key() {
    let layout = GuiPaintPart::LAYOUT;
    let appearance = GuiPartProperty::ALL;
    assert_eq!(appearance.len(), 32);
    assert_eq!(layout.property_count(), appearance.len() as u32 + 1);
    for property in appearance {
        let lane = &layout.properties[property.index() as usize];
        assert_eq!(lane.name, property.name());
        assert_eq!(lane.kind, property.kind());
        assert!(lane.optional, "{property:?}");
    }
    let key = &layout.properties[32];
    assert_eq!(key.name, "part");
    assert!(!key.optional);
}

#[test]
fn themes_and_skins_reject_duplicate_and_unknown_part_identities() {
    let background = GuiPartId::base(GuiPrimitivePart::Background);
    assert_eq!(theme([row(background)]), Ok(()));
    assert_eq!(
        theme([row(background), row(background)]),
        Err(ErrorReason::InvalidValue)
    );
    assert_eq!(
        skin([row(background), row(background)]),
        Err(ErrorReason::InvalidValue)
    );
    let unknown = GuiPaintPart {
        part: GuiPartId::COUNT,
        ..Default::default()
    };
    assert_eq!(theme([unknown.clone()]), Err(ErrorReason::InvalidValue));
    assert_eq!(skin([unknown]), Err(ErrorReason::InvalidValue));
    // The caret and selection are skin parts of their own; the provisional
    // composition underline resolves through the Label it underlines.
    let label = row(GuiPartId::base(GuiPrimitivePart::Label)).part;
    for part in [GuiPrimitivePart::Caret, GuiPrimitivePart::Selection] {
        assert_ne!(row(GuiPartId::base(part)).part, label);
        assert_eq!(theme([row(GuiPartId::base(part))]), Ok(()));
    }
    assert_eq!(
        row(GuiPartId::base(GuiPrimitivePart::Composition)).part,
        label
    );
}

#[test]
fn per_control_overrides_accept_only_unqualified_parts() {
    let hovered = GuiPartId::state(GuiPrimitivePart::Background, GuiSkinState::Hovered);
    let checked = GuiPartId::variant(
        GuiPrimitivePart::Icon,
        GuiSkinState::Idle,
        GuiPartVariant::Checked,
    );
    // A theme may qualify parts by state and variant; an override wins in every
    // state, so it names only the base part.
    assert_eq!(theme([row(hovered), row(checked)]), Ok(()));
    assert_eq!(skin([row(hovered)]), Err(ErrorReason::InvalidValue));
    assert_eq!(skin([row(checked)]), Err(ErrorReason::InvalidValue));
    assert_eq!(skin([row(GuiPartId::base(GuiPrimitivePart::Icon))]), Ok(()));
}

#[test]
fn skin_assets_must_be_drawings_or_textures_on_shape_parts() {
    for part in [
        GuiPrimitivePart::Background,
        GuiPrimitivePart::Fill,
        GuiPrimitivePart::Icon,
        GuiPrimitivePart::FocusRing,
    ] {
        for kind in [DRAWING_TYPE, crate::TEXTURE_TYPE] {
            let part = GuiPaintPart {
                asset: asset(kind),
                ..row(GuiPartId::base(part))
            };
            assert_eq!(theme([part.clone()]), Ok(()));
            assert_eq!(skin([part]), Ok(()));
        }
        let font = GuiPaintPart {
            asset: asset(FONT_TYPE),
            ..row(GuiPartId::base(part))
        };
        assert_eq!(theme([font]), Err(ErrorReason::InvalidValue), "{part:?}");
    }
    for part in [
        GuiPrimitivePart::Label,
        GuiPrimitivePart::ScrollTrackX,
        GuiPrimitivePart::ScrollThumbY,
    ] {
        let drawing = GuiPaintPart {
            asset: asset(DRAWING_TYPE),
            ..row(GuiPartId::base(part))
        };
        assert_eq!(
            theme([drawing.clone()]),
            Err(ErrorReason::InvalidValue),
            "{part:?}"
        );
        assert_eq!(skin([drawing]), Err(ErrorReason::InvalidValue), "{part:?}");
    }
}

#[test]
fn every_appearance_lane_is_range_checked_in_themes_and_skins() {
    let background = GuiPartId::base(GuiPrimitivePart::Background);
    let invalid: [fn(&mut GuiPaintPart); 27] = [
        |part| part.color = Some([1.5, 0.0, 0.0, 1.0]),
        |part| part.opacity = Some(-0.1),
        |part| part.scale = Some([f32::NAN, 1.0]),
        |part| part.align_x = Some(f32::INFINITY),
        |part| part.corner_radius = Some([-1.0, 0.0]),
        |part| part.border_width = Some(-1.0),
        |part| part.border_color = Some([0.0, 0.0, 0.0, 2.0]),
        |part| part.fill_mode = Some(5.0),
        |part| part.gradient_color0 = Some([-0.5, 0.0, 0.0, 1.0]),
        |part| part.gradient_radius = Some(-1.0),
        |part| part.glow_intensity = Some(-1.0),
        |part| part.glow_radius = Some(f32::NAN),
        |part| part.glow_falloff = Some(-1.0),
        |part| part.glow_inner_radius = Some(-1.0),
        |part| part.corner_cut = Some([0.0, f32::NAN, 0.0, 0.0]),
        |part| part.corner_accent = Some([0.0, 0.0, -2.0, 0.0]),
        |part| part.corner_accent_width = Some(-0.5),
        |part| part.shape = Some(3.0),
        |part| part.stroke_a = Some([0.0, 0.0, 1.0, 2.0]),
        |part| part.stroke_b = Some([-1.0, 0.0, 1.0, 1.0]),
        |part| part.arc_start = Some(f32::NAN),
        |part| part.arc_sweep = Some(f32::NEG_INFINITY),
        |part| part.arc_dashes = Some([12.0, 1.25]),
        |part| part.fill_hue = Some(f32::INFINITY),
        |part| part.checker_size = Some(-4.0),
        |part| part.checker_color0 = Some([0.5, 0.5, 0.5, 1.5]),
        |part| part.checker_color1 = Some([0.5, f32::NAN, 0.5, 1.0]),
    ];
    for (index, write) in invalid.into_iter().enumerate() {
        let mut part = row(background);
        write(&mut part);
        assert_eq!(
            theme([part.clone()]),
            Err(ErrorReason::InvalidValue),
            "case {index}"
        );
        assert_eq!(skin([part]), Err(ErrorReason::InvalidValue), "case {index}");
    }
    let valid = GuiPaintPart {
        color: Some([0.0, 0.5, 1.0, 1.0]),
        opacity: Some(0.0),
        scale: Some([-2.0, 0.5]),
        align_x: Some(4.0),
        corner_radius: Some([3.0, 0.0]),
        border_width: Some(0.0),
        fill_mode: Some(4.0),
        gradient_start: Some([-5.0, 5.0]),
        gradient_radius: Some(0.0),
        glow_intensity: Some(0.0),
        glow_radius: Some(8.0),
        glow_falloff: Some(0.0),
        glow_inner_radius: Some(6.0),
        corner_cut: Some([4.0, 0.0, 4.0, 0.0]),
        corner_accent: Some([0.0, 10.0, 0.0, 10.0]),
        corner_accent_width: Some(0.0),
        shape: Some(2.0),
        stroke_a: Some([0.0, 0.0, 1.0, 1.0]),
        stroke_b: Some([0.5, 0.5, 0.5, 0.5]),
        arc_start: Some(-3.5),
        arc_sweep: Some(-0.75),
        arc_dashes: Some([48.0, 0.0]),
        fill_hue: Some(-7.75),
        checker_size: Some(0.0),
        checker_color0: Some([1.0, 1.0, 1.0, 0.0]),
        checker_color1: Some([0.0, 0.0, 0.0, 1.0]),
        ..row(background)
    };
    assert_eq!(theme([valid.clone()]), Ok(()));
    assert_eq!(skin([valid]), Ok(()));
}
