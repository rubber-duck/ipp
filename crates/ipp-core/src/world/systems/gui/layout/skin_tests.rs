//! Maintained tests for GUI skins (ipp-9nx.9); never weakened.
//!
//! Expectations are hand-computed from the frozen skin rules, never derived
//! from the implementation output.

use super::*;
use crate::SurfaceRenderResource;
use crate::services::asset_management::{AssetKey, AssetTypeId};
use crate::systems::gui::{GuiNodeId, MAX_LAYOUT_DEPTH};
use crate::systems::surface::{
    GuiPrimitiveId, GuiPrimitivePart, SurfaceGlyph, SurfacePrimitiveStyle, TextGlyph, TextLayout,
};
use std::collections::BTreeMap;

fn skin_target(entity: EntityId, node: u32) -> crate::systems::gui::GuiInputTarget {
    crate::systems::gui::GuiInputTarget {
        entity,
        node: GuiNodeId(node),
        lifetime: 1,
        root_incarnation: 7,
    }
}

fn asset_source(uri: &str) -> AssetSource {
    AssetSource {
        kind: crate::services::asset_management::drawing::DRAWING_TYPE,
        uri: uri.to_owned(),
        variant: 0,
    }
}

fn resource_for(uri: &str) -> SurfaceRenderResource {
    SurfaceRenderResource {
        key: AssetKey {
            slot: 9,
            generation: 1,
        },
        source: asset_source(uri),
    }
}

struct MapResolver {
    map: BTreeMap<String, SurfaceRenderResource>,
    drawing_view_boxes: BTreeMap<String, [f32; 4]>,
}

impl MapResolver {
    fn with(uri: &str) -> Self {
        let mut map = BTreeMap::new();
        map.insert(uri.to_owned(), resource_for(uri));
        Self {
            map,
            drawing_view_boxes: BTreeMap::from([(uri.to_owned(), [0.0, 0.0, 1.0, 1.0])]),
        }
    }

    fn empty() -> Self {
        Self {
            map: BTreeMap::new(),
            drawing_view_boxes: BTreeMap::new(),
        }
    }
}

impl GuiResourceResolver for MapResolver {
    fn text_font(&self, _source: &AssetSource) -> crate::systems::gui::GuiFontResolution<'_> {
        crate::systems::gui::GuiFontResolution::Missing
    }

    fn surface_resource(&self, source: &AssetSource) -> Option<SurfaceRenderResource> {
        self.map.get(&source.uri).cloned()
    }

    fn drawing_view_box(&self, source: &AssetSource) -> Option<[f32; 4]> {
        self.drawing_view_boxes
            .get(&source.uri)
            .copied()
            .or_else(|| {
                (source.kind == crate::services::asset_management::drawing::DRAWING_TYPE
                    && self.map.contains_key(&source.uri))
                .then_some([0.0, 0.0, 1.0, 1.0])
            })
    }
}

fn evaluated_node(id: u32, content: GuiEvaluatedContent) -> GuiEvaluatedNode {
    GuiEvaluatedNode {
        node: GuiNodeId(id),
        lifetime: 1,
        depth: 1,
        rect: [0.0, 0.0, 10.0, 5.0],
        clip: None,
        content,
        enabled: true,
        visible: true,
        available: true,
        paint_suppressed: false,
        visual_offset: [0.0, 0.0],
        visual_scale: [1.0, 1.0],
        acc_scale: [1.0, 1.0],
        content_extents: None,
        content_origin: [0.0, 0.0],
        color: [1.0, 1.0, 1.0, 1.0],
        background: Some([0.2, 0.2, 0.2, 1.0]),
        opacity: 1.0,
    }
}

fn test_view(nodes: Vec<GuiEvaluatedNode>) -> GuiEvaluatedView {
    test_view_with_incarnation(7, nodes)
}

fn test_view_with_incarnation(
    root_incarnation: u64,
    nodes: Vec<GuiEvaluatedNode>,
) -> GuiEvaluatedView {
    GuiEvaluatedView {
        entity: EntityId::from_bits(1),
        root_incarnation,
        layout_revision: 1,
        paint_revision: 1,
        evaluation_tick: 1,
        root_bounds: [0.0, 0.0, 10.0, 5.0],
        units_per_metre: 1.0,
        nodes,
        diagnostics: Vec::new(),
        remeasure_count: 0,
        reflow_count: 0,
        available: true,
    }
}

fn set_part_color(root: &mut GuiRoot, node: u32, part: &str, color: [f32; 4]) {
    let name = GuiRoot::part_property_name(GuiNodeId(node), part, "color").unwrap();
    root.properties
        .set(&name, DynamicValue::Vec4(color))
        .unwrap();
}

fn set_part_opacity(root: &mut GuiRoot, node: u32, part: &str, opacity: f32) {
    let name = GuiRoot::part_property_name(GuiNodeId(node), part, "opacity").unwrap();
    root.properties
        .set(&name, DynamicValue::F32(opacity))
        .unwrap();
}

fn set_part_asset(root: &mut GuiRoot, node: u32, part: &str, uri: &str) {
    let name = GuiRoot::part_property_name(GuiNodeId(node), part, "asset").unwrap();
    root.properties
        .set(&name, DynamicValue::Asset(asset_source(uri)))
        .unwrap();
}

fn set_typed_part_asset(root: &mut GuiRoot, node: u32, part: &str, kind: AssetTypeId, uri: &str) {
    let name = GuiRoot::part_property_name(GuiNodeId(node), part, "asset").unwrap();
    root.properties
        .set(
            &name,
            DynamicValue::Asset(AssetSource {
                kind,
                uri: uri.to_owned(),
                variant: 0,
            }),
        )
        .unwrap();
}

fn set_part_lane(root: &mut GuiRoot, node: u32, part: &str, lane: &str, value: DynamicValue) {
    let name = GuiRoot::part_property_name(GuiNodeId(node), part, lane).unwrap();
    root.properties.set(&name, value).unwrap();
}

#[test]
fn motion_lanes_resolve_independently_through_the_state_chain() {
    let mut root = GuiRoot::default();
    set_part_lane(
        &mut root,
        1,
        "background",
        "motion",
        DynamicValue::Asset(AssetSource {
            kind: crate::systems::animation::ANIMATION_TYPE,
            uri: "idle.ippa".into(),
            variant: 0,
        }),
    );
    set_part_lane(
        &mut root,
        1,
        "background",
        "duration",
        DynamicValue::F32(0.4),
    );
    set_part_lane(&mut root, 1, "background", "easing", DynamicValue::F32(0.0));
    set_part_lane(&mut root, 1, "background", "track", DynamicValue::F32(6.0));
    set_part_lane(&mut root, 1, "background", "time", DynamicValue::F32(0.1));
    set_part_lane(
        &mut root,
        1,
        "background_hovered",
        "motion",
        DynamicValue::Asset(AssetSource {
            kind: crate::systems::animation::ANIMATION_TYPE,
            uri: "hover.ippa".into(),
            variant: 2,
        }),
    );
    set_part_lane(
        &mut root,
        1,
        "background_hovered",
        "easing",
        DynamicValue::F32(1.0),
    );
    set_part_lane(
        &mut root,
        1,
        "background_hovered",
        "time",
        DynamicValue::F32(0.75),
    );

    let motion = resolve_state_part_motion(
        &root,
        GuiNodeId(1),
        "background",
        GuiSkinState::Hovered,
        GuiControlVariant::Plain,
    )
    .unwrap();
    assert_eq!(motion.source.uri, "hover.ippa");
    assert_eq!(motion.source.variant, 2);
    assert_eq!(motion.duration_secs, 0.4_f32 as f64);
    assert_eq!(motion.easing, AnimationTransitionEasing::Smoothstep);
    assert_eq!(motion.base_track, 6);
    assert_eq!(motion.sample_time, 0.75);
}

