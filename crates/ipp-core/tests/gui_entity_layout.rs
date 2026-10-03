//! Ordinary GUI constraint layout, alignment, visual lanes and Canvas density through
//! real headless Host frames. Expected rectangles are hand-computed from the layout
//! rules documented on `GuiLayout`; hit targets are controls because raw layout
//! entities carry no control behavior.

mod support;

use ipp_core::components::rows::Rows;
use ipp_core::components::{
    GuiBehavior, GuiCheckbox, GuiLayout, GuiScrollView, GuiSlider, GuiTextInput, Surface,
};
use ipp_core::services::asset_management::{AssetSource, font::FONT_TYPE};
use ipp_core::systems::canvas::{
    CanvasBox, CanvasDrawing, CanvasPart, CanvasPrimitive, CanvasShapeGlow, CanvasStyle, CanvasText,
};
use ipp_core::systems::gui::presentation::{GuiFont, GuiPaintPart, GuiSkin, GuiTheme};
use ipp_core::systems::gui::{GuiPartId, GuiPartProperty, GuiPrimitivePart};
use ipp_core::*;
use std::mem::offset_of;
use std::sync::Arc;
use support::gui_panel::*;
use support::selection::{ATTACHMENTS, CANVAS, SURFACE, select};

fn sized(width: f32, height: f32) -> GuiLayout {
    GuiLayout {
        width,
        height,
        ..Default::default()
    }
}

fn container(kind: u32) -> GuiLayout {
    GuiLayout {
        kind,
        ..Default::default()
    }
}

fn styled(style: CanvasStyle) -> ComponentValue {
    ComponentValue::CanvasStyle(style)
}

/// Register the minimal fixture font as an immutable client source of `world`.
fn font(host: &mut HostRuntime, world: WorldId, asset: u32) -> String {
    let source = AssetSource {
        kind: FONT_TYPE,
        uri: std::sync::Arc::<str>::from(format!("producer://{}/{}/{asset}", world.0, FONT_TYPE.0)),
        variant: 0,
    };
    host.asset_resources_mut()
        .register_client_source(world, source.clone(), support::canvas_font_bytes())
        .unwrap();
    source.uri.to_string()
}

fn text(source: &str, text: &str) -> ComponentValue {
    ComponentValue::CanvasText(CanvasText {
        text: text.into(),
        source: source.into(),
        variant: 0,
        font_size: 10.0,
    })
}

fn glyph_style(panel: &GuiPanel, entity: EntityId) -> Option<CanvasPrimitive> {
    parts(&panel.output(), entity, CanvasPart::Content)
        .into_iter()
        .find(|primitive| matches!(primitive, CanvasPrimitive::Glyphs { .. }))
}

#[test]
fn columns_stack_children_and_rows_share_leftover_space_by_flex_weight() {
    let mut column = GuiPanel::new(container(2));
    let root = column.root_entity;
    let first = column.button(root, sized(100.0, 100.0));
    let second = column.button(root, sized(200.0, 50.0));
    column.frame();
    let view = column.output();
    assert_near(&bounds(&view, first), &rect(0.0, 0.0, 100.0, 100.0));
    assert_near(&bounds(&view, second), &rect(0.0, 100.0, 200.0, 50.0));
    assert_eq!(column.layout(root).size, [400.0, 200.0]);

    // The fixed child measures first; 300 units of leftover split 1:3.
    let mut row = GuiPanel::new(container(1));
    let root = row.root_entity;
    let fixed = row.button(root, sized(100.0, 50.0));
    let one = row.button(
        root,
        GuiLayout {
            flex: 1.0,
            ..sized(-1.0, 50.0)
        },
    );
    let three = row.button(
        root,
        GuiLayout {
            flex: 3.0,
            ..sized(-1.0, 50.0)
        },
    );
    row.frame();
    let view = row.output();
    assert_near(&bounds(&view, fixed), &rect(0.0, 0.0, 100.0, 50.0));
    assert_near(&bounds(&view, one), &rect(100.0, 0.0, 75.0, 50.0));
    assert_near(&bounds(&view, three), &rect(175.0, 0.0, 225.0, 50.0));
    assert_eq!(row.layout(root).size, [400.0, 200.0]);
}

#[test]
fn row_negative_margins_and_cross_end_alignment_set_overlap_hit_order() {
    let mut panel = GuiPanel::new(container(1));
    let root = panel.root_entity;
    let first = panel.button(root, sized(100.0, 100.0));
    let spacer = panel.button(
        root,
        GuiLayout {
            flex: 1.0,
            ..sized(-1.0, 50.0)
        },
    );
    // A negative leading margin overlaps the spacer, so hit order shows which
    // sibling comes later.
    let second = panel.button(
        root,
        GuiLayout {
            kind: 3,
            margin_right: 10.0,
            margin_left: -20.0,
            ..sized(50.0, 50.0)
        },
    );
    let nested = panel.button(second, sized(25.0, 25.0));
    let tail = panel.button(
        root,
        GuiLayout {
            flex: 3.0,
            align_y: 1.0,
            ..sized(-1.0, 50.0)
        },
    );
    panel.frame();
    let view = panel.output();

    // Fixed margin boxes measure 100 and 40 first; flex shares the 260 leftover
    // 1:3. Slots then follow core order: first, spacer, second (pulled 20 left),
    // tail at the cross end of the 100-tall row.
    assert_near(&bounds(&view, first), &rect(0.0, 0.0, 100.0, 100.0));
    assert_near(&bounds(&view, spacer), &rect(100.0, 0.0, 65.0, 50.0));
    assert_near(&bounds(&view, second), &rect(145.0, 0.0, 50.0, 50.0));
    assert_near(&bounds(&view, nested), &rect(145.0, 0.0, 25.0, 25.0));
    assert_near(&bounds(&view, tail), &rect(205.0, 50.0, 195.0, 50.0));
    assert!(
        panel
            .host
            .world_mut(panel.world)
            .unwrap()
            .gui_entity_layout_diagnostics()
            .unwrap()
            .is_empty()
    );
    let order: Vec<_> = view.hits.iter().map(|hit| hit.target.entity).collect();
    assert_eq!(order, vec![first, spacer, second, nested, tail]);
    assert_eq!(hit_at(&view, [155.0, 40.0]), Some(second));
    assert_eq!(hit_at(&view, [155.0, 10.0]), Some(nested));
    assert_eq!(hit_at(&view, [120.0, 25.0]), Some(spacer));
    assert_eq!(hit_at(&view, [300.0, 75.0]), Some(tail));
}

