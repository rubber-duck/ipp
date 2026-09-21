use super::*;
use crate::services::asset_management::{AssetKey, AssetSource, AssetTypeId};
#[cfg(feature = "gui")]
use std::collections::BTreeMap;

fn resource(kind: u16) -> SurfaceRenderResource {
    SurfaceRenderResource {
        key: AssetKey {
            slot: 1,
            generation: 1,
        },
        source: AssetSource {
            kind: AssetTypeId(kind),
            uri: format!("asset://{kind}/7"),
            variant: 0,
        },
    }
}

fn style(clip: Option<SurfaceClipRect>) -> SurfacePrimitiveStyle {
    SurfacePrimitiveStyle {
        identity: SurfacePrimitiveIdentity::Authored(SurfaceItemId(1)),
        position: [0.25, 0.5],
        scale: [1.0, 1.0],
        color: [1.0, 0.5, 0.25, 1.0],
        opacity: 0.5,
        clip,
    }
}

fn drawing(clip: Option<SurfaceClipRect>) -> SurfaceRenderPrimitive {
    SurfaceRenderPrimitive::Drawing {
        style: style(clip),
        drawing: resource(18),
    }
}

#[test]
fn root_clip_covers_exact_content_rectangle() {
    assert_eq!(surface_content_clip(3.8, 2.4), Some([0.0, 0.0, 3.8, 2.4]));
    assert_eq!(surface_content_clip(0.0, 2.4), None);
    assert_eq!(surface_content_clip(3.8, -1.0), None);
    assert_eq!(surface_content_clip(f32::NAN, 2.4), None);
    assert_eq!(surface_content_clip(3.8, f32::INFINITY), None);
}

#[test]
fn empty_detection_covers_inverted_degenerate_and_nonfinite_clips() {
    assert!(!surface_clip_is_empty([0.0, 0.0, 3.8, 2.4]));
    assert!(surface_clip_is_empty([1.0, 1.0, 1.0, 2.0]));
    assert!(surface_clip_is_empty([0.0, 1.0, 2.0, 1.0]));
    assert!(surface_clip_is_empty([2.0, 0.0, 1.0, 2.0]));
    assert!(surface_clip_is_empty([0.0, 2.0, 2.0, 1.0]));
    assert!(surface_clip_is_empty([f32::NAN, 0.0, 1.0, 1.0]));
    assert!(surface_clip_is_empty([0.0, 0.0, f32::INFINITY, 1.0]));
}

#[test]
fn intersection_nests_clips_and_rejects_disjoint_or_touching_rects() {
    assert_eq!(
        intersect_surface_clips([0.0, 0.0, 3.8, 2.4], [1.0, 0.5, 2.0, 1.5]),
        Some([1.0, 0.5, 2.0, 1.5])
    );
    assert_eq!(
        intersect_surface_clips([1.0, 0.5, 2.0, 1.5], [1.5, 1.0, 3.0, 2.0]),
        Some([1.5, 1.0, 2.0, 1.5])
    );
    assert_eq!(
        intersect_surface_clips([0.0, 0.0, 1.0, 1.0], [2.0, 2.0, 3.0, 3.0]),
        None
    );
    assert_eq!(
        intersect_surface_clips([0.0, 0.0, 1.0, 1.0], [1.0, 0.0, 2.0, 1.0]),
        None
    );
    assert_eq!(
        intersect_surface_clips([0.0, 0.0, f32::NAN, 1.0], [0.0, 0.0, 1.0, 1.0]),
        None
    );
}

#[test]
fn effective_clip_defaults_to_root_and_intersects_style_clips() {
    let size = [3.8, 2.4];

    assert_eq!(
        primitive_effective_clip(&style(None), size),
        Some([0.0, 0.0, 3.8, 2.4])
    );
    assert_eq!(
        primitive_effective_clip(&style(Some([1.0, 1.0, 2.0, 2.0])), size),
        Some([1.0, 1.0, 2.0, 2.0])
    );
    assert_eq!(
        primitive_effective_clip(&style(Some([-1.0, -1.0, 1.0, 1.0])), size),
        Some([0.0, 0.0, 1.0, 1.0])
    );
    assert_eq!(
        primitive_effective_clip(&style(Some([9.0, 9.0, 10.0, 10.0])), size),
        None
    );
    assert_eq!(
        primitive_effective_clip(&style(Some([0.0, 0.0, 1.0, f32::NAN])), size),
        None
    );
    assert_eq!(primitive_effective_clip(&style(None), [0.0, 2.4]), None);
}