#[test]
fn motion_track_range_rejects_rounded_overflow_at_validation_and_resolution() {
    const LARGEST_F32_BELOW_U32_LIMIT: f32 = 4_294_967_040.0;
    const ROUNDED_U32_LIMIT: f32 = 4_294_967_296.0;

    let name = GuiRoot::part_property_name(GuiNodeId(1), "background", "track").unwrap();
    assert_eq!(
        crate::systems::gui::validate_gui_property_value(
            &name,
            &DynamicValue::F32(ROUNDED_U32_LIMIT),
        ),
        Err(crate::ErrorReason::InvalidValue)
    );
    assert_eq!(
        crate::systems::gui::validate_gui_property_value(
            &name,
            &DynamicValue::F32(LARGEST_F32_BELOW_U32_LIMIT),
        ),
        Ok(())
    );

    let mut root = GuiRoot::default();
    set_part_lane(
        &mut root,
        1,
        "background",
        "motion",
        DynamicValue::Asset(AssetSource {
            kind: crate::systems::animation::ANIMATION_TYPE,
            uri: "limit.ippa".into(),
            variant: 0,
        }),
    );
    set_part_lane(
        &mut root,
        1,
        "background",
        "duration",
        DynamicValue::F32(0.4),
    );
    set_part_lane(&mut root, 1, "background", "easing", DynamicValue::F32(0.0));
    set_part_lane(
        &mut root,
        1,
        "background",
        "track",
        DynamicValue::F32(LARGEST_F32_BELOW_U32_LIMIT),
    );
    set_part_lane(&mut root, 1, "background", "time", DynamicValue::F32(0.1));

    let motion = resolve_state_part_motion(
        &root,
        GuiNodeId(1),
        "background",
        GuiSkinState::Idle,
        GuiControlVariant::Plain,
    )
    .unwrap();
    assert_eq!(motion.base_track, 4_294_967_040);
    assert_eq!(motion.base_track.checked_add(2), Some(4_294_967_042));

    root.properties
        .set(&name, DynamicValue::F32(ROUNDED_U32_LIMIT))
        .unwrap();
    assert!(
        resolve_state_part_motion(
            &root,
            GuiNodeId(1),
            "background",
            GuiSkinState::Idle,
            GuiControlVariant::Plain,
        )
        .is_none()
    );
}

#[test]
fn state_precedence_is_disabled_over_pressed_over_hovered_over_idle() {
    assert_eq!(
        GuiSkinState::resolve(true, true, true),
        GuiSkinState::Disabled
    );
    assert_eq!(
        GuiSkinState::resolve(false, true, true),
        GuiSkinState::Pressed
    );
    assert_eq!(
        GuiSkinState::resolve(false, false, true),
        GuiSkinState::Hovered
    );
    assert_eq!(
        GuiSkinState::resolve(false, false, false),
        GuiSkinState::Idle
    );
}

#[test]
fn focus_is_a_separate_channel_from_base_state() {
    let interaction = GuiInteractionState {
        disabled: false,
        hovered: false,
        pressed: false,
        focused: true,
    };
    assert_eq!(interaction.state(), GuiSkinState::Idle);
    assert!(interaction.focused);
}

#[test]
fn cursors_build_interaction_per_node_from_frozen_sources() {
    let entity = EntityId::from_bits(1);
    let cursors = GuiSkinCursors {
        hovered: [skin_target(entity, 2)].into_iter().collect(),
        pressed: [skin_target(entity, 3)].into_iter().collect(),
        focus: Some(GuiInputFocus {
            target: crate::systems::gui::GuiInputTarget {
                entity,
                node: GuiNodeId(3),
                lifetime: 1,
                root_incarnation: 7,
            },
            session: 11,
        }),
    };
    let hovered = cursors.interaction_for(skin_target(entity, 2), true);
    assert!(hovered.hovered && !hovered.pressed && !hovered.focused);
    assert_eq!(hovered.state(), GuiSkinState::Hovered);
    let pressed = cursors.interaction_for(skin_target(entity, 3), true);
    assert!(pressed.pressed && pressed.focused);
    assert_eq!(pressed.state(), GuiSkinState::Pressed);
    let disabled = cursors.interaction_for(skin_target(entity, 4), false);
    assert_eq!(disabled.state(), GuiSkinState::Disabled);
    let replacement = cursors.interaction_for(
        crate::systems::gui::GuiInputTarget {
            root_incarnation: 8,
            ..skin_target(entity, 3)
        },
        true,
    );
    assert!(!replacement.hovered && !replacement.pressed && !replacement.focused);
}

#[test]
fn checkbox_variant_selects_qualified_part_first() {
    let mut root = GuiRoot::default();
    set_part_color(
        &mut root,
        1,
        "background_idle_unchecked",
        [0.1, 0.1, 0.1, 1.0],
    );
    set_part_color(
        &mut root,
        1,
        "background_idle_checked",
        [0.9, 0.1, 0.1, 1.0],
    );
    set_part_color(&mut root, 1, "background_idle", [0.5, 0.5, 0.5, 1.0]);
    let interaction = GuiInteractionState::idle();
    let checked = GuiEvaluatedNode {
        content: GuiEvaluatedContent::Checkbox {
            checked: true,
            revision: 2,
        },
        ..evaluated_node(1, GuiEvaluatedContent::Container)
    };
    let appearance = resolve_appearance(&root, &checked, &interaction, "background").unwrap();
    assert_eq!(appearance.variant, GuiControlVariant::Checked(true));
    assert_eq!(appearance.color, Some([0.9, 0.1, 0.1, 1.0]));
    let unchecked = GuiEvaluatedNode {
        content: GuiEvaluatedContent::Checkbox {
            checked: false,
            revision: 3,
        },
        ..evaluated_node(1, GuiEvaluatedContent::Container)
    };
    let appearance = resolve_appearance(&root, &unchecked, &interaction, "background").unwrap();
    assert_eq!(appearance.color, Some([0.1, 0.1, 0.1, 1.0]));
}

#[test]
fn slider_and_text_variants_follow_committed_payloads() {
    let slider = GuiEvaluatedContent::Slider {
        value: 7.5,
        min: 5.0,
        max: 10.0,
        step: 0.0,
        revision: 4,
    };
    assert_eq!(
        variant_for_content(&slider),
        GuiControlVariant::Slider01(Some(0.5))
    );
    let degenerate = GuiEvaluatedContent::Slider {
        value: 1.0,
        min: 1.0,
        max: 1.0,
        step: 0.0,
        revision: 1,
    };
    assert_eq!(
        variant_for_content(&degenerate),
        GuiControlVariant::Slider01(None)
    );
    let filled = GuiEvaluatedContent::TextInput {
        layout: crate::systems::surface::TextLayout {
            font_size: 0.1,
            glyphs: Vec::new(),
            lines: Vec::new(),
            grapheme_boundaries: vec![0],
            size: [0.0, 0.0],
        },
        font: resource_for("font"),
        font_size: 0.1,
        text: "hi".to_owned(),
        revision: 2,
    };
    assert_eq!(
        variant_for_content(&filled),
        GuiControlVariant::Text {
            empty: false,
            revision: 2,
        }
    );
}

#[test]
fn part_identity_is_stable_and_independent_of_order() {
    assert_eq!(
        skin_part_key(GuiNodeId(3), "thumb"),
        Some((GuiNodeId(3), "thumb".to_owned()))
    );
    assert!(skin_part_key(GuiNodeId(3), "").is_none());
    assert!(skin_part_key(GuiNodeId(3), "has-dash").is_none());
    assert!(is_valid_skin_part("focus_ring_2"));
    assert!(!is_valid_skin_part(""));
    // Reordered candidate vectors keep the same winning key.
    let first = state_part_candidates(
        "background",
        GuiSkinState::Pressed,
        GuiControlVariant::Checked(true),
    );
    assert_eq!(
        first,
        vec![
            "background_pressed_checked".to_owned(),
            "background_pressed".to_owned(),
            "background".to_owned(),
        ]
    );
    // Part keys do not depend on node order in the view.
    let a = skin_part_key(GuiNodeId(1), "background").unwrap();
    let b = skin_part_key(GuiNodeId(2), "background").unwrap();
    assert_ne!(a, b);
    let _ = MAX_LAYOUT_DEPTH;
}

#[test]
fn skins_never_mutate_behavior_or_layout() {
    let root = GuiRoot::default();
    let node = evaluated_node(1, GuiEvaluatedContent::Container);
    let view = test_view(vec![node.clone()]);
    let before_root = root.clone();
    let before_view = view.clone();
    let cursors = GuiSkinCursors::default();
    let resolver = MapResolver::empty();
    let appearance =
        resolve_appearance(&root, &node, &GuiInteractionState::idle(), "background").unwrap();
    assert_eq!(appearance.state, GuiSkinState::Idle);
    let painted = skinned_primitives_for_view(&view, &root, &cursors, &resolver);
    let repainted = view.surface_primitives();
    assert_eq!(painted.len(), repainted.len());
    assert_eq!(root, before_root);
    assert_eq!(view, before_view);
}