#[test]
fn column_flex_scroll_view_keeps_its_slot_clip_and_hits_between_fixed_children() {
    let mut panel = GuiPanel::new(container(2));
    let root = panel.root_entity;
    let header = panel.button(root, sized(400.0, 50.0));
    let scroll = panel.create(
        Some(root),
        vec![
            ComponentValue::GuiScrollView(GuiScrollView::default()),
            ComponentValue::GuiLayout(GuiLayout {
                flex: 1.0,
                ..sized(400.0, -1.0)
            }),
        ],
    );
    let content = panel.node(scroll, container(2));
    let rows = [
        panel.button(content, sized(400.0, 100.0)),
        panel.button(content, sized(400.0, 100.0)),
    ];
    let footer = panel.button(root, sized(400.0, 25.0));
    panel.frame();
    let view = panel.output();

    // The flex viewport takes the 125 left after both fixed children and sits
    // between them; its content and clip move with it.
    assert_near(&bounds(&view, header), &rect(0.0, 0.0, 400.0, 50.0));
    assert_near(&bounds(&view, scroll), &rect(0.0, 50.0, 400.0, 125.0));
    assert_near(&bounds(&view, rows[0]), &rect(0.0, 50.0, 400.0, 100.0));
    assert_near(&bounds(&view, rows[1]), &rect(0.0, 150.0, 400.0, 100.0));
    assert_near(&bounds(&view, footer), &rect(0.0, 175.0, 400.0, 25.0));
    assert_near(
        &control_hit(&view, rows[1]).clip,
        &rect(0.0, 50.0, 400.0, 125.0),
    );
    let geometry = panel.scroll(scroll);
    assert_eq!(geometry.viewport, [400.0, 125.0]);
    assert_eq!(geometry.content, [400.0, 200.0]);

    // Clipped scroll content below the viewport never covers the footer.
    assert_eq!(hit_at(&view, [200.0, 190.0]), Some(footer));
    assert_eq!(hit_at(&view, [200.0, 160.0]), Some(rows[1]));
    assert_eq!(hit_at(&view, [200.0, 25.0]), Some(header));
}

/// The legacy lane diagnosed unbounded flex (`GuiLayoutDiagnostic::UnboundedFlex`).
/// Scroll content measured on its unbounded axis keeps its own padding, so
/// the end of padded content can scroll into view.
#[test]
fn padded_scroll_content_keeps_its_padding_in_the_content_extent() {
    let mut panel = GuiPanel::new(container(3));
    let root = panel.root_entity;
    let scroll = panel.create(
        Some(root),
        vec![
            ComponentValue::GuiScrollView(GuiScrollView::default()),
            ComponentValue::GuiLayout(sized(200.0, 100.0)),
        ],
    );
    let column = panel.node(
        scroll,
        GuiLayout {
            kind: 2,
            padding_top: 10.0,
            padding_bottom: 10.0,
            ..Default::default()
        },
    );
    let item = panel.button(column, sized(200.0, 150.0));
    panel.frame();

    assert_eq!(panel.layout(column).size, [200.0, 170.0]);
    assert_eq!(panel.layout(item).origin, [0.0, 10.0]);
    assert_eq!(panel.scroll(scroll).content, [200.0, 170.0]);
}