#[test]
fn unclipped_primitives_survive_on_valid_surfaces() {
    let size = [3.8, 2.4];
    let glyphs = SurfaceRenderPrimitive::Glyphs {
        style: style(None),
        font: resource(17),
        font_size: 0.1,
        glyphs: Vec::new(),
    };
    let bitmap = SurfaceRenderPrimitive::Bitmap {
        style: style(None),
        bitmap: resource(2),
        size: [0.4, 0.4],
    };

    assert!(surface_primitive_visible(&glyphs, size));
    assert!(surface_primitive_visible(&drawing(None), size));
    assert!(surface_primitive_visible(&bitmap, size));
    assert!(!surface_primitive_visible(&drawing(None), [0.0, 2.4]));
}

#[test]
fn empty_intersection_suppresses_every_raw_primitive_kind() {
    let size = [3.8, 2.4];
    let outside = Some([9.0, 9.0, 10.0, 10.0]);
    let glyphs = SurfaceRenderPrimitive::Glyphs {
        style: style(outside),
        font: resource(17),
        font_size: 0.1,
        glyphs: Vec::new(),
    };
    let bitmap = SurfaceRenderPrimitive::Bitmap {
        style: style(outside),
        bitmap: resource(2),
        size: [0.4, 0.4],
    };

    assert!(!surface_primitive_visible(&glyphs, size));
    assert!(!surface_primitive_visible(&drawing(outside), size));
    assert!(!surface_primitive_visible(&bitmap, size));
    assert!(surface_primitive_visible(
        &drawing(Some([1.0, 1.0, 2.0, 2.0])),
        size
    ));
}

#[test]
fn style_accessor_returns_shared_style_for_every_raw_kind() {
    let clip = Some([1.0, 1.0, 2.0, 2.0]);
    let glyphs = SurfaceRenderPrimitive::Glyphs {
        style: style(clip),
        font: resource(17),
        font_size: 0.1,
        glyphs: Vec::new(),
    };
    let bitmap = SurfaceRenderPrimitive::Bitmap {
        style: style(clip),
        bitmap: resource(2),
        size: [0.4, 0.4],
    };

    assert_eq!(glyphs.style().clip, clip);
    assert_eq!(drawing(clip).style().clip, clip);
    assert_eq!(bitmap.style().clip, clip);
    assert_eq!(
        glyphs.style().identity,
        SurfacePrimitiveIdentity::Authored(SurfaceItemId(1))
    );
}

#[test]
fn authored_and_gui_identities_occupy_disjoint_domains() {
    let authored = SurfacePrimitiveIdentity::Authored(SurfaceItemId(1));

    assert_ne!(
        authored,
        SurfacePrimitiveIdentity::Authored(SurfaceItemId(2))
    );
    #[cfg(feature = "gui")]
    {
        let node = GuiPrimitiveId {
            root_incarnation: 7,
            node: crate::systems::gui::GuiNodeId(1),
            lifetime: 0,
            part: GuiPrimitivePart::Background,
        };
        let gui = SurfacePrimitiveIdentity::Gui(node);

        assert_ne!(authored, gui);
        assert_eq!(gui, SurfacePrimitiveIdentity::Gui(node));
        assert_ne!(
            gui,
            SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
                root_incarnation: 7,
                node: crate::systems::gui::GuiNodeId(1),
                lifetime: 1,
                part: GuiPrimitivePart::Background,
            })
        );
        assert_ne!(
            gui,
            SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
                root_incarnation: 7,
                node: crate::systems::gui::GuiNodeId(2),
                lifetime: 0,
                part: GuiPrimitivePart::Background,
            })
        );
        assert_ne!(
            gui,
            SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
                root_incarnation: 8,
                ..node
            })
        );
        assert_ne!(
            gui,
            SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
                part: GuiPrimitivePart::Label,
                ..node
            })
        );
    }
}