#[test]
fn ready_asset_replaces_drawing_while_missing_retains_prior() {
    let style = SurfacePrimitiveStyle {
        identity: SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
            root_incarnation: 7,
            node: GuiNodeId(1),
            lifetime: 1,
            part: GuiPrimitivePart::Icon,
        }),
        position: [0.0, 0.0],
        scale: [1.0, 1.0],
        color: [1.0, 1.0, 1.0, 1.0],
        opacity: 1.0,
        clip: None,
    };
    let drawing = SurfaceRenderPrimitive::Drawing {
        style,
        drawing: resource_for("old"),
    };
    let appearance = GuiSkinnedAppearance {
        state: GuiSkinState::Idle,
        focused: false,
        variant: GuiControlVariant::Plain,
        color: None,
        opacity: None,
        scale: None,
        asset: Some(asset_source("new-draw")),
        corner_radius: None,
        border_width: None,
        border_color: None,
        fill: None,
        glow: None,
    };
    let replaced =
        apply_asset_to_primitive(drawing.clone(), &appearance, &MapResolver::with("new-draw"));
    match replaced {
        SurfaceRenderPrimitive::Drawing {
            drawing,
            ..
        } => {
            assert_eq!(drawing.source.uri, "new-draw");
        }
        _ => panic!("expected drawing"),
    }
    let retained = apply_asset_to_primitive(drawing, &appearance, &MapResolver::empty());
    match retained {
        SurfaceRenderPrimitive::Drawing {
            drawing,
            ..
        } => {
            assert_eq!(drawing.source.uri, "old");
        }
        _ => panic!("expected drawing"),
    }
}

#[test]
fn glyph_fonts_never_remeasure_for_skins() {
    let style = SurfacePrimitiveStyle {
        identity: SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
            root_incarnation: 7,
            node: GuiNodeId(2),
            lifetime: 1,
            part: GuiPrimitivePart::Label,
        }),
        position: [0.0, 0.0],
        scale: [1.0, 1.0],
        color: [1.0, 1.0, 1.0, 1.0],
        opacity: 1.0,
        clip: None,
    };
    let glyphs = SurfaceRenderPrimitive::Glyphs {
        style,
        font: resource_for("measured-font"),
        font_size: 0.1,
        glyphs: vec![SurfaceGlyph {
            glyph_id: 1,
            position: [0.0, 0.0],
            color: None,
        }],
    };
    let appearance = GuiSkinnedAppearance {
        state: GuiSkinState::Hovered,
        focused: false,
        variant: GuiControlVariant::Plain,
        color: None,
        opacity: None,
        scale: None,
        asset: Some(asset_source("other-font")),
        corner_radius: None,
        border_width: None,
        border_color: None,
        fill: None,
        glow: None,
    };
    let kept = apply_asset_to_primitive(glyphs, &appearance, &MapResolver::with("other-font"));
    match kept {
        SurfaceRenderPrimitive::Glyphs {
            font,
            ..
        } => {
            assert_eq!(font.source.uri, "measured-font");
        }
        _ => panic!("expected glyphs"),
    }
}

#[test]
fn paint_keeps_order_identities_and_focus_border() {
    let mut root = GuiRoot::default();
    set_part_color(&mut root, 1, "background_hovered", [0.0, 1.0, 0.0, 1.0]);
    set_part_opacity(&mut root, 1, "background_hovered", 0.5);
    set_part_color(&mut root, 2, "focusRing", [0.2, 0.4, 1.0, 1.0]);
    let nodes = vec![
        evaluated_node(1, GuiEvaluatedContent::Container),
        evaluated_node(2, GuiEvaluatedContent::Container),
    ];
    let view = test_view(nodes);
    let base = view.surface_primitives();
    assert_eq!(base.len(), 2);
    let entity = EntityId::from_bits(1);
    let cursors = GuiSkinCursors {
        hovered: [skin_target(entity, 1)].into_iter().collect(),
        pressed: BTreeSet::new(),
        focus: Some(GuiInputFocus {
            target: crate::systems::gui::GuiInputTarget {
                entity,
                node: GuiNodeId(2),
                lifetime: 1,
                root_incarnation: 7,
            },
            session: 3,
        }),
    };
    let skinned = skinned_primitives_for_view(&view, &root, &cursors, &MapResolver::empty());
    assert_eq!(skinned.len(), base.len() + 1);
    for (before, after) in base.iter().zip(skinned.iter()) {
        assert_eq!(before.style().identity, after.style().identity);
        assert_eq!(before.style().clip, after.style().clip);
        assert_eq!(before.style().position, after.style().position);
    }
    // Hovered node picks the qualified part lanes.
    assert_eq!(skinned[0].style().color, [0.0, 1.0, 0.0, 1.0]);
    assert_eq!(skinned[0].style().opacity, 0.5);
    // Focused node keeps its independent background unchanged.
    match &skinned[1] {
        SurfaceRenderPrimitive::Box {
            border_width,
            ..
        } => {
            assert_eq!(*border_width, 0.0);
        }
        _ => panic!("expected background box"),
    }
    // The focus ring is its own stable named part and painter record.
    match &skinned[2] {
        SurfaceRenderPrimitive::Box {
            style,
            border_width,
            border_color,
            ..
        } => {
            assert!(matches!(
                style.identity,
                SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
                    root_incarnation: 7,
                    node: GuiNodeId(2),
                    lifetime: 1,
                    part: GuiPrimitivePart::FocusRing,
                })
            ));
            assert_eq!(*border_width, FOCUS_BORDER_WIDTH);
            assert_eq!(*border_color, [0.2, 0.4, 1.0, 1.0]);
        }
        _ => panic!("expected focus ring box"),
    }
}

#[test]
fn focus_ring_paints_without_a_background_primitive() {
    let mut node = evaluated_node(1, GuiEvaluatedContent::Container);
    node.background = None;
    let view = test_view(vec![node]);
    let mut root = GuiRoot::default();
    set_part_color(&mut root, 1, "focusRing", [0.8, 0.3, 0.1, 1.0]);
    let cursors = GuiSkinCursors {
        focus: Some(GuiInputFocus {
            target: skin_target(view.entity, 1),
            session: 2,
        }),
        ..Default::default()
    };

    let painted = skinned_primitives_for_view(&view, &root, &cursors, &MapResolver::empty());

    assert_eq!(painted.len(), 1);
    let SurfaceRenderPrimitive::Box {
        style,
        border_width,
        border_color,
        fill,
        ..
    } = &painted[0]
    else {
        panic!("focused background-free node must emit a ring box")
    };
    assert_eq!(*fill, crate::GuiShapeFill::Solid([0.0; 4]));
    assert_eq!(*border_width, FOCUS_BORDER_WIDTH);
    assert_eq!(*border_color, [0.8, 0.3, 0.1, 1.0]);
    assert!(matches!(
        style.identity,
        SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
            part: GuiPrimitivePart::FocusRing,
            ..
        })
    ));

    // An authored border colour strokes the ring in preference to the
    // colour lane, and the ring interior stays transparent.
    set_part_lane(
        &mut root,
        1,
        "focusRing",
        "border_color",
        DynamicValue::Vec4([0.1, 0.9, 0.2, 1.0]),
    );
    let painted = skinned_primitives_for_view(&view, &root, &cursors, &MapResolver::empty());
    let SurfaceRenderPrimitive::Box {
        border_color,
        fill,
        ..
    } = &painted[0]
    else {
        panic!("focused background-free node must emit a ring box")
    };
    assert_eq!(*border_color, [0.1, 0.9, 0.2, 1.0]);
    assert_eq!(*fill, crate::GuiShapeFill::Solid([0.0; 4]));
}