/// The ordinary lane gives the flex child zero main-axis extent and reports no
/// diagnostic; this pins today's behavior pending a decision.
#[test]
fn unbounded_flex_collapses_to_zero_extent_without_a_diagnostic() {
    let mut panel = GuiPanel::new(container(3));
    let root = panel.root_entity;
    let scroll = panel.create(
        Some(root),
        vec![
            ComponentValue::GuiScrollView(GuiScrollView::default()),
            ComponentValue::GuiLayout(sized(200.0, 100.0)),
        ],
    );
    let column = panel.node(scroll, container(2));
    let flexed = panel.node(
        column,
        GuiLayout {
            flex: 1.0,
            ..Default::default()
        },
    );
    panel.frame();

    assert_eq!(panel.layout(scroll).size, [200.0, 100.0]);
    assert_eq!(panel.layout(flexed).size, [0.0, 0.0]);
    assert_eq!(panel.layout(column).size, [200.0, 0.0]);
    assert_eq!(panel.scroll(scroll).content, [200.0, 0.0]);
    assert!(
        panel
            .host
            .world_mut(panel.world)
            .unwrap()
            .gui_entity_layout_diagnostics()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn min_max_clamp_explicit_sizes_and_padding_containers_offset_their_child() {
    let mut panel = GuiPanel::new(container(3));
    let root = panel.root_entity;
    let clamped = panel.node(
        root,
        GuiLayout {
            max_width: 200.0,
            max_height: 100.0,
            ..sized(500.0, 500.0)
        },
    );
    let raised = panel.node(
        root,
        GuiLayout {
            min_width: 30.0,
            min_height: 40.0,
            ..sized(10.0, 10.0)
        },
    );
    let padding = panel.node(
        root,
        GuiLayout {
            kind: 4,
            padding_top: 50.0,
            padding_left: 25.0,
            ..Default::default()
        },
    );
    let child = panel.button(padding, sized(100.0, 100.0));
    panel.frame();

    assert_eq!(panel.layout(clamped).size, [200.0, 100.0]);
    assert_eq!(panel.layout(raised).size, [30.0, 40.0]);
    // The padding container fills its bounded constraints.
    assert_eq!(panel.layout(padding).size, [400.0, 200.0]);
    assert_eq!(panel.layout(child).origin, [25.0, 50.0]);
    assert_near(
        &bounds(&panel.output(), child),
        &rect(25.0, 50.0, 100.0, 100.0),
    );
}

#[test]
fn stack_later_siblings_win_hits_and_margins_place_descendants_within_clips() {
    let mut panel = GuiPanel::new(container(3));
    let root = panel.root_entity;
    let first = panel.button(root, sized(100.0, 100.0));
    let second = panel.button(root, sized(200.0, 50.0));
    panel.frame();
    let view = panel.output();
    assert_eq!(hit_at(&view, [10.0, 10.0]), Some(second));
    assert_eq!(hit_at(&view, [50.0, 75.0]), Some(first));

    let mut panel = GuiPanel::new(container(3));
    let root = panel.root_entity;
    let offset = panel.node(
        root,
        GuiLayout {
            kind: 3,
            margin_top: 30.0,
            margin_right: -120.0,
            margin_bottom: -30.0,
            margin_left: 120.0,
            ..sized(100.0, 40.0)
        },
    );
    let child = panel.button(offset, sized(100.0, 40.0));
    let clipped = panel.button(
        root,
        GuiLayout {
            margin_right: 25.0,
            margin_left: -25.0,
            ..sized(50.0, 50.0)
        },
    );
    let inset = panel.create(
        Some(root),
        vec![
            ComponentValue::GuiButton(Default::default()),
            ComponentValue::GuiBehavior(GuiBehavior {
                enabled: false,
                ..Default::default()
            }),
            ComponentValue::GuiLayout(GuiLayout {
                margin_top: 100.0,
                margin_right: 50.0,
                margin_bottom: 50.0,
                margin_left: 50.0,
                ..sized(400.0, 200.0)
            }),
        ],
    );
    panel.frame();
    let view = panel.output();
    assert_eq!(panel.layout(offset).origin, [120.0, 30.0]);
    assert_near(&bounds(&view, child), &rect(120.0, 30.0, 100.0, 40.0));
    assert_near(&bounds(&view, clipped), &rect(-25.0, 0.0, 50.0, 50.0));
    // Explicit size never exceeds the constraints left inside the margins.
    assert_near(&bounds(&view, inset), &rect(50.0, 100.0, 300.0, 50.0));
    assert_eq!(hit_at(&view, [150.0, 50.0]), Some(child));
    assert_eq!(hit_at(&view, [10.0, 10.0]), Some(clipped));
    assert_eq!(hit_at(&view, [-10.0, 10.0]), None);
    assert!(!control_hit(&view, inset).eligible);
    assert_eq!(hit_at(&view, [200.0, 125.0]), None);
}

#[test]
fn stack_aligns_centre_and_end_children_within_their_margin_boxes() {
    let mut panel = GuiPanel::new(container(3));
    let root = panel.root_entity;
    let centred = panel.button(
        root,
        GuiLayout {
            margin_top: 20.0,
            margin_right: 30.0,
            margin_bottom: 10.0,
            margin_left: 50.0,
            align_x: 0.0,
            align_y: 1.0,
            ..sized(100.0, 40.0)
        },
    );
    let overhanging = panel.button(
        root,
        GuiLayout {
            margin_right: -20.0,
            align_x: 1.0,
            ..sized(100.0, 40.0)
        },
    );
    panel.frame();
    let view = panel.output();

    // The 180 x 70 margin box centres horizontally and ends vertically.
    assert_near(&bounds(&view, centred), &rect(160.0, 150.0, 100.0, 40.0));
    // A negative end margin shrinks the margin box to 80, so end alignment
    // places the control 20 past the Stack's right edge, still clipped by it.
    assert_near(&bounds(&view, overhanging), &rect(320.0, 0.0, 100.0, 40.0));
    assert_eq!(
        control_hit(&view, overhanging).clip,
        [0.0, 0.0, 400.0, 200.0]
    );
    assert_eq!(hit_at(&view, [410.0, 20.0]), None);
    assert_eq!(panel.layout(root).size, [400.0, 200.0]);
}

#[test]
fn zero_visual_scale_suppresses_the_hits_of_its_subtree() {
    let mut panel = GuiPanel::new(container(3));
    let root = panel.root_entity;
    let flat = panel.create(
        Some(root),
        vec![
            ComponentValue::GuiLayout(sized(100.0, 100.0)),
            styled(CanvasStyle {
                scale_x: 0.0,
                ..Default::default()
            }),
        ],
    );
    let control = panel.button(flat, sized(100.0, 100.0));
    let shape = panel.create(
        Some(flat),
        vec![
            ComponentValue::CanvasBox(CanvasBox::default()),
            styled(CanvasStyle::default()),
        ],
    );
    panel.frame();
    let view = panel.output();

    // Paint keeps the singular scale for the renderer, which draws nothing
    // for it; hits are ineligible, so nothing in the subtree can be targeted.
    assert!(!control_hit(&view, control).eligible);
    assert_eq!(hit_at(&view, [0.0, 10.0]), None);
    let content = parts(&view, shape, CanvasPart::Content);
    assert_eq!(content[0].style().scale[0], 0.0);
}

#[test]
fn scroll_views_clip_content_from_paint_and_hits_in_final_coordinates() {
    for scale in [[1.0, 1.0], [2.0, 0.5]] {
        let mut panel = GuiPanel::new(container(3));
        let root = panel.root_entity;
        let scroll = panel.create(
            Some(root),
            vec![
                ComponentValue::GuiScrollView(GuiScrollView::default()),
                ComponentValue::GuiLayout(sized(200.0, 100.0)),
                styled(CanvasStyle {
                    scale_x: scale[0],
                    scale_y: scale[1],
                    ..Default::default()
                }),
            ],
        );
        let column = panel.node(scroll, container(2));
        let items: Vec<_> = (0..3)
            .map(|_| panel.button(column, sized(200.0, 100.0)))
            .collect();
        panel.frame();
        let view = panel.output();
        let viewport = rect(0.0, 0.0, 200.0 * scale[0], 100.0 * scale[1]);
        assert_near(&bounds(&view, scroll), &viewport);
        assert_eq!(panel.scroll(scroll).content, [200.0, 300.0]);

        // Content below the viewport keeps its rectangles but carries the
        // viewport clip, so paint and hit testing agree on visibility.
        assert_near(
            &bounds(&view, items[2]),
            &rect(0.0, 200.0 * scale[1], 200.0 * scale[0], 100.0 * scale[1]),
        );
        for &item in &items {
            assert_near(&control_hit(&view, item).clip, &viewport);
            for primitive in parts(&view, item, CanvasPart::Background) {
                assert_near(&primitive.style().clip, &viewport);
            }
        }
        assert_eq!(hit_at(&view, [100.0, 250.0 * scale[1]]), None);
        assert_eq!(hit_at(&view, [100.0, 50.0 * scale[1]]), Some(items[0]));
    }
}

#[test]
fn accumulated_nested_scale_applies_to_extents_glyphs_drawings_and_hits() {
    let mut panel = GuiPanel::new(container(3));
    host_drawing(&mut panel);
    let source = font(&mut panel.host, panel.world, 31);
    let root = panel.root_entity;
    let scaled = panel.create(
        Some(root),
        vec![
            ComponentValue::GuiLayout(sized(200.0, 200.0)),
            styled(CanvasStyle {
                scale_x: 2.0,
                scale_y: 0.5,
                ..Default::default()
            }),
        ],
    );
    let leaf = panel.button(scaled, sized(100.0, 100.0));
    let middle = panel.create(
        Some(root),
        vec![
            ComponentValue::GuiLayout(sized(100.0, 100.0)),
            styled(CanvasStyle {
                y: 100.0,
                scale_x: 2.0,
                ..Default::default()
            }),
        ],
    );
    let label = panel.create(
        Some(middle),
        vec![
            text(&source, "A"),
            styled(CanvasStyle {
                scale_y: 0.5,
                ..Default::default()
            }),
        ],
    );
    let drawn = panel.create(
        Some(root),
        vec![
            ComponentValue::GuiLayout(sized(100.0, 50.0)),
            styled(CanvasStyle {
                x: 100.0,
                scale_x: 3.0,
                scale_y: 2.0,
                ..Default::default()
            }),
        ],
    );
    let drawing = panel.create(
        Some(drawn),
        vec![
            ComponentValue::CanvasDrawing(CanvasDrawing {
                source: DRAWING_SOURCE.into(),
                variant: 0,
            }),
            ComponentValue::GuiLayout(sized(100.0, 50.0)),
            styled(CanvasStyle::default()),
        ],
    );
    frame_resolving(&mut panel, 16, |panel| {
        glyph_style(panel, label).is_some()
            && !parts(&panel.output(), drawing, CanvasPart::Content).is_empty()
    });
    let view = panel.output();

    // The 100 x 100 leaf inherits the chain: stretched X hits where the
    // unscaled box ended, shrunk Y falls outside where the unscaled box covered.
    assert_near(&bounds(&view, leaf), &rect(0.0, 0.0, 200.0, 50.0));
    assert_eq!(control_hit(&view, leaf).scale, [2.0, 0.5]);
    assert_eq!(hit_at(&view, [150.0, 25.0]), Some(leaf));
    assert_eq!(hit_at(&view, [50.0, 75.0]), None);

    // Glyph payload stays in unscaled local units; the style carries the
    // accumulated [2, 1] x [1, 0.5] scale.
    let CanvasPrimitive::Glyphs {
        style,
        glyphs,
        font_size,
        ..
    } = glyph_style(&panel, label).unwrap()
    else {
        unreachable!()
    };
    assert_eq!(style.scale, [2.0, 0.5]);
    assert_eq!(style.position, [0.0, 100.0]);
    assert_eq!(font_size, 10.0);
    assert_eq!(glyphs[0].position[0], 0.0);
    assert_near(&[glyphs[0].position[1]], &[8.0]);

    // Drawings carry the same accumulated scale and keep their layout box.
    let drawn_primitive = &parts(&view, drawing, CanvasPart::Content)[0];
    assert!(matches!(drawn_primitive, CanvasPrimitive::Drawing { .. }));
    assert_eq!(drawn_primitive.style().scale, [3.0, 2.0]);
    assert_eq!(drawn_primitive.style().position, [100.0, 0.0]);
    assert_eq!(panel.layout(drawing).size, [100.0, 50.0]);
}

const DRAWING_SOURCE: &str = "gui-layout-drawing:///icon.ippd";

fn host_drawing(panel: &mut GuiPanel) {
    panel
        .host
        .register_stream_resource_provider("gui-layout-drawing")
        .unwrap();
}

/// Frame, answering pending drawing requests, until `ready`.
fn frame_resolving(panel: &mut GuiPanel, limit: usize, ready: impl Fn(&GuiPanel) -> bool) {
    for _ in 0..limit {
        for request in panel.host.take_resource_requests() {
            panel
                .host
                .complete_resource(request.id, Ok(drawing_bytes()))
                .unwrap();
        }
        panel.frame();
        if ready(panel) {
            return;
        }
    }
    panic!("panel resources did not become ready within {limit} frames");
}

fn drawing_bytes() -> Vec<u8> {
    let mut bytes = b"IPPD".to_vec();
    bytes.extend(1_u32.to_le_bytes());
    for value in [10.0_f32, 20.0, 42.0, 44.0, 10.0, 20.0, 42.0, 44.0, 0.05] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(0_u32.to_le_bytes());
    bytes
}

#[test]
fn missing_drawing_keeps_its_box_and_paints_nothing_until_ready() {
    let mut panel = GuiPanel::new(container(2));
    host_drawing(&mut panel);
    let root = panel.root_entity;
    let drawing = panel.create(
        Some(root),
        vec![
            ComponentValue::CanvasDrawing(CanvasDrawing {
                source: DRAWING_SOURCE.into(),
                variant: 0,
            }),
            ComponentValue::GuiLayout(sized(100.0, 50.0)),
            styled(CanvasStyle::default()),
        ],
    );
    let after = panel.button(root, sized(50.0, 50.0));
    panel.frame();
    panel.frame();
    assert_eq!(panel.layout(drawing).size, [100.0, 50.0]);
    assert!(
        panel
            .output()
            .entries
            .iter()
            .all(|entry| { primitive(entry).style().identity.target.entity != drawing })
    );
    assert_near(
        &bounds(&panel.output(), after),
        &rect(0.0, 50.0, 50.0, 50.0),
    );

    frame_resolving(&mut panel, 16, |panel| {
        !parts(&panel.output(), drawing, CanvasPart::Content).is_empty()
    });
    let ready = parts(&panel.output(), drawing, CanvasPart::Content);
    assert!(matches!(ready[0], CanvasPrimitive::Drawing { .. }));
    assert_eq!(panel.layout(drawing).size, [100.0, 50.0]);
    assert_near(
        &bounds(&panel.output(), after),
        &rect(0.0, 50.0, 50.0, 50.0),
    );
}

#[test]
fn controls_carry_intrinsic_sizes_from_their_font_and_placeholder() {
    let mut panel = GuiPanel::new(container(2));
    let source = font(&mut panel.host, panel.world, 32);
    let root = panel.root_entity;
    apply_font(&mut panel, root, &source);
    let checkbox = panel.create(
        Some(root),
        vec![
            ComponentValue::GuiCheckbox(GuiCheckbox::default()),
            ComponentValue::GuiLayout(GuiLayout::default()),
        ],
    );
    let slider = panel.create(
        Some(root),
        vec![
            ComponentValue::GuiSlider(GuiSlider::default()),
            ComponentValue::GuiLayout(GuiLayout::default()),
        ],
    );
    // Fourteen 0.6 em glyphs outgrow the 8 em minimum of an input.
    let input = panel.create(
        Some(root),
        vec![
            ComponentValue::GuiTextInput(GuiTextInput {
                text: std::sync::Arc::<str>::default(),
                placeholder: std::sync::Arc::<str>::from("A".repeat(14)),
                ..Default::default()
            }),
            ComponentValue::GuiLayout(GuiLayout::default()),
        ],
    );
    frame_resolving(&mut panel, 16, |panel| {
        let view = panel.output();
        view.hits
            .iter()
            .any(|hit| hit.target.entity == input && hit.bounds[2] - hit.bounds[0] > 80.0)
    });

    // At 10 units per em: the 2 em box of a small control, an 8 em slider
    // 4/3 em tall for its one-em thumb, and the measured placeholder in a
    // full-size control 2.5 em tall.
    let slider_height = 40.0 / 3.0;
    assert_near(&panel.layout(checkbox).size, &[20.0, 20.0]);
    assert_near(&panel.layout(slider).size, &[80.0, slider_height]);
    assert_near(&panel.layout(input).size, &[84.0, 25.0]);
    assert_near(
        &bounds(&panel.output(), input),
        &rect(0.0, 20.0 + slider_height, 84.0, 25.0),
    );
}

fn apply_font(panel: &mut GuiPanel, entity: EntityId, source: &str) {
    panel
        .apply(vec![Command::insert_value(
            EntityRef::Handle(entity),
            ComponentValue::GuiFont(GuiFont {
                source: source.into(),
                variant: 0,
                font_size: 10.0,
            }),
        )])
        .result
        .unwrap();
}

#[test]
fn written_text_reflows_its_input_once_then_reuses_the_measurement() {
    use ipp_core::systems::gui::layout::GuiEntityLayoutWork;

    let mut panel = GuiPanel::new(container(2));
    let source = font(&mut panel.host, panel.world, 33);
    let root = panel.root_entity;
    apply_font(&mut panel, root, &source);
    let input = panel.create(
        Some(root),
        vec![
            ComponentValue::GuiTextInput(GuiTextInput {
                text: "A".repeat(15).into(),
                placeholder: std::sync::Arc::<str>::default(),
                ..Default::default()
            }),
            ComponentValue::GuiLayout(GuiLayout::default()),
        ],
    );
    frame_resolving(&mut panel, 16, |panel| {
        let view = panel.output();
        view.hits
            .iter()
            .any(|hit| hit.target.entity == input && hit.bounds[2] - hit.bounds[0] > 80.0)
    });
    panel.frame();
    assert_near(&panel.layout(input).size, &[90.0, 25.0]);
    let before = panel.output();

    // A written text replaces the stored text and reflows.
    panel.queue_set(
        input,
        ComponentValue::GUI_TEXT_INPUT,
        std::mem::offset_of!(GuiTextInput, text),
        FieldValue::String("A".repeat(20).into()),
    );
    panel.frame();
    panel.take_outcome().result.unwrap();
    assert_near(&panel.layout(input).size, &[120.0, 25.0]);
    let work = panel.work();
    assert_eq!((work.reflows, work.text_measurements), (1, 1));
    let after = panel.output();
    assert!(after.layout_revision > before.layout_revision);
    assert_eq!(
        panel.value(input),
        ControlValue::Text("A".repeat(20).into())
    );

    // Unchanged frames reuse the retained output without further work.
    panel.frame();
    assert_eq!(panel.work(), GuiEntityLayoutWork::default());
    assert!(Arc::ptr_eq(&after.entries, &panel.output().entries));
}

#[test]
fn aligned_containers_move_whole_subtrees_with_text_origins_and_hits() {
    let mut panel = GuiPanel::new(container(5));
    let source = font(&mut panel.host, panel.world, 34);
    let root = panel.root_entity;
    let column = panel.create(
        Some(root),
        vec![
            ComponentValue::GuiLayout(GuiLayout {
                kind: 2,
                ..sized(200.0, 100.0)
            }),
            styled(CanvasStyle::default()),
        ],
    );
    let block = panel.button(column, sized(100.0, 50.0));
    let label = panel.create(
        Some(column),
        vec![text(&source, "A"), styled(CanvasStyle::default())],
    );
    frame_resolving(&mut panel, 16, |panel| glyph_style(panel, label).is_some());
    let view = panel.output();

    // The 200 x 100 column centres in 400 x 200 at (100, 50); its children keep
    // their column-local slots relative to that origin.
    assert_eq!(panel.layout(column).origin, [100.0, 50.0]);
    assert_near(&bounds(&view, block), &rect(100.0, 50.0, 100.0, 50.0));
    assert_eq!(panel.layout(label).origin, [0.0, 50.0]);
    let glyphs = glyph_style(&panel, label).unwrap();
    assert_eq!(glyphs.style().position, [100.0, 100.0]);
    assert_eq!(hit_at(&view, [150.0, 75.0]), Some(block));
    assert_eq!(hit_at(&view, [150.0, 125.0]), None);

    // A visual move of the aligned column moves the text and hits without
    // remeasuring the retained glyph run.
    panel
        .set(
            column,
            ComponentValue::CANVAS_STYLE,
            offset_of!(CanvasStyle, x),
            FieldValue::F32(50.0),
        )
        .result
        .unwrap();
    panel.frame();
    let moved = panel.output();
    assert_near(&bounds(&moved, block), &rect(150.0, 50.0, 100.0, 50.0));
    let CanvasPrimitive::Glyphs {
        glyphs: before,
        ..
    } = glyphs
    else {
        unreachable!()
    };
    let Some(CanvasPrimitive::Glyphs {
        style,
        glyphs: after,
        ..
    }) = glyph_style(&panel, label)
    else {
        unreachable!()
    };
    assert_eq!(style.position, [150.0, 100.0]);
    assert!(Arc::ptr_eq(&before, &after));
    assert_eq!(panel.work().reflows, 0);
}

#[test]
fn stack_and_cross_alignment_move_nested_children() {
    let mut panel = GuiPanel::new(container(3));
    let root = panel.root_entity;
    let centred = panel.node(
        root,
        GuiLayout {
            kind: 1,
            align_x: 0.0,
            align_y: 1.0,
            ..sized(200.0, 50.0)
        },
    );
    let first = panel.button(centred, sized(50.0, 50.0));
    let second = panel.button(centred, sized(100.0, 25.0));
    let ended = panel.node(
        root,
        GuiLayout {
            kind: 3,
            margin_right: 25.0,
            margin_bottom: 25.0,
            align_x: 1.0,
            align_y: 1.0,
            ..sized(100.0, 100.0)
        },
    );
    let inner = panel.node(ended, container(5));
    let dot = panel.button(inner, sized(50.0, 50.0));
    panel.frame();
    let view = panel.output();

    // Centre x and end y: ((400 - 200) / 2, 200 - 50).
    assert_eq!(panel.layout(centred).origin, [100.0, 150.0]);
    assert_near(&bounds(&view, first), &rect(100.0, 150.0, 50.0, 50.0));
    assert_near(&bounds(&view, second), &rect(150.0, 150.0, 100.0, 25.0));
    // The 125 x 125 margin box ends at (275, 75); the nested Align centres its
    // dot by (25, 25), composing both deferred moves.
    assert_eq!(panel.layout(ended).origin, [275.0, 75.0]);
    assert_near(&bounds(&view, dot), &rect(300.0, 100.0, 50.0, 50.0));
    assert_eq!(hit_at(&view, [120.0, 170.0]), Some(first));
    assert_eq!(hit_at(&view, [200.0, 160.0]), Some(second));
    assert_eq!(hit_at(&view, [320.0, 120.0]), Some(dot));

    // Row: the tallest child sets the 150 cross extent, so the centred column
    // moves down by (150 - 50) / 2 with its leaf.
    let mut row = GuiPanel::new(container(1));
    let root = row.root_entity;
    row.node(root, sized(50.0, 150.0));
    let centred = row.node(
        root,
        GuiLayout {
            kind: 2,
            align_y: 0.0,
            ..sized(100.0, 50.0)
        },
    );
    let leaf = row.button(centred, sized(50.0, 25.0));
    row.frame();
    assert_eq!(row.layout(centred).origin, [50.0, 50.0]);
    assert_near(&bounds(&row.output(), leaf), &rect(50.0, 50.0, 50.0, 25.0));

    // Column: the widest child sets the 200 cross extent, so the end-aligned
    // row moves right by 200 - 100 with its leaf.
    let mut column = GuiPanel::new(container(2));
    let root = column.root_entity;
    column.node(root, sized(200.0, 50.0));
    let ended = column.node(
        root,
        GuiLayout {
            kind: 1,
            align_x: 1.0,
            ..sized(100.0, 50.0)
        },
    );
    let leaf = column.button(ended, sized(25.0, 25.0));
    column.frame();
    assert_eq!(column.layout(ended).origin, [100.0, 50.0]);
    assert_near(
        &bounds(&column.output(), leaf),
        &rect(100.0, 50.0, 25.0, 25.0),
    );
}

#[test]
fn scaled_and_reflected_parents_move_aligned_subtrees() {
    let mut panel = GuiPanel::new(container(3));
    let root = panel.root_entity;
    let scaled = panel.create(
        Some(root),
        vec![
            ComponentValue::GuiLayout(GuiLayout {
                kind: 5,
                ..sized(100.0, 100.0)
            }),
            styled(CanvasStyle {
                scale_x: 2.0,
                scale_y: 0.5,
                ..Default::default()
            }),
        ],
    );
    let column = panel.node(
        scaled,
        GuiLayout {
            kind: 2,
            ..sized(50.0, 50.0)
        },
    );
    let leaf = panel.button(column, sized(25.0, 25.0));
    let mirrored = panel.create(
        Some(root),
        vec![
            ComponentValue::GuiLayout(GuiLayout {
                kind: 5,
                ..sized(100.0, 100.0)
            }),
            styled(CanvasStyle {
                x: 300.0,
                y: 100.0,
                scale_x: -1.0,
                ..Default::default()
            }),
        ],
    );
    let mirrored_leaf = panel.button(mirrored, sized(50.0, 25.0));
    panel.frame();
    let view = panel.output();

    // Local centring offset (25, 25) scales by (2, 0.5) to (50, 12.5).
    assert_near(&bounds(&view, leaf), &rect(50.0, 12.5, 50.0, 12.5));
    assert_eq!(hit_at(&view, [70.0, 20.0]), Some(leaf));
    assert_eq!(hit_at(&view, [30.0, 10.0]), None);
    // The mirrored box spans x 200..300 from its origin at 300. Local centring
    // (25, 37.5) maps through scale (-1, 1) to (-25, 37.5), leaving the 50 x 25
    // leaf centred in the mirrored box.
    assert_near(
        &bounds(&view, mirrored_leaf),
        &rect(225.0, 137.5, 50.0, 25.0),
    );
    assert_eq!(control_hit(&view, mirrored_leaf).scale, [-1.0, 1.0]);
    assert_eq!(hit_at(&view, [250.0, 150.0]), Some(mirrored_leaf));
    assert_eq!(hit_at(&view, [210.0, 110.0]), None);
}

/// Stack > ScrollView (200 x 100) > Align (200 x `align_height`, centred) >
/// ScrollView (100 x 100) > content button (100 x 200).
fn nested_scroll_panel(align_height: f32) -> (GuiPanel, [EntityId; 3]) {
    let mut panel = GuiPanel::new(container(3));
    let root = panel.root_entity;
    let outer = panel.create(
        Some(root),
        vec![
            ComponentValue::GuiScrollView(GuiScrollView::default()),
            ComponentValue::GuiLayout(sized(200.0, 100.0)),
        ],
    );
    let align = panel.node(
        outer,
        GuiLayout {
            kind: 5,
            ..sized(200.0, align_height)
        },
    );
    let inner = panel.create(
        Some(align),
        vec![
            ComponentValue::GuiScrollView(GuiScrollView::default()),
            ComponentValue::GuiLayout(sized(100.0, 100.0)),
        ],
    );
    let content = panel.button(inner, sized(100.0, 200.0));
    panel.frame();
    (panel, [outer, inner, content])
}

#[test]
fn aligned_scroll_views_intersect_their_moved_viewport_with_fixed_ancestor_clips() {
    let (panel, [outer, inner, content]) = nested_scroll_panel(150.0);
    let view = panel.output();

    // Centring moves the inner viewport by (50, 25) to 50..150 x 25..125; the
    // outer viewport stays at 0..200 x 0..100.
    assert_near(&bounds(&view, inner), &rect(50.0, 25.0, 100.0, 100.0));
    // A clipping box intersects its own box too, so the ScrollView's hit clip
    // is its viewport within the fixed outer viewport.
    assert_near(&control_hit(&view, inner).clip, &[50.0, 25.0, 150.0, 100.0]);
    assert_near(&bounds(&view, content), &rect(50.0, 25.0, 100.0, 200.0));
    assert_near(
        &control_hit(&view, content).clip,
        &[50.0, 25.0, 150.0, 100.0],
    );
    for primitive in parts(&view, content, CanvasPart::Background) {
        assert_near(&primitive.style().clip, &[50.0, 25.0, 150.0, 100.0]);
    }
    assert_eq!(hit_at(&view, [100.0, 50.0]), Some(content));
    assert_eq!(hit_at(&view, [25.0, 50.0]), Some(outer));
    assert_eq!(hit_at(&view, [100.0, 110.0]), None);
}

#[test]
fn disjoint_nested_viewports_keep_an_empty_clip_for_hits_and_paint() {
    let (panel, [outer, inner, content]) = nested_scroll_panel(300.0);
    let view = panel.output();

    // Centring in 200 x 300 moves the inner viewport to y 100..200, entirely
    // below the outer 0..100 viewport. Its content keeps an explicit empty clip
    // instead of escaping unclipped, so nothing there is hittable, and an outer
    // scroll can still bring it into view.
    assert_near(&bounds(&view, inner), &rect(50.0, 100.0, 100.0, 100.0));
    let clip = control_hit(&view, content).clip;
    assert!(clip[3] <= clip[1], "{clip:?}");
    assert_eq!(hit_at(&view, [100.0, 150.0]), None);
    let paint = parts(&view, content, CanvasPart::Background);
    assert!(!paint.is_empty());
    assert!(
        paint
            .iter()
            .all(|primitive| primitive.style().clip[3] <= primitive.style().clip[1])
    );
    assert_eq!(hit_at(&view, [100.0, 50.0]), Some(outer));
}

#[test]
fn visual_lanes_move_paint_and_hits_together_without_reflow() {
    let mut panel = GuiPanel::new(container(2));
    let root = panel.root_entity;
    let control = panel.create(
        Some(root),
        vec![
            ComponentValue::GuiButton(Default::default()),
            ComponentValue::GuiLayout(sized(100.0, 100.0)),
            styled(CanvasStyle::default()),
        ],
    );
    panel.frame();
    let before = panel.output();
    for (offset, value) in [
        (offset_of!(CanvasStyle, x), 200.0),
        (offset_of!(CanvasStyle, y), 50.0),
        (offset_of!(CanvasStyle, scale_x), 2.0),
        (offset_of!(CanvasStyle, scale_y), 1.5),
    ] {
        panel
            .set(
                control,
                ComponentValue::CANVAS_STYLE,
                offset,
                FieldValue::F32(value),
            )
            .result
            .unwrap();
    }
    panel.frame();
    let after = panel.output();
    assert_near(&bounds(&after, control), &rect(200.0, 50.0, 200.0, 150.0));
    let background = &parts(&after, control, CanvasPart::Background)[0];
    assert_eq!(background.style().position, [200.0, 50.0]);
    assert_eq!(background.style().scale, [2.0, 1.5]);
    assert_eq!(hit_at(&after, [350.0, 150.0]), Some(control));
    assert_eq!(hit_at(&after, [50.0, 50.0]), None);
    assert_eq!(panel.layout(control).origin, [0.0, 0.0]);
    assert!(after.paint_revision > before.paint_revision);
    assert_eq!(panel.work().reflows, 0);
}

#[test]
fn disabled_controls_skip_hits_and_re_enable_without_reflow() {
    let mut panel = GuiPanel::new(container(2));
    let root = panel.root_entity;
    let control = panel.create(
        Some(root),
        vec![
            ComponentValue::GuiCheckbox(GuiCheckbox::default()),
            ComponentValue::GuiBehavior(GuiBehavior {
                enabled: false,
                ..Default::default()
            }),
            ComponentValue::GuiLayout(sized(100.0, 100.0)),
        ],
    );
    panel.frame();
    let disabled = panel.output();
    // The disabled control keeps its rectangle but never activates.
    assert_near(&bounds(&disabled, control), &rect(0.0, 0.0, 100.0, 100.0));
    assert!(!control_hit(&disabled, control).eligible);
    assert_eq!(hit_at(&disabled, [50.0, 50.0]), None);

    panel
        .set(
            control,
            ComponentValue::GUI_BEHAVIOR,
            offset_of!(GuiBehavior, enabled),
            FieldValue::Bool(true),
        )
        .result
        .unwrap();
    panel.frame();
    let enabled = panel.output();
    assert!(control_hit(&enabled, control).eligible);
    assert_eq!(hit_at(&enabled, [50.0, 50.0]), Some(control));
    assert_eq!(enabled.layout_revision, disabled.layout_revision);
    assert_eq!(panel.work().reflows, 0);
}

fn paint_part(identity: GuiPartId) -> GuiPaintPart {
    GuiPaintPart::keyed(identity).unwrap()
}

fn background() -> GuiPartId {
    GuiPartId::base(GuiPrimitivePart::Background)
}

fn theme(panel: &mut GuiPanel, part: GuiPaintPart) -> (EntityId, u32) {
    let mut parts = Rows::new();
    let slot = parts.push(part).unwrap();
    let entity = panel.create(
        None,
        vec![ComponentValue::GuiTheme(GuiTheme {
            parts,
            ..Default::default()
        })],
    );
    (entity, slot)
}

fn edit_part(
    panel: &mut GuiPanel,
    theme: EntityId,
    slot: u32,
    property: GuiPartProperty,
    value: DynamicValue,
) {
    panel
        .set(
            theme,
            ComponentValue::GUI_THEME,
            Rows::<GuiPaintPart>::offset(0, slot, property.index()).unwrap() as usize,
            FieldValue::Dynamic(value),
        )
        .result
        .unwrap();
}

fn background_box(panel: &GuiPanel, control: EntityId) -> CanvasPrimitive {
    parts(&panel.output(), control, CanvasPart::Background)[0].clone()
}

#[test]
fn theme_and_material_edits_repaint_referencing_controls_without_reflow() {
    let mut panel = GuiPanel::new(container(2));
    let root = panel.root_entity;
    let (referenced, slot) = theme(
        &mut panel,
        GuiPaintPart {
            color: Some([0.0, 0.0, 1.0, 1.0]),
            gradient_start: Some([0.0, 0.0]),
            gradient_end: Some([1.0, 0.0]),
            gradient_color0: Some([0.0, 0.0, 1.0, 1.0]),
            gradient_color1: Some([1.0, 1.0, 1.0, 1.0]),
            glow_color: Some([1.0, 1.0, 1.0, 1.0]),
            glow_intensity: Some(1.0),
            glow_radius: Some(4.0),
            ..paint_part(background())
        },
    );
    let (unreferenced, other) = theme(
        &mut panel,
        GuiPaintPart {
            color: Some([0.0, 1.0, 0.0, 1.0]),
            ..paint_part(background())
        },
    );
    let control = panel.create(
        Some(root),
        vec![
            ComponentValue::GuiButton(Default::default()),
            ComponentValue::GuiLayout(sized(100.0, 50.0)),
            ComponentValue::GuiSkin(GuiSkin {
                theme: referenced,
                ..Default::default()
            }),
        ],
    );
    panel.frame();
    let first = panel.output();

    // A reskin touches only the referenced theme: paint advances, layout holds.
    edit_part(
        &mut panel,
        referenced,
        slot,
        GuiPartProperty::Color,
        DynamicValue::Vec4([1.0, 0.0, 0.0, 1.0]),
    );
    panel.frame();
    let second = panel.output();
    assert!(second.paint_revision > first.paint_revision);
    assert_eq!(second.layout_revision, first.layout_revision);
    assert_eq!(panel.work().reflows, 0);

    // Editing a theme no control references leaves paint untouched.
    edit_part(
        &mut panel,
        unreferenced,
        other,
        GuiPartProperty::Color,
        DynamicValue::Vec4([0.5, 0.5, 0.5, 1.0]),
    );
    panel.frame();
    let third = panel.output();
    assert_eq!(third.paint_revision, second.paint_revision);
    assert!(Arc::ptr_eq(&third.entries, &second.entries));

    // Material fields repaint without reflow: glow intensity, the fill mode,
    // then a gradient colour of the selected linear fill.
    edit_part(
        &mut panel,
        referenced,
        slot,
        GuiPartProperty::GlowIntensity,
        DynamicValue::F32(2.5),
    );
    panel.frame();
    let glowing = panel.output();
    assert!(glowing.paint_revision > third.paint_revision);
    assert_eq!(glowing.layout_revision, first.layout_revision);
    let CanvasPrimitive::Box {
        glow: Some(CanvasShapeGlow {
            intensity,
            ..
        }),
        ..
    } = background_box(&panel, control)
    else {
        panic!("expected a glowing background")
    };
    assert_eq!(intensity, 2.5);
    let mut revision = glowing.paint_revision;
    for (property, value) in [
        (GuiPartProperty::FillMode, DynamicValue::F32(1.0)),
        (
            GuiPartProperty::GradientColor0,
            DynamicValue::Vec4([0.2, 0.4, 0.6, 1.0]),
        ),
    ] {
        edit_part(&mut panel, referenced, slot, property, value);
        panel.frame();
        let repainted = panel.output();
        assert!(repainted.paint_revision > revision, "{property:?}");
        assert_eq!(repainted.layout_revision, first.layout_revision);
        assert_eq!(panel.work().reflows, 0);
        revision = repainted.paint_revision;
    }
    let CanvasPrimitive::Box {
        fill:
            ipp_core::systems::canvas::CanvasShapeFill::LinearGradient {
                start_color,
                ..
            },
        ..
    } = background_box(&panel, control)
    else {
        panic!("expected a linear gradient background")
    };
    assert_eq!(start_color, [0.2, 0.4, 0.6, 1.0]);
    let gradient = panel.output();

    // Switching the control to the other theme repaints it.
    panel
        .set(
            control,
            ComponentValue::GUI_SKIN,
            offset_of!(GuiSkin, theme),
            FieldValue::Entity(EntityRef::Handle(unreferenced)),
        )
        .result
        .unwrap();
    panel.frame();
    let switched = panel.output();
    assert!(switched.paint_revision > gradient.paint_revision);
    assert_eq!(switched.layout_revision, first.layout_revision);
    let CanvasPrimitive::Box {
        fill,
        ..
    } = background_box(&panel, control)
    else {
        unreachable!()
    };
    assert_eq!(
        fill,
        ipp_core::systems::canvas::CanvasShapeFill::Solid([0.5, 0.5, 0.5, 1.0])
    );
}

/// Glow is paint-only (gui.md "Layout and presentation"); the legacy lane also kept
/// hit bounds at the node box. This pins the ordinary lane's matching behavior.
#[test]
fn glow_paints_beyond_a_control_while_its_hit_bounds_stay_at_the_layout_box() {
    let mut panel = GuiPanel::new(container(3));
    let root = panel.root_entity;
    let (glowing, _) = theme(
        &mut panel,
        GuiPaintPart {
            glow_color: Some([1.0, 0.5, 0.0, 1.0]),
            glow_intensity: Some(1.0),
            glow_radius: Some(20.0),
            ..paint_part(background())
        },
    );
    let control = panel.create(
        Some(root),
        vec![
            ComponentValue::GuiButton(Default::default()),
            ComponentValue::GuiLayout(GuiLayout {
                margin_top: 50.0,
                margin_left: 50.0,
                ..sized(100.0, 50.0)
            }),
            ComponentValue::GuiSkin(GuiSkin {
                theme: glowing,
                ..Default::default()
            }),
        ],
    );
    panel.frame();
    let view = panel.output();
    let CanvasPrimitive::Box {
        size,
        glow: Some(glow),
        ..
    } = background_box(&panel, control)
    else {
        panic!("expected a glowing background")
    };
    assert_eq!(size, [100.0, 50.0]);
    assert_eq!(glow.radius, 20.0);
    assert_near(&bounds(&view, control), &rect(50.0, 50.0, 100.0, 50.0));
    assert_eq!(hit_at(&view, [45.0, 75.0]), None);
    assert_eq!(hit_at(&view, [55.0, 75.0]), Some(control));
}

/// Parent World presenting a child World's Canvas through a 4 x 2 m Surface.
struct PresentedPanel {
    host: HostRuntime,
    child: WorldId,
    canvas: OutputRef,
    boxed: EntityId,
    label: EntityId,
    spacer: EntityId,
}

impl PresentedPanel {
    fn new() -> Self {
        let mut host = HostRuntime::new();
        let parent = host
            .create_world(Default::default(), &select(&[ATTACHMENTS, CANVAS, SURFACE]))
            .unwrap();
        let child = host
            .create_world(Default::default(), PANEL_SYSTEMS)
            .unwrap();
        let (_, parent_root) = canvas_root(&mut host, parent, PANEL, None);
        let (canvas, root) = canvas_root(
            &mut host,
            child,
            CanvasState {
                units_per_metre: 100.0,
                ..PANEL
            },
            Some(container(2)),
        );
        let boxed = button_in(&mut host, child, root, sized(100.0, 50.0));
        let row = create(
            &mut host,
            child,
            Some(root),
            vec![ComponentValue::GuiLayout(GuiLayout {
                kind: 1,
                padding_top: 10.0,
                padding_right: 10.0,
                padding_bottom: 10.0,
                padding_left: 10.0,
                ..Default::default()
            })],
        );
        let source = font(&mut host, child, 35);
        let label = create(
            &mut host,
            child,
            Some(row),
            vec![text(&source, "AA"), styled(CanvasStyle::default())],
        );
        let spacer = button_in(
            &mut host,
            child,
            row,
            GuiLayout {
                flex: 1.0,
                ..sized(-1.0, 20.0)
            },
        );
        let surface = Surface {
            width: 4.0,
            height: 2.0,
            ..Default::default()
        };
        create(
            &mut host,
            parent,
            Some(parent_root),
            vec![
                ComponentValue::Surface(surface),
                ComponentValue::WorldAttachment(WorldAttachment::surface(canvas)),
            ],
        );
        let mut panel = Self {
            host,
            child,
            canvas,
            boxed,
            label,
            spacer,
        };
        for _ in 0..8 {
            panel.frame(0.125);
            if parts(&panel.output(), label, CanvasPart::Content).len() == 1 {
                break;
            }
        }
        panel
    }

    fn frame(&mut self, dt: f64) {
        let result = self.host.frame(dt).unwrap();
        assert!(
            result.worlds.values().all(Result::is_ok),
            "{:?}",
            result.worlds
        );
    }

    /// Queue a density change through the Canvas System command.
    fn set_density(&mut self, value: f32) {
        self.host
            .world_mut(self.child)
            .unwrap()
            .enqueue_canvas_state_update(CanvasStateUpdate {
                extent: None,
                units_per_metre: Some(value),
            })
            .unwrap();
    }

    fn output(&self) -> ipp_core::systems::canvas::CanvasPublication {
        output(&self.host, self.canvas)
    }

    fn density(&mut self) -> f32 {
        self.host
            .world_mut(self.child)
            .unwrap()
            .canvas_state()
            .unwrap()
            .state
            .units_per_metre
    }

    /// Assert the child Canvas evaluated at `units` per metre: the root extent
    /// follows the density while authored lengths and text stay logical, and the
    /// flex spacer takes what the root width leaves after padding and the text.
    fn assert_density(&self, units: f32) {
        let view = self.output();
        assert_eq!(view.units_per_metre, units);
        assert_near(&view.logical_extent, &[4.0 * units, 2.0 * units]);
        assert_near(&bounds(&view, self.boxed), &rect(0.0, 0.0, 100.0, 50.0));
        // "AA" is 1.2 em wide and one 1.2 em line tall at 10 units per em.
        let label = &parts(&view, self.label, CanvasPart::Content)[0];
        assert_eq!(label.style().position, [10.0, 60.0]);
        assert_near(
            &bounds(&view, self.spacer),
            &rect(22.0, 60.0, 4.0 * units - 32.0, 20.0),
        );
    }
}

/// The primitive with its intersected clip cleared, for comparing everything else.
fn unclipped(mut primitive: CanvasPrimitive) -> CanvasPrimitive {
    match &mut primitive {
        CanvasPrimitive::Path {
            style,
            ..
        }
        | CanvasPrimitive::Glyphs {
            style,
            ..
        }
        | CanvasPrimitive::Drawing {
            style,
            ..
        }
        | CanvasPrimitive::Bitmap {
            style,
            ..
        }
        | CanvasPrimitive::Box {
            style,
            ..
        } => style.clip = [0.0; 4],
    }
    primitive
}

fn button_in(
    host: &mut HostRuntime,
    world: WorldId,
    parent: EntityId,
    layout: GuiLayout,
) -> EntityId {
    create(
        host,
        world,
        Some(parent),
        vec![
            ComponentValue::GuiButton(Default::default()),
            ComponentValue::GuiLayout(layout),
        ],
    )
}

#[test]
fn authored_lengths_stay_logical_across_densities() {
    let mut panel = PresentedPanel::new();
    panel.assert_density(100.0);
    let reference = panel.output();
    for units in [200.0, 50.0, 100.0] {
        panel.set_density(units);
        panel.frame(0.125);
        panel.assert_density(units);

        // Fixed children paint identically in logical units at every density,
        // clipped by the density-dependent root extent; the renderer maps logical
        // units to Surface metres once, by the published density.
        let view = panel.output();
        for (entity, part) in [
            (panel.boxed, CanvasPart::Background),
            (panel.label, CanvasPart::Content),
        ] {
            let expected = parts(&reference, entity, part);
            let actual = parts(&view, entity, part);
            assert!(!expected.is_empty());
            assert_eq!(
                actual
                    .iter()
                    .map(|primitive| primitive.style().clip)
                    .collect::<Vec<_>>(),
                vec![[0.0, 0.0, 4.0 * units, 2.0 * units]; actual.len()]
            );
            assert_eq!(
                actual.into_iter().map(unclipped).collect::<Vec<_>>(),
                expected.into_iter().map(unclipped).collect::<Vec<_>>()
            );
        }
    }
}

/// A density command reflows the presented canvas; an invalid density is
/// refused without effect: the density and the output stay.
#[test]
fn density_commands_reflow_the_presented_canvas_and_invalid_values_are_refused() {
    let mut panel = PresentedPanel::new();
    let before = panel.output();
    panel.set_density(200.0);
    panel.frame(0.125);
    panel.assert_density(200.0);
    assert!(panel.output().layout_revision > before.layout_revision);

    for units in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        let valid = panel.output();
        panel.set_density(units);
        panel.frame(0.125);
        assert_eq!(panel.density(), 200.0, "{units} applied");
        panel.assert_density(200.0);
        assert_eq!(panel.output().paint_revision, valid.paint_revision);
    }
}
