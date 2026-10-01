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
    let appearance: Vec<_> = GuiPartProperty::ALL
        .into_iter()
        .filter(|property| property.appearance())
        .collect();
    assert_eq!(appearance.len(), 18);
    assert_eq!(layout.property_count(), appearance.len() as u32 + 1);
    for property in appearance {
        let lane = &layout.properties[property.index() as usize];
        assert_eq!(lane.name, property.name());
        assert_eq!(lane.kind, property.kind());
        assert!(lane.optional, "{property:?}");
    }
    let key = &layout.properties[18];
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
    // Text-input overlays are paint identities, not skin parts: their rows
    // resolve through the Label they annotate.
    assert_eq!(
        row(GuiPartId::base(GuiPrimitivePart::Caret)).part,
        row(GuiPartId::base(GuiPrimitivePart::Label)).part
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
    let invalid: [fn(&mut GuiPaintPart); 13] = [
        |part| part.color = Some([1.5, 0.0, 0.0, 1.0]),
        |part| part.opacity = Some(-0.1),
        |part| part.scale = Some([f32::NAN, 1.0]),
        |part| part.align_x = Some(f32::INFINITY),
        |part| part.corner_radius = Some([-1.0, 0.0]),
        |part| part.border_width = Some(-1.0),
        |part| part.border_color = Some([0.0, 0.0, 0.0, 2.0]),
        |part| part.fill_mode = Some(3.0),
        |part| part.gradient_color0 = Some([-0.5, 0.0, 0.0, 1.0]),
        |part| part.gradient_radius = Some(-1.0),
        |part| part.glow_intensity = Some(-1.0),
        |part| part.glow_radius = Some(f32::NAN),
        |part| part.glow_falloff = Some(-1.0),
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
        fill_mode: Some(2.0),
        gradient_start: Some([-5.0, 5.0]),
        gradient_radius: Some(0.0),
        glow_intensity: Some(0.0),
        glow_radius: Some(8.0),
        glow_falloff: Some(0.0),
        ..row(background)
    };
    assert_eq!(theme([valid.clone()]), Ok(()));
    assert_eq!(skin([valid]), Ok(()));
}