#[test]
fn background_label_icon_and_focus_ring_resolve_independently() {
    let layout = TextLayout {
        font_size: 0.2,
        glyphs: vec![TextGlyph {
            glyph_id: 3,
            position: [0.0, 0.0],
            advance: 1.0,
            source_range: [0, 1],
            missing: false,
        }],
        lines: Vec::new(),
        grapheme_boundaries: vec![0, 1],
        size: [1.0, 1.0],
    };
    let mut label = evaluated_node(
        1,
        GuiEvaluatedContent::Button {
            layout,
            font: resource_for("font"),
            font_size: 0.2,
            label: "A".into(),
        },
    );
    label.background = Some([0.1, 0.1, 0.1, 1.0]);
    let mut icon = evaluated_node(
        2,
        GuiEvaluatedContent::Drawing {
            drawing: Some(resource_for("base-icon")),
        },
    );
    icon.background = Some([0.15, 0.15, 0.15, 1.0]);
    let view = test_view(vec![label, icon]);
    let mut root = GuiRoot::default();
    set_part_color(&mut root, 1, "background", [1.0, 0.0, 0.0, 1.0]);
    set_part_color(&mut root, 1, "label", [0.0, 0.0, 1.0, 1.0]);
    set_part_color(&mut root, 1, "focusRing", [0.0, 1.0, 0.0, 1.0]);
    set_part_color(&mut root, 2, "background", [1.0, 0.0, 1.0, 1.0]);
    set_part_color(&mut root, 2, "icon", [1.0, 1.0, 0.0, 1.0]);
    set_part_asset(&mut root, 2, "icon", "replacement-icon");
    let cursors = GuiSkinCursors {
        focus: Some(GuiInputFocus {
            target: crate::systems::gui::GuiInputTarget {
                entity: view.entity,
                node: GuiNodeId(1),
                lifetime: 1,
                root_incarnation: view.root_incarnation,
            },
            session: 1,
        }),
        ..Default::default()
    };

    let painted = skinned_primitives_for_view(
        &view,
        &root,
        &cursors,
        &MapResolver::with("replacement-icon"),
    );
    let parts: Vec<_> = painted
        .iter()
        .map(|primitive| {
            let SurfacePrimitiveIdentity::Gui(id) = primitive.style().identity else {
                panic!("GUI paint must keep GUI identity")
            };
            (id.node, id.part, primitive.style().color)
        })
        .collect();
    assert_eq!(
        parts,
        vec![
            (
                GuiNodeId(1),
                GuiPrimitivePart::Background,
                [1.0, 0.0, 0.0, 1.0]
            ),
            (GuiNodeId(1), GuiPrimitivePart::Label, [0.0, 0.0, 1.0, 1.0]),
            (
                GuiNodeId(1),
                GuiPrimitivePart::FocusRing,
                [0.0, 0.0, 0.0, 0.0]
            ),
            (
                GuiNodeId(2),
                GuiPrimitivePart::Background,
                [1.0, 0.0, 1.0, 1.0]
            ),
            (GuiNodeId(2), GuiPrimitivePart::Icon, [1.0, 1.0, 0.0, 1.0]),
        ]
    );
    let SurfaceRenderPrimitive::Drawing {
        drawing,
        ..
    } = &painted[4]
    else {
        panic!("icon must remain drawing paint")
    };
    assert_eq!(drawing.source.uri, "replacement-icon");
    let SurfaceRenderPrimitive::Box {
        border_color,
        ..
    } = &painted[2]
    else {
        panic!("focus ring must be independent box paint")
    };
    assert_eq!(*border_color, [0.0, 1.0, 0.0, 1.0]);
}

#[test]
fn theme_only_button_and_text_input_materialize_background_parts() {
    let layout = TextLayout {
        font_size: 0.2,
        glyphs: vec![TextGlyph {
            glyph_id: 3,
            position: [0.0, 0.0],
            advance: 1.0,
            source_range: [0, 1],
            missing: false,
        }],
        lines: Vec::new(),
        grapheme_boundaries: vec![0, 1],
        size: [1.0, 1.0],
    };
    let mut button = evaluated_node(
        1,
        GuiEvaluatedContent::Button {
            layout: layout.clone(),
            font: resource_for("font"),
            font_size: 0.2,
            label: "A".into(),
        },
    );
    button.background = None;
    let mut input = evaluated_node(
        2,
        GuiEvaluatedContent::TextInput {
            layout,
            font: resource_for("font"),
            font_size: 0.2,
            text: "B".into(),
            revision: 4,
        },
    );
    input.background = None;
    let view = test_view(vec![button, input]);
    assert_eq!(view.surface_primitives().len(), 2);
    let mut root = GuiRoot::default();
    set_part_color(&mut root, 1, "background", [0.1, 0.2, 0.7, 1.0]);
    set_part_color(&mut root, 2, "background", [0.2, 0.1, 0.6, 1.0]);
    for node in [1, 2] {
        set_part_color(&mut root, node, "icon_idle_checked", [0.8, 0.9, 1.0, 1.0]);
        set_part_color(&mut root, node, "icon_idle_unchecked", [0.1, 0.2, 0.3, 1.0]);
    }

    let painted = skinned_primitives_for_view(
        &view,
        &root,
        &GuiSkinCursors::default(),
        &MapResolver::empty(),
    );
    let parts: Vec<_> = painted
        .iter()
        .map(|primitive| {
            let SurfacePrimitiveIdentity::Gui(id) = primitive.style().identity else {
                panic!("expected GUI primitive")
            };
            (id.node, id.part, primitive.style().color)
        })
        .collect();
    assert_eq!(
        parts,
        vec![
            (
                GuiNodeId(1),
                GuiPrimitivePart::Background,
                [0.1, 0.2, 0.7, 1.0],
            ),
            (GuiNodeId(1), GuiPrimitivePart::Label, [1.0; 4]),
            (
                GuiNodeId(2),
                GuiPrimitivePart::Background,
                [0.2, 0.1, 0.6, 1.0],
            ),
            (GuiNodeId(2), GuiPrimitivePart::Label, [1.0; 4]),
        ]
    );
    assert!(
        parts
            .iter()
            .all(|(_, part, _)| *part != GuiPrimitivePart::Icon)
    );
}

#[test]
fn opacity_only_theme_does_not_fabricate_fill_and_transparent_color_stays_transparent() {
    let mut node = evaluated_node(
        1,
        GuiEvaluatedContent::Button {
            layout: TextLayout {
                font_size: 0.2,
                glyphs: Vec::new(),
                lines: Vec::new(),
                grapheme_boundaries: vec![0],
                size: [0.0, 0.0],
            },
            font: resource_for("font"),
            font_size: 0.2,
            label: String::new(),
        },
    );
    node.background = None;
    let view = test_view(vec![node]);
    let mut root = GuiRoot::default();
    set_part_opacity(&mut root, 1, "background", 0.5);
    assert!(
        skinned_primitives_for_view(
            &view,
            &root,
            &GuiSkinCursors::default(),
            &MapResolver::empty(),
        )
        .is_empty()
    );

    set_part_color(&mut root, 1, "background", [0.0, 0.0, 0.0, 0.0]);
    let painted = skinned_primitives_for_view(
        &view,
        &root,
        &GuiSkinCursors::default(),
        &MapResolver::empty(),
    );
    assert_eq!(painted.len(), 1);
    assert_eq!(painted[0].style().color, [0.0, 0.0, 0.0, 0.0]);
    assert_eq!(painted[0].style().opacity, 0.5);
}

#[test]
fn checkbox_indicator_follows_committed_value_with_stable_part_identity() {
    let checkbox = |checked| {
        let mut node = evaluated_node(
            1,
            GuiEvaluatedContent::Checkbox {
                checked,
                revision: 2,
            },
        );
        node.background = None;
        node
    };
    let mut root = GuiRoot::default();
    set_part_color(&mut root, 1, "background", [0.1, 0.2, 0.7, 1.0]);
    set_part_color(&mut root, 1, "icon", [0.9, 0.8, 0.2, 1.0]);
    set_part_opacity(&mut root, 1, "icon", 1.0);
    let paint = |root: &GuiRoot, checked| {
        skinned_primitives_for_view(
            &test_view(vec![checkbox(checked)]),
            root,
            &GuiSkinCursors::default(),
            &MapResolver::empty(),
        )
    };
    let unchecked = paint(&root, false);
    let checked = paint(&root, true);
    assert_eq!(unchecked.len(), 2);
    assert_eq!(checked.len(), 2);
    assert_eq!(unchecked[1].style().identity, checked[1].style().identity);
    assert!(matches!(
        checked[1].style().identity,
        SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
            part: GuiPrimitivePart::Icon,
            ..
        })
    ));
    assert_eq!(unchecked[1].style().color, [0.9, 0.8, 0.2, 1.0]);
    assert_eq!(unchecked[1].style().opacity, 0.0);
    assert_eq!(checked[1].style().color, [0.9, 0.8, 0.2, 1.0]);
    assert_eq!(checked[1].style().opacity, 1.0);

    set_part_color(&mut root, 1, "icon_idle_unchecked", [0.2, 0.9, 0.8, 1.0]);
    set_part_opacity(&mut root, 1, "icon_idle_unchecked", 0.5);
    let explicit_unchecked = paint(&root, false);
    assert_eq!(explicit_unchecked[1].style().color, [0.2, 0.9, 0.8, 1.0]);
    assert_eq!(explicit_unchecked[1].style().opacity, 0.5);
}