#[cfg(feature = "gui")]
fn gui_style(node: u32, lifetime: u32, clip: Option<SurfaceClipRect>) -> SurfacePrimitiveStyle {
    SurfacePrimitiveStyle {
        identity: SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
            root_incarnation: 1,
            node: crate::systems::gui::GuiNodeId(node),
            lifetime,
            part: GuiPrimitivePart::Background,
        }),
        position: [0.5, 0.5],
        scale: [1.0, 1.0],
        color: [0.2, 0.4, 0.8, 1.0],
        opacity: 1.0,
        clip,
    }
}

#[cfg(feature = "gui")]
fn gui_box(
    size: [f32; 2],
    corner_radius: [f32; 2],
    border_width: f32,
    border_color: [f32; 4],
    clip: Option<SurfaceClipRect>,
) -> SurfaceRenderPrimitive {
    SurfaceRenderPrimitive::Box {
        style: gui_style(3, 0, clip),
        size,
        corner_radius,
        border_width,
        border_color,
    }
}

#[cfg(feature = "gui")]
#[test]
fn valid_boxes_survive_with_sharp_rounded_and_bordered_shapes() {
    let size = [3.8, 2.4];

    assert!(surface_primitive_visible(
        &gui_box([1.0, 0.5], [0.0, 0.0], 0.0, [0.0, 0.0, 0.0, 1.0], None),
        size
    ));
    assert!(surface_primitive_visible(
        &gui_box([1.0, 0.5], [0.1, 0.05], 0.0, [0.0, 0.0, 0.0, 1.0], None),
        size
    ));
    assert!(surface_primitive_visible(
        &gui_box(
            [1.0, 0.5],
            [0.1, 0.1],
            0.02,
            [1.0, 1.0, 1.0, 1.0],
            Some([0.25, 0.25, 1.5, 1.0])
        ),
        size
    ));
}

#[cfg(feature = "gui")]
#[test]
fn invalid_box_parameters_suppress_only_the_box() {
    let size = [3.8, 2.4];
    let valid = gui_box([1.0, 0.5], [0.1, 0.1], 0.02, [1.0, 1.0, 1.0, 1.0], None);

    for invalid in [
        gui_box([0.0, 0.5], [0.0, 0.0], 0.0, [0.0, 0.0, 0.0, 1.0], None),
        gui_box([-1.0, 0.5], [0.0, 0.0], 0.0, [0.0, 0.0, 0.0, 1.0], None),
        gui_box([f32::NAN, 0.5], [0.0, 0.0], 0.0, [0.0, 0.0, 0.0, 1.0], None),
        gui_box([1.0, 0.5], [-0.1, 0.0], 0.0, [0.0, 0.0, 0.0, 1.0], None),
        gui_box([1.0, 0.5], [0.0, 0.0], -0.01, [0.0, 0.0, 0.0, 1.0], None),
        gui_box([1.0, 0.5], [0.0, 0.0], f32::NAN, [0.0, 0.0, 0.0, 1.0], None),
        gui_box([1.0, 0.5], [0.0, 0.0], 0.0, [2.0, 0.0, 0.0, 1.0], None),
        gui_box(
            [1.0, 0.5],
            [0.1, 0.1],
            0.02,
            [1.0, 1.0, 1.0, 1.0],
            Some([9.0, 9.0, 10.0, 10.0]),
        ),
    ] {
        assert!(!surface_primitive_visible(&invalid, size));
    }
    assert!(surface_primitive_visible(&valid, size));
}