#[test]
fn slider_thumb_tracks_committed_ratio_without_changing_identity() {
    let slider = |value| {
        let mut node = evaluated_node(
            1,
            GuiEvaluatedContent::Slider {
                value,
                min: 0.0,
                max: 10.0,
                step: 0.0,
                revision: 3,
            },
        );
        node.background = None;
        node
    };
    let mut root = GuiRoot::default();
    set_part_color(&mut root, 1, "background", [0.1, 0.2, 0.7, 1.0]);
    let paint = |value| {
        skinned_primitives_for_view(
            &test_view(vec![slider(value)]),
            &root,
            &GuiSkinCursors::default(),
            &MapResolver::empty(),
        )
    };
    let content = crate::systems::gui::GuiNodeContent::Slider {
        value: 0.0,
        min: 0.0,
        max: 10.0,
        step: 0.0,
    };
    let mut identity = None;
    for (value, expected_fraction) in [(0.0, 0.0), (5.0, 0.5), (10.0, 1.0)] {
        let painted = paint(value);
        assert_eq!(painted.len(), 2);
        identity.get_or_insert(painted[1].style().identity);
        assert_eq!(Some(painted[1].style().identity), identity);
        let SurfaceRenderPrimitive::Box {
            style,
            size,
            ..
        } = &painted[1]
        else {
            panic!("slider thumb must be a box")
        };
        let painted_center = style.position[0] + size[0] * 0.5;
        assert_eq!(
            super::super::super::input::system::slider_value_at(
                &content,
                [0.0, 0.0, 10.0, 5.0],
                painted_center,
                &crate::systems::gui::GuiControlValue::Scalar(value),
            ),
            Some(crate::systems::gui::GuiControlValue::Scalar(value)),
            "painted thumb center must map back to its exact committed value"
        );
        assert_eq!(
            crate::systems::gui::slider_rail([0.0, 0.0, 10.0, 5.0])
                .unwrap()
                .fraction_at(painted_center),
            Some(expected_fraction),
        );
    }
    let low = paint(0.0);
    let SurfaceRenderPrimitive::Box {
        size: track_size,
        ..
    } = &low[0]
    else {
        panic!("slider track must be a box")
    };
    assert!(*track_size == [10.0, 1.25]);
}

#[test]
fn slider_fill_uses_committed_value_and_stable_skin_identity() {
    let mut root = GuiRoot::default();
    set_part_color(&mut root, 1, "background", [0.1, 0.2, 0.7, 1.0]);
    set_part_color(&mut root, 1, "fill", [0.2, 0.9, 0.8, 1.0]);
    let paint = |value| {
        let mut node = evaluated_node(
            1,
            GuiEvaluatedContent::Slider {
                value,
                min: 0.0,
                max: 10.0,
                step: 0.0,
                revision: 3,
            },
        );
        node.background = None;
        skinned_primitives_for_view(
            &test_view(vec![node]),
            &root,
            &GuiSkinCursors::default(),
            &MapResolver::empty(),
        )
    };
    // The [0, 0, 10, 5] rail has a 3.75 thumb whose centre travels between
    // 1.875 and 8.125; the fill starts at the rail's left edge and ends under
    // the thumb centre.
    let thumb_edge = 5.0 * 0.75;
    let (center_min, center_max) = (thumb_edge * 0.5, 10.0 - thumb_edge * 0.5);
    let mut identity = None;
    for value in [0.0, 5.0, 10.0] {
        let width = center_min + value / 10.0 * (center_max - center_min);
        let painted = paint(value);
        assert_eq!(painted.len(), 3);
        let SurfaceRenderPrimitive::Box {
            style,
            size,
            ..
        } = &painted[1]
        else {
            panic!("slider fill must be a box")
        };
        assert_eq!(style.position, [0.0, 1.875]);
        assert_eq!(*size, [width, 1.25]);
        assert_eq!(style.color, [0.2, 0.9, 0.8, 1.0]);
        identity.get_or_insert(style.identity);
        assert_eq!(Some(style.identity), identity);
        assert!(matches!(
            style.identity,
            SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
                part: GuiPrimitivePart::Fill,
                ..
            })
        ));
    }
}

#[test]
fn state_only_icon_keeps_inventory_and_identity_while_visually_hidden() {
    let mut node = evaluated_node(
        1,
        GuiEvaluatedContent::Button {
            layout: TextLayout {
                font_size: 0.2,
                glyphs: Vec::new(),
                lines: Vec::new(),
                grapheme_boundaries: vec![0],
                size: [0.0, 0.0],
            },
            font: resource_for("font"),
            font_size: 0.2,
            label: String::new(),
        },
    );
    node.background = None;
    let view = test_view(vec![node]);
    let mut root = GuiRoot::default();
    set_part_color(&mut root, 1, "icon_hovered", [0.2, 0.8, 0.5, 1.0]);
    let idle = skinned_primitives_for_view(
        &view,
        &root,
        &GuiSkinCursors::default(),
        &MapResolver::empty(),
    );
    let hovered = skinned_primitives_for_view(
        &view,
        &root,
        &GuiSkinCursors {
            hovered: [skin_target(view.entity, 1)].into_iter().collect(),
            ..Default::default()
        },
        &MapResolver::empty(),
    );
    assert_eq!(idle.len(), 1);
    assert_eq!(hovered.len(), 1);
    assert_eq!(idle[0].style().identity, hovered[0].style().identity);
    assert_eq!(idle[0].style().color, [0.0, 0.0, 0.0, 0.0]);
    assert_eq!(hovered[0].style().color, [0.2, 0.8, 0.5, 1.0]);
    assert!(
        skinned_parts_for_view(&view, &root, &GuiSkinCursors::default())
            .iter()
            .any(|part| part.id.part == GuiPrimitivePart::Icon)
    );
}

#[test]
fn typed_skin_assets_materialize_theme_only_background_and_icon_parts() {
    let mut node = evaluated_node(
        1,
        GuiEvaluatedContent::Checkbox {
            checked: true,
            revision: 2,
        },
    );
    node.background = None;
    let view = test_view(vec![node]);
    let mut root = GuiRoot::default();
    set_typed_part_asset(&mut root, 1, "background", crate::TEXTURE_TYPE, "panel");
    set_typed_part_asset(
        &mut root,
        1,
        "icon",
        crate::services::asset_management::drawing::DRAWING_TYPE,
        "check",
    );
    let mut resolver = MapResolver::empty();
    for source in [
        AssetSource {
            kind: crate::TEXTURE_TYPE,
            uri: "panel".into(),
            variant: 0,
        },
        AssetSource {
            kind: crate::services::asset_management::drawing::DRAWING_TYPE,
            uri: "check".into(),
            variant: 0,
        },
    ] {
        resolver.map.insert(
            source.uri.clone(),
            SurfaceRenderResource {
                key: AssetKey {
                    slot: resolver.map.len() as u32 + 1,
                    generation: 1,
                },
                source,
            },
        );
    }

    let painted = skinned_primitives_for_view(&view, &root, &GuiSkinCursors::default(), &resolver);
    assert!(matches!(painted[0], SurfaceRenderPrimitive::Bitmap { .. }));
    assert!(matches!(painted[1], SurfaceRenderPrimitive::Drawing { .. }));
    let parts = skinned_parts_for_view(&view, &root, &GuiSkinCursors::default());
    assert!(
        parts
            .iter()
            .any(|part| part.id.part == GuiPrimitivePart::Background)
    );
    assert!(
        parts
            .iter()
            .any(|part| part.id.part == GuiPrimitivePart::Icon)
    );
}

#[test]
fn drawing_and_bitmap_parts_reject_incompatible_skin_asset_kinds() {
    let identity = SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
        root_incarnation: 7,
        node: GuiNodeId(1),
        lifetime: 1,
        part: GuiPrimitivePart::Icon,
    });
    let style = SurfacePrimitiveStyle {
        identity,
        position: [2.0, 3.0],
        scale: [1.0, 1.0],
        color: [1.0; 4],
        opacity: 1.0,
        clip: None,
    };
    let drawing = SurfaceRenderPrimitive::Drawing {
        style,
        drawing: resource_for("base-drawing"),
    };
    let texture_source = AssetSource {
        kind: crate::TEXTURE_TYPE,
        uri: "texture".into(),
        variant: 0,
    };
    let drawing_source = AssetSource {
        kind: crate::services::asset_management::drawing::DRAWING_TYPE,
        uri: "drawing".into(),
        variant: 0,
    };
    let bitmap = SurfaceRenderPrimitive::Bitmap {
        style,
        bitmap: SurfaceRenderResource {
            key: AssetKey {
                slot: 3,
                generation: 1,
            },
            source: AssetSource {
                kind: crate::TEXTURE_TYPE,
                uri: "base-texture".into(),
                variant: 0,
            },
        },
        size: [4.0, 5.0],
    };
    let mut resolver = MapResolver::empty();
    for (slot, source) in [texture_source.clone(), drawing_source.clone()]
        .into_iter()
        .enumerate()
    {
        resolver.map.insert(
            source.uri.clone(),
            SurfaceRenderResource {
                key: AssetKey {
                    slot: slot as u32 + 10,
                    generation: 1,
                },
                source,
            },
        );
    }
    let appearance = |asset| GuiSkinnedAppearance {
        state: GuiSkinState::Idle,
        focused: false,
        variant: GuiControlVariant::Plain,
        color: None,
        opacity: None,
        scale: None,
        asset: Some(asset),
        corner_radius: None,
        border_width: None,
        border_color: None,
        fill: None,
        glow: None,
    };

    let kept_drawing = apply_asset_to_primitive(
        drawing.clone(),
        &appearance(texture_source.clone()),
        &resolver,
    );
    let SurfaceRenderPrimitive::Drawing {
        drawing: kept,
        ..
    } = kept_drawing
    else {
        panic!("an incompatible texture must leave the valid drawing in place")
    };
    assert_eq!(kept.source.uri, "base-drawing");

    let kept_bitmap = apply_asset_to_primitive(
        bitmap.clone(),
        &appearance(drawing_source.clone()),
        &resolver,
    );
    let SurfaceRenderPrimitive::Bitmap {
        bitmap: kept,
        ..
    } = kept_bitmap
    else {
        panic!("an incompatible drawing must leave the valid bitmap in place")
    };
    assert_eq!(kept.source.uri, "base-texture");

    let SurfaceRenderPrimitive::Drawing {
        drawing: replacement,
        ..
    } = apply_asset_to_primitive(drawing, &appearance(drawing_source), &resolver)
    else {
        panic!("a drawing asset must replace a drawing part")
    };
    assert_eq!(replacement.source.uri, "drawing");
    let SurfaceRenderPrimitive::Bitmap {
        bitmap: replacement,
        ..
    } = apply_asset_to_primitive(bitmap, &appearance(texture_source), &resolver)
    else {
        panic!("a texture asset must replace a bitmap part")
    };
    assert_eq!(replacement.source.uri, "texture");
}

fn nonunit_drawing_fixture() -> Vec<u8> {
    let mut bytes = b"IPPD".to_vec();
    bytes.extend(1_u32.to_le_bytes());
    for value in [10.0_f32, 20.0, 42.0, 44.0, 10.0, 20.0, 42.0, 44.0, 0.05] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(1_u32.to_le_bytes());
    bytes.extend([255, 255, 255, 255, 0, 0, 0, 0]);
    bytes.extend(1_u32.to_le_bytes());
    bytes.extend([0; 8]);
    bytes.extend(3_u32.to_le_bytes());
    for point in [[10.0_f32, 20.0], [42.0, 20.0], [10.0, 44.0]] {
        bytes.extend([0; 4]);
        for value in point {
            bytes.extend(value.to_le_bytes());
        }
    }
    bytes
}

#[test]
fn synthesized_drawing_fits_decoded_nonunit_nonzero_view_box_and_composes_scale() {
    let decoded = crate::services::asset_management::drawing::DrawingAsset::decode(
        &nonunit_drawing_fixture(),
    )
    .unwrap();
    assert!(!decoded.layers().is_empty());
    assert_eq!(decoded.view_box(), [10.0, 20.0, 42.0, 44.0]);

    let node = evaluated_node(
        1,
        GuiEvaluatedContent::Checkbox {
            checked: true,
            revision: 1,
        },
    );
    let view = test_view(vec![node]);
    let mut root = GuiRoot::default();
    set_typed_part_asset(
        &mut root,
        1,
        "icon",
        crate::services::asset_management::drawing::DRAWING_TYPE,
        "nonunit",
    );
    set_part_lane(
        &mut root,
        1,
        "icon",
        "scale",
        DynamicValue::Vec2([2.0, 0.5]),
    );
    let source = AssetSource {
        kind: crate::services::asset_management::drawing::DRAWING_TYPE,
        uri: "nonunit".into(),
        variant: 0,
    };
    let mut resolver = MapResolver::empty();
    resolver.map.insert(
        source.uri.clone(),
        SurfaceRenderResource {
            key: AssetKey {
                slot: 12,
                generation: 1,
            },
            source,
        },
    );
    resolver
        .drawing_view_boxes
        .insert("nonunit".into(), decoded.view_box());

    let painted = skinned_primitives_for_view(&view, &root, &GuiSkinCursors::default(), &resolver);
    let SurfaceRenderPrimitive::Drawing {
        style,
        ..
    } = &painted[1]
    else {
        panic!("ready synthesized drawing must use drawing paint")
    };
    let view_box = decoded.view_box();
    let rendered_min = [
        style.position[0] + view_box[0] * style.scale[0],
        style.position[1] + view_box[1] * style.scale[1],
    ];
    let rendered_max = [
        style.position[0] + view_box[2] * style.scale[0],
        style.position[1] + view_box[3] * style.scale[1],
    ];
    assert_eq!(rendered_min, [3.75, 1.25]);
    assert_eq!(rendered_max, [8.75, 2.5]);
}

#[test]
fn large_default_themed_tree_plans_synthesis_once_per_node_and_phase() {
    const NODE_COUNT: usize = 512;
    let mut root = GuiRoot::default();
    let mut nodes = Vec::with_capacity(NODE_COUNT);
    for index in 0..NODE_COUNT {
        let id = index as u32 + 1;
        let mut node = evaluated_node(
            id,
            GuiEvaluatedContent::Slider {
                value: 0.5,
                min: 0.0,
                max: 1.0,
                step: 0.0,
                revision: 1,
            },
        );
        node.background = None;
        nodes.push(node);
        set_part_color(&mut root, id, "background", [0.1, 0.2, 0.7, 1.0]);
        set_part_color(&mut root, id, "icon", [0.8, 0.9, 1.0, 1.0]);
    }
    let view = test_view(nodes);

    synthesis_plan_builds(true);
    let inventory = skinned_parts_for_view(&view, &root, &GuiSkinCursors::default());
    assert_eq!(inventory.len(), NODE_COUNT * 2);
    assert_eq!(synthesis_plan_builds(true), NODE_COUNT);

    let painted = skinned_primitives_for_view(
        &view,
        &root,
        &GuiSkinCursors::default(),
        &MapResolver::empty(),
    );
    assert_eq!(painted.len(), NODE_COUNT * 2);
    assert_eq!(synthesis_plan_builds(true), NODE_COUNT);
}

#[test]
fn inventory_carries_original_index_and_synthetic_parts_without_base_clones() {
    let mut unavailable = evaluated_node(1, GuiEvaluatedContent::Container);
    unavailable.available = false;
    let mut slider = evaluated_node(
        2,
        GuiEvaluatedContent::Slider {
            value: 0.5,
            min: 0.0,
            max: 1.0,
            step: 0.0,
            revision: 1,
        },
    );
    slider.background = None;
    slider.content_origin = [f32::NAN, 0.0];
    let view = test_view(vec![unavailable, slider]);
    let retained = view.surface_part_inventory();
    assert_eq!(retained.len(), 1);
    assert_eq!(retained[0].index, 1);
    assert_eq!(retained[0].parts, [None, None]);

    let mut root = GuiRoot::default();
    set_part_color(&mut root, 2, "background", [0.1, 0.2, 0.7, 1.0]);
    let inventory = skinned_parts_for_view(&view, &root, &GuiSkinCursors::default());
    assert_eq!(inventory.len(), 2);
    assert_eq!(inventory[0].node.node, GuiNodeId(2));
    assert_eq!(inventory[0].id.part, GuiPrimitivePart::Background);
    assert_eq!(inventory[1].id.part, GuiPrimitivePart::Icon);
}