#[cfg(feature = "gui")]
#[test]
fn box_style_accessor_exposes_shared_clip_and_identity() {
    let primitive = gui_box(
        [1.0, 0.5],
        [0.05, 0.05],
        0.01,
        [0.0, 0.0, 0.0, 1.0],
        Some([0.0, 0.0, 2.0, 2.0]),
    );

    assert_eq!(primitive.style().clip, Some([0.0, 0.0, 2.0, 2.0]));
    assert_eq!(
        primitive.style().identity,
        SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
            root_incarnation: 1,
            node: crate::systems::gui::GuiNodeId(3),
            lifetime: 0,
            part: GuiPrimitivePart::Background,
        })
    );
}

#[cfg(feature = "gui")]
#[test]
fn gui_logical_mapping_scales_units_without_flips_or_offsets() {
    assert_eq!(
        gui_logical_to_surface_content([350.0, 225.0], 100.0),
        Some([3.5, 2.25])
    );
    assert_eq!(
        surface_content_to_gui_logical([3.5, 2.25], 100.0),
        Some([350.0, 225.0])
    );
    assert_eq!(
        gui_logical_to_surface_content([0.0, 0.0], 100.0),
        Some([0.0, 0.0])
    );

    let logical = [300.0, 150.0];
    let content = gui_logical_to_surface_content(logical, 100.0).unwrap();

    assert_eq!(
        surface_content_to_gui_logical(content, 100.0),
        Some(logical)
    );
    assert_eq!(gui_logical_to_surface_content(logical, 0.0), None);
    assert_eq!(gui_logical_to_surface_content(logical, -2.0), None);
    assert_eq!(gui_logical_to_surface_content([f32::NAN, 0.0], 100.0), None);
    assert_eq!(
        surface_content_to_gui_logical([1.0, f32::INFINITY], 100.0),
        None
    );
    assert_eq!(surface_content_to_gui_logical([1.0, 1.0], f32::NAN), None);
}

#[test]
fn skin_style_override_preserves_identity_order_and_payload() {
    let base = drawing(None);
    let styled = super::surface_primitive_with_skin_style(
        &base,
        Some([0.0, 1.0, 0.0, 1.0]),
        Some(0.25),
        Some([2.0, 3.0]),
    );
    assert_eq!(styled.style().identity, base.style().identity);
    assert_eq!(styled.style().position, base.style().position);
    assert_eq!(styled.style().clip, base.style().clip);
    assert_eq!(styled.style().color, [0.0, 1.0, 0.0, 1.0]);
    assert_eq!(styled.style().opacity, 0.25);
    assert_eq!(styled.style().scale, [2.0, 3.0]);
    match (&base, &styled) {
        (
            SurfaceRenderPrimitive::Drawing {
                drawing: before,
                ..
            },
            SurfaceRenderPrimitive::Drawing {
                drawing: after,
                ..
            },
        ) => assert_eq!(before, after),
        _ => panic!("expected drawing"),
    }
    // Invalid lanes keep the base field; behaviour never moves.
    let kept = super::surface_primitive_with_skin_style(
        &base,
        Some([9.0, 0.0, 0.0, 1.0]),
        Some(f32::NAN),
        Some([f32::INFINITY, 1.0]),
    );
    assert_eq!(kept.style().color, base.style().color);
    assert_eq!(kept.style().opacity, base.style().opacity);
    assert_eq!(kept.style().scale, base.style().scale);
    assert_eq!(kept.style().identity, base.style().identity);
}

#[cfg(feature = "gui")]
fn scrolled_kinds() -> Vec<SurfaceRenderPrimitive> {
    let style = SurfacePrimitiveStyle {
        identity: SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
            root_incarnation: 1,
            node: crate::systems::gui::GuiNodeId(5),
            lifetime: 2,
            part: GuiPrimitivePart::Background,
        }),
        position: [1.0, 2.0],
        scale: [1.0, 1.0],
        color: [1.0, 1.0, 1.0, 1.0],
        opacity: 1.0,
        clip: Some([0.0, 0.0, 10.0, 4.0]),
    };
    vec![
        SurfaceRenderPrimitive::Box {
            style,
            size: [10.0, 4.0],
            corner_radius: [0.0, 0.0],
            border_width: 0.0,
            border_color: [0.0, 0.0, 0.0, 0.0],
        },
        SurfaceRenderPrimitive::Glyphs {
            style,
            font: resource(17),
            font_size: 0.1,
            glyphs: vec![SurfaceGlyph {
                glyph_id: 3,
                position: [0.25, 0.5],
                color: None,
            }],
        },
        SurfaceRenderPrimitive::Drawing {
            style,
            drawing: resource(18),
        },
        SurfaceRenderPrimitive::Bitmap {
            style,
            bitmap: resource(2),
            size: [10.0, 4.0],
        },
    ]
}