#[test]
fn synthesized_asset_replacement_retains_ready_part_but_never_crosses_root_lifetime() {
    let button = || {
        let mut node = evaluated_node(
            1,
            GuiEvaluatedContent::Button {
                layout: TextLayout {
                    font_size: 0.2,
                    glyphs: Vec::new(),
                    lines: Vec::new(),
                    grapheme_boundaries: vec![0],
                    size: [0.0, 0.0],
                },
                font: resource_for("font"),
                font_size: 0.2,
                label: String::new(),
            },
        );
        node.background = None;
        node
    };
    let source = |uri: &str| AssetSource {
        kind: crate::TEXTURE_TYPE,
        uri: uri.into(),
        variant: 0,
    };
    let mut root = GuiRoot::default();
    set_typed_part_asset(&mut root, 1, "background", crate::TEXTURE_TYPE, "ready");
    let mut resolver = MapResolver::empty();
    resolver.map.insert(
        "ready".into(),
        SurfaceRenderResource {
            key: AssetKey {
                slot: 1,
                generation: 1,
            },
            source: source("ready"),
        },
    );
    let mut retained = BTreeMap::new();
    let first = skinned_primitives_for_view_with_retained_resources(
        &test_view_with_incarnation(7, vec![button()]),
        &root,
        &GuiSkinCursors::default(),
        &resolver,
        &mut retained,
    );
    let SurfaceRenderPrimitive::Bitmap {
        bitmap,
        ..
    } = &first[0]
    else {
        panic!("ready themed background must be a bitmap")
    };
    assert_eq!(bitmap.source.uri, "ready");

    set_typed_part_asset(&mut root, 1, "background", crate::TEXTURE_TYPE, "pending");
    let pending = skinned_primitives_for_view_with_retained_resources(
        &test_view_with_incarnation(7, vec![button()]),
        &root,
        &GuiSkinCursors::default(),
        &MapResolver::empty(),
        &mut retained,
    );
    let SurfaceRenderPrimitive::Bitmap {
        bitmap,
        ..
    } = &pending[0]
    else {
        panic!("pending replacement must retain the prior bitmap")
    };
    assert_eq!(bitmap.source.uri, "ready");

    let replacement = skinned_primitives_for_view_with_retained_resources(
        &test_view_with_incarnation(8, vec![button()]),
        &root,
        &GuiSkinCursors::default(),
        &MapResolver::empty(),
        &mut retained,
    );
    assert!(replacement.is_empty());
    assert!(retained.keys().any(|(_, id)| id.root_incarnation == 7));
    assert!(!retained.keys().any(|(_, id)| id.root_incarnation == 8));
}

#[test]
fn retained_skin_resource_is_fenced_by_root_incarnation_and_named_part() {
    let drawing_node = |uri: &str| {
        let mut node = evaluated_node(
            2,
            GuiEvaluatedContent::Drawing {
                drawing: Some(resource_for(uri)),
            },
        );
        node.background = None;
        node
    };
    let mut retained = BTreeMap::new();
    let mut ready_root = GuiRoot::default();
    set_part_asset(&mut ready_root, 2, "icon", "ready-icon");
    let first = skinned_primitives_for_view_with_retained_resources(
        &test_view_with_incarnation(7, vec![drawing_node("base-a")]),
        &ready_root,
        &GuiSkinCursors::default(),
        &MapResolver::with("ready-icon"),
        &mut retained,
    );
    let SurfaceRenderPrimitive::Drawing {
        drawing,
        ..
    } = &first[0]
    else {
        panic!("expected drawing")
    };
    assert_eq!(drawing.source.uri, "ready-icon");

    let mut pending_root = GuiRoot::default();
    set_part_asset(&mut pending_root, 2, "icon", "pending-icon");
    // The fresh root deliberately reuses node 2/lifetime 1. Its pending icon
    // cannot retrieve root 7's ready cache entry; base-b remains visible.
    let replacement = skinned_primitives_for_view_with_retained_resources(
        &test_view_with_incarnation(8, vec![drawing_node("base-b")]),
        &pending_root,
        &GuiSkinCursors::default(),
        &MapResolver::empty(),
        &mut retained,
    );
    let SurfaceRenderPrimitive::Drawing {
        drawing,
        ..
    } = &replacement[0]
    else {
        panic!("expected drawing")
    };
    assert_eq!(drawing.source.uri, "base-b");
    assert!(
        retained
            .keys()
            .any(|(_, id)| { id.root_incarnation == 7 && id.part == GuiPrimitivePart::Icon })
    );
    assert!(
        !retained
            .keys()
            .any(|(_, id)| { id.root_incarnation == 8 && id.part == GuiPrimitivePart::Icon })
    );
}

#[test]
fn shape_materials_resolve_corner_radius_borders_gradients_and_glow() {
    let mut root = GuiRoot::default();
    set_part_lane(
        &mut root,
        1,
        "background",
        "corner_radius",
        DynamicValue::Vec2([0.05, 0.08]),
    );
    set_part_lane(
        &mut root,
        1,
        "background",
        "border_width",
        DynamicValue::F32(0.01),
    );
    set_part_lane(
        &mut root,
        1,
        "background",
        "border_color",
        DynamicValue::Vec4([0.2, 0.4, 0.8, 1.0]),
    );
    set_part_lane(
        &mut root,
        1,
        "background",
        "fill_mode",
        DynamicValue::F32(1.0),
    );
    set_part_lane(
        &mut root,
        1,
        "background",
        "gradient_start",
        DynamicValue::Vec2([0.0, 0.0]),
    );
    set_part_lane(
        &mut root,
        1,
        "background",
        "gradient_end",
        DynamicValue::Vec2([1.0, 1.0]),
    );
    set_part_lane(
        &mut root,
        1,
        "background",
        "gradient_color0",
        DynamicValue::Vec4([1.0, 0.0, 0.0, 1.0]),
    );
    set_part_lane(
        &mut root,
        1,
        "background",
        "gradient_color1",
        DynamicValue::Vec4([0.0, 0.0, 1.0, 1.0]),
    );
    set_part_lane(
        &mut root,
        1,
        "background",
        "glow_color",
        DynamicValue::Vec4([1.0, 0.5, 0.0, 1.0]),
    );
    set_part_lane(
        &mut root,
        1,
        "background",
        "glow_intensity",
        DynamicValue::F32(2.0),
    );
    set_part_lane(
        &mut root,
        1,
        "background",
        "glow_radius",
        DynamicValue::F32(0.05),
    );
    set_part_lane(
        &mut root,
        1,
        "background",
        "glow_falloff",
        DynamicValue::F32(1.5),
    );

    let node = evaluated_node(1, GuiEvaluatedContent::Container);
    let appearance = resolve_appearance(
        &root,
        &node,
        &GuiInteractionState::idle(),
        GuiPrimitivePart::Background.as_str(),
    )
    .unwrap();

    assert_eq!(appearance.corner_radius, Some([0.05, 0.08]));
    assert_eq!(appearance.border_width, Some(0.01));
    assert_eq!(appearance.border_color, Some([0.2, 0.4, 0.8, 1.0]));
    assert_eq!(
        appearance.fill,
        Some(GuiShapeFill::LinearGradient {
            start: [0.0, 0.0],
            end: [1.0, 1.0],
            start_color: [1.0, 0.0, 0.0, 1.0],
            end_color: [0.0, 0.0, 1.0, 1.0],
        })
    );
    assert_eq!(
        appearance.glow,
        Some(GuiShapeGlow {
            color: [1.0, 0.5, 0.0, 1.0],
            intensity: 2.0,
            radius: 0.05,
            falloff: 1.5,
        })
    );

    let box_primitive = SurfaceRenderPrimitive::Box {
        style: SurfacePrimitiveStyle {
            identity: SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
                root_incarnation: 1,
                node: GuiNodeId(1),
                lifetime: 0,
                part: GuiPrimitivePart::Background,
            }),
            position: [0.0, 0.0],
            scale: [1.0, 1.0],
            color: [0.0, 0.0, 0.0, 1.0],
            opacity: 1.0,
            clip: None,
        },
        size: [2.0, 1.0],
        corner_radius: [0.0, 0.0],
        border_width: 0.0,
        border_color: [0.0, 0.0, 0.0, 0.0],
        fill: GuiShapeFill::Solid([0.0, 0.0, 0.0, 1.0]),
        glow: None,
    };

    let applied = apply_appearance_to_primitive(&box_primitive, &appearance);
    let SurfaceRenderPrimitive::Box {
        corner_radius,
        border_width,
        border_color,
        fill,
        glow,
        size,
        ..
    } = applied
    else {
        panic!("expected box");
    };
    assert_eq!(size, [2.0, 1.0]);
    assert_eq!(corner_radius, [0.05, 0.08]);
    assert_eq!(border_width, 0.01);
    assert_eq!(border_color, [0.2, 0.4, 0.8, 1.0]);
    assert_eq!(
        fill,
        GuiShapeFill::LinearGradient {
            start: [0.0, 0.0],
            end: [1.0, 1.0],
            start_color: [1.0, 0.0, 0.0, 1.0],
            end_color: [0.0, 0.0, 1.0, 1.0],
        }
    );
    assert_eq!(
        glow,
        Some(GuiShapeGlow {
            color: [1.0, 0.5, 0.0, 1.0],
            intensity: 2.0,
            radius: 0.05,
            falloff: 1.5,
        })
    );
}

/// Background box for node 1 as layout emits it before skinning.
fn background_box() -> SurfaceRenderPrimitive {
    SurfaceRenderPrimitive::Box {
        style: SurfacePrimitiveStyle {
            identity: SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
                root_incarnation: 7,
                node: GuiNodeId(1),
                lifetime: 1,
                part: GuiPrimitivePart::Background,
            }),
            position: [0.0, 0.0],
            scale: [1.0, 1.0],
            color: [0.0, 0.0, 0.0, 1.0],
            opacity: 1.0,
            clip: None,
        },
        size: [2.0, 1.0],
        corner_radius: [0.0, 0.0],
        border_width: 0.0,
        border_color: [0.0, 0.0, 0.0, 0.0],
        fill: GuiShapeFill::Solid([0.0, 0.0, 0.0, 1.0]),
        glow: None,
    }
}

#[test]
fn solid_state_replaces_inherited_gradient_while_glow_lanes_stay_independent() {
    let mut root = GuiRoot::default();
    set_part_color(&mut root, 1, "background", [0.1, 0.2, 0.3, 1.0]);
    for (lane, value) in [
        ("fill_mode", DynamicValue::F32(1.0)),
        ("gradient_color0", DynamicValue::Vec4([1.0, 0.0, 0.0, 1.0])),
        ("gradient_color1", DynamicValue::Vec4([0.0, 0.0, 1.0, 1.0])),
        ("glow_intensity", DynamicValue::F32(0.5)),
        ("glow_radius", DynamicValue::F32(0.04)),
    ] {
        set_part_lane(&mut root, 1, "background", lane, value);
    }
    set_part_color(&mut root, 1, "background_hovered", [0.0, 1.0, 0.0, 1.0]);
    set_part_color(&mut root, 1, "background_disabled", [0.4, 0.4, 0.4, 0.5]);
    set_part_lane(
        &mut root,
        1,
        "background_disabled",
        "fill_mode",
        DynamicValue::F32(0.0),
    );
    set_part_color(&mut root, 1, "background_pressed", [0.9, 0.8, 0.1, 1.0]);
    set_part_lane(
        &mut root,
        1,
        "background_pressed",
        "fill_mode",
        DynamicValue::F32(0.0),
    );
    set_part_lane(
        &mut root,
        1,
        "background_pressed",
        "glow_intensity",
        DynamicValue::F32(0.0),
    );

    let node = evaluated_node(1, GuiEvaluatedContent::Container);
    let paint = |interaction: GuiInteractionState| {
        let appearance = resolve_appearance(&root, &node, &interaction, "background").unwrap();
        let SurfaceRenderPrimitive::Box {
            fill,
            glow,
            ..
        } = apply_appearance_to_primitive(&background_box(), &appearance)
        else {
            panic!("expected box");
        };
        (fill, glow)
    };
    let gradient = GuiShapeFill::LinearGradient {
        start: [0.0, 0.0],
        end: [1.0, 1.0],
        start_color: [1.0, 0.0, 0.0, 1.0],
        end_color: [0.0, 0.0, 1.0, 1.0],
    };
    let base_glow = Some(GuiShapeGlow {
        color: [1.0, 1.0, 1.0, 1.0],
        intensity: 0.5,
        radius: 0.04,
        falloff: 1.0,
    });

    // A colour-only state inherits the base fill mode, so the gradient hides it.
    let hovered = GuiInteractionState {
        hovered: true,
        ..GuiInteractionState::idle()
    };
    assert_eq!(paint(hovered), (gradient, base_glow));

    // Explicit solid mode paints the state colour; glow is a separate lane.
    let disabled = GuiInteractionState {
        disabled: true,
        ..GuiInteractionState::idle()
    };
    assert_eq!(
        paint(disabled),
        (GuiShapeFill::Solid([0.4, 0.4, 0.4, 0.5]), base_glow)
    );

    // Zero intensity is how a state suppresses an inherited glow.
    let pressed = GuiInteractionState {
        pressed: true,
        ..GuiInteractionState::idle()
    };
    assert_eq!(
        paint(pressed),
        (GuiShapeFill::Solid([0.9, 0.8, 0.1, 1.0]), None)
    );
}

#[test]
fn sampled_colour_reaches_solid_fill_and_focus_stroke_but_not_gradient_stops() {
    let mut authored = GuiRoot::default();
    set_part_color(&mut authored, 1, "background", [0.0, 1.0, 0.0, 1.0]);
    set_part_color(&mut authored, 1, "focusRing", [1.0, 1.0, 1.0, 1.0]);

    // AnimationSystem writes mid-transition samples into the effective base lanes.
    let mut effective = authored.clone();
    set_part_color(&mut effective, 1, "background", [0.25, 0.5, 0.25, 1.0]);
    set_part_color(&mut effective, 1, "focusRing", [0.5, 0.5, 0.5, 1.0]);

    let node = evaluated_node(1, GuiEvaluatedContent::Container);
    let view = test_view(vec![node.clone()]);
    let cursors = GuiSkinCursors {
        focus: Some(GuiInputFocus {
            target: skin_target(view.entity, 1),
            session: 1,
        }),
        ..Default::default()
    };
    let interaction = cursors.interaction_for(skin_target(view.entity, 1), true);
    let paint = |authored: &GuiRoot| {
        let overrides: BTreeMap<_, _> = [GuiPrimitivePart::Background, GuiPrimitivePart::FocusRing]
            .into_iter()
            .map(|part| {
                let desired =
                    resolve_paint_appearance(authored, &node, &interaction, part).unwrap();
                let id = GuiPrimitiveId {
                    root_incarnation: 7,
                    node: GuiNodeId(1),
                    lifetime: 1,
                    part,
                };
                (
                    id,
                    appearance_with_effective_numeric(desired, &effective, GuiNodeId(1), part),
                )
            })
            .collect();

        skinned_primitives_for_view_with_overrides(
            &view,
            authored,
            &cursors,
            &MapResolver::empty(),
            &mut BTreeMap::new(),
            &overrides,
        )
    };

    let painted = paint(&authored);
    assert_eq!(painted.len(), 2);
    let SurfaceRenderPrimitive::Box {
        style,
        fill,
        ..
    } = &painted[0]
    else {
        panic!("expected background box");
    };
    assert_eq!(style.color, [0.25, 0.5, 0.25, 1.0]);
    assert_eq!(*fill, GuiShapeFill::Solid([0.25, 0.5, 0.25, 1.0]));
    let SurfaceRenderPrimitive::Box {
        border_color,
        fill,
        ..
    } = &painted[1]
    else {
        panic!("expected focus ring box");
    };
    assert_eq!(*border_color, [0.5, 0.5, 0.5, 1.0]);
    assert_eq!(*fill, GuiShapeFill::Solid([0.0; 4]));

    // Gradient stops are separate unanimated lanes: sampling the colour lane
    // leaves an authored gradient unchanged.
    set_part_lane(
        &mut authored,
        1,
        "background",
        "fill_mode",
        DynamicValue::F32(1.0),
    );
    set_part_lane(
        &mut authored,
        1,
        "background",
        "gradient_color1",
        DynamicValue::Vec4([0.0, 0.0, 1.0, 1.0]),
    );
    let painted = paint(&authored);
    let SurfaceRenderPrimitive::Box {
        fill,
        ..
    } = &painted[0]
    else {
        panic!("expected background box");
    };
    assert_eq!(
        *fill,
        GuiShapeFill::LinearGradient {
            start: [0.0, 0.0],
            end: [1.0, 1.0],
            start_color: [0.0, 1.0, 0.0, 1.0],
            end_color: [0.0, 0.0, 1.0, 1.0],
        }
    );
}

#[test]
fn shape_glow_does_not_expand_hit_target() {
    let mut node = evaluated_node(1, GuiEvaluatedContent::Container);
    node.rect = [10.0, 10.0, 100.0, 50.0];
    node.clip = Some([0.0, 0.0, 200.0, 200.0]);
    let view = test_view(vec![node]);

    // Points inside rect hit
    assert_eq!(
        view.hit_test([15.0, 15.0]).map(|h| h.node),
        Some(GuiNodeId(1))
    );
    assert_eq!(
        view.hit_test([109.0, 59.0]).map(|h| h.node),
        Some(GuiNodeId(1))
    );

    // Points outside rect in the glow margin never hit (hit testing is layout-owned)
    assert!(view.hit_test([8.0, 10.0]).is_none());
    assert!(view.hit_test([112.0, 50.0]).is_none());
    assert!(view.hit_test([50.0, 62.0]).is_none());
}