#[cfg(feature = "gui")]
#[test]
fn scroll_translation_moves_every_kind_keeping_clips_and_payloads() {
    let base = scrolled_kinds();
    let shifts: BTreeMap<(crate::systems::gui::GuiNodeId, u32), [f32; 2]> =
        BTreeMap::from([((crate::systems::gui::GuiNodeId(5), 2), [0.5, -4.0])]);

    let moved = super::translate_gui_primitives_for_scroll(base.clone(), &shifts);
    assert_eq!(moved.len(), base.len());
    for (before, after) in base.iter().zip(&moved) {
        assert_eq!(
            after.style().position,
            [
                before.style().position[0] + 0.5,
                before.style().position[1] - 4.0
            ]
        );
        assert_eq!(after.style().clip, before.style().clip);
        assert_eq!(after.style().identity, before.style().identity);
        assert_eq!(after.style().scale, before.style().scale);
        assert_eq!(after.style().color, before.style().color);
        assert_eq!(after.style().opacity, before.style().opacity);
    }
    // Payloads stay node-local: glyph origins, box geometry, bitmap sizes.
    match &moved[1] {
        SurfaceRenderPrimitive::Glyphs {
            glyphs,
            ..
        } => assert_eq!(glyphs[0].position, [0.25, 0.5]),
        _ => panic!("expected glyphs"),
    }
    match (&base[0], &moved[0]) {
        (
            SurfaceRenderPrimitive::Box {
                size: before,
                ..
            },
            SurfaceRenderPrimitive::Box {
                size: after,
                ..
            },
        ) => assert_eq!(before, after),
        _ => panic!("expected boxes"),
    }
    match (&base[3], &moved[3]) {
        (
            SurfaceRenderPrimitive::Bitmap {
                size: before,
                ..
            },
            SurfaceRenderPrimitive::Bitmap {
                size: after,
                ..
            },
        ) => assert_eq!(before, after),
        _ => panic!("expected bitmaps"),
    }
}

#[cfg(feature = "gui")]
#[test]
fn scroll_translation_fences_lifetime_and_ignores_unshifted_paint() {
    let mut base = scrolled_kinds();
    base.push(SurfaceRenderPrimitive::Drawing {
        style: style(None),
        drawing: resource(18),
    });

    // Stale lifetimes, unknown nodes and empty maps pass through untouched,
    // as does the authored primitive which GUI scrolling never moves.
    let stale: BTreeMap<(crate::systems::gui::GuiNodeId, u32), [f32; 2]> =
        BTreeMap::from([((crate::systems::gui::GuiNodeId(5), 7), [0.5, -4.0])]);
    assert_eq!(
        super::translate_gui_primitives_for_scroll(base.clone(), &stale),
        base
    );
    assert_eq!(
        super::translate_gui_primitives_for_scroll(base.clone(), &BTreeMap::new()),
        base
    );
    // Zero and non-finite shifts never move paint.
    let zero: BTreeMap<(crate::systems::gui::GuiNodeId, u32), [f32; 2]> =
        BTreeMap::from([((crate::systems::gui::GuiNodeId(5), 2), [0.0, 0.0])]);
    assert_eq!(
        super::translate_gui_primitives_for_scroll(base.clone(), &zero),
        base
    );
    let wild: BTreeMap<(crate::systems::gui::GuiNodeId, u32), [f32; 2]> =
        BTreeMap::from([((crate::systems::gui::GuiNodeId(5), 2), [f32::NAN, 0.0])]);
    assert_eq!(
        super::translate_gui_primitives_for_scroll(base.clone(), &wild),
        base
    );
}
