//! Which changes republish Canvas output (its paint, layout and resource revisions),
//! intersected clips and leaf validation through real headless Host frames,
//! including animation and World save/load.

mod support;
use support::task_scheduler::HostTaskTestDriver;

use ipp_core::components::rows::Rows;
use ipp_core::components::{FlatSurface, GuiBehavior, GuiLayout, GuiScrollView, SurfaceCache};
use ipp_core::services::asset_management::{
    AssetSource, AssetUpload, AssetUploadIdentity, font::FONT_TYPE,
};
use ipp_core::services::world_serialization::WorldLoadOptions;
use ipp_core::systems::animation::*;
use ipp_core::systems::canvas::{
    CanvasBitmap, CanvasBox, CanvasGlyphRow, CanvasGlyphRun, CanvasPart, CanvasPrimitive,
    CanvasPublication, CanvasStyle, CanvasText,
};
use ipp_core::systems::gui::local::GuiLocalAction;
use ipp_core::systems::surface::SurfaceCachePolicy;
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

fn stack() -> GuiLayout {
    GuiLayout {
        kind: 3,
        ..Default::default()
    }
}

fn shape(width: f32, height: f32) -> Vec<ComponentValue> {
    vec![
        ComponentValue::CanvasBox(CanvasBox {
            width,
            height,
            ..Default::default()
        }),
        ComponentValue::CanvasStyle(CanvasStyle::default()),
    ]
}

fn content(view: &CanvasPublication, entity: EntityId) -> Option<CanvasPrimitive> {
    parts(view, entity, CanvasPart::Content).into_iter().next()
}

fn revisions(view: &CanvasPublication) -> [u64; 3] {
    [
        view.layout_revision,
        view.paint_revision,
        view.resource_revision,
    ]
}

#[test]
fn optional_bounds_added_to_retained_layout_are_fresh_after_removal_and_reinsertion() {
    let mut panel = GuiPanel::new(stack());
    let node = panel.node(panel.root_entity, sized(80.0, 30.0));
    panel.frame();
    let read = |panel: &mut GuiPanel| {
        panel
            .host
            .world_mut(panel.world)
            .unwrap()
            .inspect(node)
            .unwrap()
            .components
            .into_iter()
            .find_map(|value| match value {
                ComponentValue::CanvasBounds(bounds) => {
                    Some([bounds.x, bounds.y, bounds.width, bounds.height])
                }
                _ => None,
            })
    };
    assert_eq!(read(&mut panel), None);

    // Inserting only the observer component must publish its evaluated bounds.
    panel
        .apply(vec![Command::insert_value(
            EntityRef::Handle(node),
            ComponentValue::CanvasBounds(Default::default()),
        )])
        .result
        .unwrap();
    panel.frame();
    assert_eq!(read(&mut panel), Some([0.0, 0.0, 80.0, 30.0]));
    let stable = revisions(&panel.output());
    panel.frame();
    assert_eq!(revisions(&panel.output()), stable);

    panel
        .apply(vec![Command::RemoveComponent {
            entity: EntityRef::Handle(node),
            component: ComponentValue::CANVAS_BOUNDS,
        }])
        .result
        .unwrap();
    panel
        .set(
            node,
            ComponentValue::GUI_LAYOUT,
            offset_of!(GuiLayout, width),
            FieldValue::F32(42.0),
        )
        .result
        .unwrap();
    panel.frame();
    assert_eq!(read(&mut panel), None);

    panel
        .apply(vec![Command::insert_value(
            EntityRef::Handle(node),
            ComponentValue::CanvasBounds(Default::default()),
        )])
        .result
        .unwrap();
    panel.frame();
    assert_eq!(read(&mut panel), Some([0.0, 0.0, 42.0, 30.0]));
}

#[test]
fn surface_resize_advances_the_presented_canvas_revisions_and_identical_rewrites_do_not() {
    let mut host = crate::support::task_scheduler::host();
    let parent = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, CANVAS, SURFACE]))
        .unwrap();
    let child = host.create_world(Default::default(), CANVAS).unwrap();
    let (_, parent_root) = canvas_root(&mut host, parent, PANEL, None);
    let (canvas, canvas_entity) = canvas_root(&mut host, child, PANEL, None);
    let leaf = create(&mut host, child, Some(canvas_entity), shape(20.0, 10.0));
    let surface = FlatSurface {
        width: 2.0,
        height: 1.0,
        ..Default::default()
    };
    let anchor = create(
        &mut host,
        parent,
        Some(parent_root),
        vec![
            ComponentValue::FlatSurface(surface),
            ComponentValue::WorldAttachment(WorldAttachment::surface(canvas)),
        ],
    );
    let frames = |host: &mut HostRuntime| {
        for _ in 0..2 {
            host.frame_for_test(0.125).unwrap();
        }
    };
    frames(&mut host);
    let original = output(&host, canvas);
    assert_eq!(original.logical_extent, [200.0, 100.0]);

    // A leaf tint repaints without new geometry or resources.
    apply(
        &mut host,
        child,
        vec![Command::SetField {
            entity: EntityRef::Handle(leaf),
            component: ComponentValue::CANVAS_STYLE,
            field: FieldWrite {
                offset: offset_of!(CanvasStyle, red) as u32,
                value: FieldValue::F32(0.5),
            },
        }],
    )
    .result
    .unwrap();
    frames(&mut host);
    let tinted = output(&host, canvas);
    assert!(tinted.paint_revision > original.paint_revision);
    assert_eq!(tinted.resource_revision, original.resource_revision);
    assert_eq!(tinted.layout_revision, original.layout_revision);

    // Resizing the presenting Surface changes the child extent and repaints it.
    let resize = |host: &mut HostRuntime, width: f32| {
        apply(
            host,
            parent,
            vec![Command::SetField {
                entity: EntityRef::Handle(anchor),
                component: ComponentValue::FLAT_SURFACE,
                field: FieldWrite {
                    offset: offset_of!(FlatSurface, width) as u32,
                    value: FieldValue::F32(width),
                },
            }],
        )
        .result
        .unwrap();
    };
    resize(&mut host, 2.5);
    frames(&mut host);
    let resized = output(&host, canvas);
    assert_eq!(resized.logical_extent, [250.0, 100.0]);
    assert!(resized.paint_revision > tinted.paint_revision);
    assert_eq!(resized.resource_revision, tinted.resource_revision);

    // Rewriting an identical value publishes nothing new.
    resize(&mut host, 2.5);
    frames(&mut host);
    let rewritten = output(&host, canvas);
    assert_eq!(revisions(&rewritten), revisions(&resized));
    assert_eq!(rewritten.input_revision, resized.input_revision);
    assert!(Arc::ptr_eq(&rewritten.entries, &resized.entries));
}

#[test]
fn numeric_leaf_animation_advances_the_paint_revision_until_the_clip_holds() {
    let mut panel = GuiPanel::with_canvas(PANEL, None);
    let root = panel.root_entity;
    let leaf = panel.create(Some(root), shape(20.0, 10.0));
    let target = AnimationTrackTarget::AnimationProperty(AnimationProperty {
        component: ComponentValue::CANVAS_STYLE,
        offsets: vec![offset_of!(CanvasStyle, opacity) as u32],
    });
    let key = |time, value, interpolation| AnimationKeyframe {
        time,
        value: AnimationValue::Field(ipp_core::components::schema::FieldValue::F32(value)),
        interpolation,
    };
    let clip = AnimationClip::new(
        1.0,
        vec![AnimationTrack {
            target: target.clone(),
            keys: vec![
                key(0.0, 1.0, AnimationInterpolation::Linear),
                key(1.0, 0.0, AnimationInterpolation::Step),
            ],
        }],
    )
    .unwrap();
    panel
        .host
        .world_mut(panel.world)
        .unwrap()
        .enqueue_asset(AssetUpload {
            id: 91,
            key: AssetUploadIdentity {
                kind: ANIMATION_TYPE,
                asset: 91,
                variant: 0,
            },
            bytes: clip.encode(),
        })
        .unwrap();
    for _ in 0..512 {
        let report = panel.host.frame_for_test(0.0).unwrap();
        if report.worlds[&panel.world]
            .as_ref()
            .unwrap()
            .assets
            .iter()
            .any(|asset| asset.id == 91)
        {
            break;
        }
    }
    {
        let mut context = panel.host.world_mut(panel.world).unwrap();
        let controller = context
            .create_animation_controller(AnimationControllerDescription {
                drivers: vec![AnimationDriverDescription {
                    source: std::sync::Arc::<str>::from(format!("asset://{}/91", ANIMATION_TYPE.0)),
                    variant: 0,
                    track: 0,
                    target: leaf,
                    property: target,
                    entity_bindings: Vec::new(),
                    weight: 1.0,
                    additive: false,
                    reference_time: 0.0,
                    repeat: false,
                }],
                speed: 1.0,
                ..Default::default()
            })
            .unwrap();
        context
            .control_animation_controller(controller, AnimationPlaybackControl::Play)
            .unwrap();
    }
    panel.frame_for(0.0);

    let mut previous = panel.output();
    for _ in 0..3 {
        panel.frame_for(0.25);
        let next = panel.output();
        assert!(
            next.paint_revision > previous.paint_revision,
            "each sampled opacity repaints"
        );
        assert_eq!(next.resource_revision, previous.resource_revision);
        assert_eq!(next.layout_revision, previous.layout_revision);
        assert!(
            content(&next, leaf).unwrap().style().opacity
                < content(&previous, leaf).unwrap().style().opacity
        );
        previous = next;
    }

    // The clip completes, then holds its final value.
    panel.frame_for(0.5);
    let held = panel.output();
    panel.frame_for(0.25);
    panel.frame_for(0.25);
    assert_eq!(panel.output().paint_revision, held.paint_revision);
    assert!(Arc::ptr_eq(&panel.output().entries, &held.entries));
}

#[test]
fn font_readiness_and_replacement_advance_the_resource_revision() {
    let mut panel = GuiPanel::with_canvas(PANEL, None);
    panel
        .host
        .register_stream_resource_provider("canvas-revision-fonts")
        .unwrap();
    let ready = AssetSource {
        kind: FONT_TYPE,
        uri: std::sync::Arc::<str>::from(format!(
            "producer://{}/{}/51",
            panel.world.0, FONT_TYPE.0
        )),
        variant: 0,
    };
    panel
        .host
        .asset_resources_mut()
        .register_client_source(panel.world, ready.clone(), support::canvas_font_bytes())
        .unwrap();
    let root = panel.root_entity;
    let label = panel.create(
        Some(root),
        vec![
            ComponentValue::CanvasText(CanvasText {
                text: "AA".into(),
                source: ready.uri.clone(),
                variant: 0,
                font_size: 10.0,
            }),
            ComponentValue::CanvasStyle(CanvasStyle::default()),
        ],
    );
    for _ in 0..8 {
        panel.frame();
        if content(&panel.output(), label).is_some() {
            break;
        }
    }
    let first = panel.output();
    let first_font = content(&first, label).unwrap().resource().unwrap();
    let set_source = |panel: &mut GuiPanel, source: &str| {
        panel
            .set(
                label,
                ComponentValue::CANVAS_TEXT,
                offset_of!(CanvasText, source),
                FieldValue::String(source.into()),
            )
            .result
            .unwrap();
    };

    // A pending font suppresses the label: its resource leaves the publication.
    set_source(&mut panel, "canvas-revision-fonts:///pending.ippf");
    panel.frame();
    let pending = panel.output();
    assert!(content(&pending, label).is_none());
    assert!(pending.paint_revision > first.paint_revision);
    assert!(pending.resource_revision > first.resource_revision);
    assert_eq!(pending.resources().count(), 0);

    // Readiness brings the replacement resource into the publication.
    let mut requested = false;
    for _ in 0..16 {
        for request in panel.host.take_resource_requests() {
            panel
                .host
                .complete_resource(request.id, Ok(support::canvas_font_bytes()))
                .unwrap();
            requested = true;
        }
        panel.frame();
        if content(&panel.output(), label).is_some() {
            break;
        }
    }
    assert!(requested);
    let loaded = panel.output();
    let loaded_font = content(&loaded, label).unwrap().resource().unwrap();
    assert_ne!(loaded_font, first_font);
    assert!(loaded.paint_revision > pending.paint_revision);
    assert!(loaded.resource_revision > pending.resource_revision);
    panel.frame();
    assert_eq!(revisions(&panel.output()), revisions(&loaded));

    // Returning to the first font replaces the resource identity again.
    set_source(&mut panel, &ready.uri);
    panel.frame();
    let returned = panel.output();
    assert!(returned.resource_revision > loaded.resource_revision);
    assert_eq!(
        content(&returned, label).unwrap().resource(),
        Some(first_font)
    );
    panel.frame();
    assert_eq!(panel.output().resource_revision, returned.resource_revision);
}

#[test]
fn scroll_theme_and_focus_changes_advance_the_canvas_paint_revision() {
    let mut panel = GuiPanel::new(stack());
    let root = panel.root_entity;
    let scroll = panel.create(
        Some(root),
        vec![
            ComponentValue::GuiScrollView(GuiScrollView::default()),
            ComponentValue::GuiLayout(sized(200.0, 100.0)),
        ],
    );
    panel.button(scroll, sized(200.0, 300.0));
    let control = panel.button(
        root,
        GuiLayout {
            margin_left: 250.0,
            ..sized(100.0, 50.0)
        },
    );
    panel.frame();
    let initial = panel.output();

    panel.act(scroll, GuiLocalAction::ScrollTo([0.0, 50.0]));
    panel.frame();
    let scrolled = panel.output();
    assert!(scrolled.paint_revision > initial.paint_revision);
    assert_eq!(scrolled.resource_revision, initial.resource_revision);

    // Programmatic focus paints the focus ring and asks for direct presentation.
    panel.act(control, GuiLocalAction::Focus(0));
    panel.frame();
    let focused = panel.output();
    assert!(focused.paint_revision > scrolled.paint_revision);
    assert!(!parts(&focused, control, CanvasPart::FocusRing).is_empty());
    assert!(focused.interaction.focused && focused.interaction.requires_direct());

    // Disabling the focused control clears its interaction priority.
    panel
        .apply(vec![Command::insert_value(
            EntityRef::Handle(control),
            ComponentValue::GuiBehavior(GuiBehavior {
                enabled: false,
                ..Default::default()
            }),
        )])
        .result
        .unwrap();
    panel.frame();
    let disabled = panel.output();
    assert!(!disabled.interaction.focused);
    assert!(!disabled.interaction.requires_direct());
    assert!(parts(&disabled, control, CanvasPart::FocusRing).is_empty());
    assert!(disabled.paint_revision > focused.paint_revision);
}

#[test]
fn canvas_clips_intersect_through_ancestors_and_empty_intersections_hold_no_hits() {
    let mut panel = GuiPanel::new(stack());
    let root = panel.root_entity;
    let clipped = |clip: [f32; 4]| {
        vec![
            ComponentValue::GuiLayout(GuiLayout::default()),
            ComponentValue::CanvasStyle(CanvasStyle {
                clipped: true,
                clip_min_x: clip[0],
                clip_min_y: clip[1],
                clip_max_x: clip[2],
                clip_max_y: clip[3],
                ..Default::default()
            }),
        ]
    };
    // Without a clip the root rectangle clips; a clip beyond it is intersected.
    let unclipped = panel.create(Some(root), shape(20.0, 10.0));
    let wide = panel.create(Some(root), clipped([-100.0, -100.0, 1000.0, 1000.0]));
    let within_wide = panel.create(Some(wide), shape(20.0, 10.0));
    // Nested clips intersect.
    let outer = panel.create(Some(root), clipped([100.0, 50.0, 300.0, 150.0]));
    let inner = panel.create(Some(outer), clipped([200.0, 100.0, 500.0, 300.0]));
    let nested = panel.create(Some(inner), shape(20.0, 10.0));
    let nested_control = panel.button(inner, sized(400.0, 200.0));
    // Touching, disjoint and inverted clips are empty.
    let left = panel.create(Some(root), clipped([0.0, 0.0, 100.0, 100.0]));
    let touching = panel.create(Some(left), clipped([100.0, 0.0, 200.0, 100.0]));
    let touching_control = panel.button(touching, sized(400.0, 200.0));
    let disjoint = panel.create(Some(left), clipped([200.0, 150.0, 300.0, 200.0]));
    let disjoint_control = panel.button(disjoint, sized(400.0, 200.0));
    let inverted = panel.create(Some(root), clipped([150.0, 150.0, 50.0, 50.0]));
    let inverted_control = panel.button(inverted, sized(400.0, 200.0));
    let inverted_shape = panel.create(Some(inverted), shape(20.0, 10.0));
    panel.frame();
    let view = panel.output();

    let clip_of = |entity| content(&view, entity).unwrap().style().clip;
    assert_eq!(clip_of(unclipped), [0.0, 0.0, 400.0, 200.0]);
    assert_eq!(clip_of(within_wide), [0.0, 0.0, 400.0, 200.0]);
    assert_eq!(clip_of(nested), [200.0, 100.0, 300.0, 150.0]);
    assert_eq!(
        control_hit(&view, nested_control).clip,
        [200.0, 100.0, 300.0, 150.0]
    );
    // Minimum edges are inclusive and maximum edges exclusive.
    assert_eq!(hit_at(&view, [200.0, 100.0]), Some(nested_control));
    assert_eq!(hit_at(&view, [300.0, 120.0]), None);
    assert_eq!(hit_at(&view, [250.0, 150.0]), None);

    for control in [touching_control, disjoint_control, inverted_control] {
        let hit = control_hit(&view, control);
        assert!(
            hit.clip[2] <= hit.clip[0] || hit.clip[3] <= hit.clip[1],
            "{:?}",
            hit.clip
        );
        for point in [[0.0, 0.0], [100.0, 50.0], [150.0, 150.0], [250.0, 175.0]] {
            assert!(!hit.contains(point));
        }
    }

    // Paint under an empty clip stays published with that empty clip; the
    // renderer draws nothing for it until an ancestor change reopens the clip.
    let empty = clip_of(inverted_shape);
    assert!(empty[2] <= empty[0] && empty[3] <= empty[1], "{empty:?}");

    // Non-finite clips never reach the component.
    let rejected = panel.apply(vec![Command::insert_value(
        EntityRef::Handle(left),
        ComponentValue::CanvasStyle(CanvasStyle {
            clipped: true,
            clip_max_x: f32::NAN,
            ..Default::default()
        }),
    )]);
    assert!(rejected.result.is_err());
    let rejected = panel.set(
        left,
        ComponentValue::CANVAS_STYLE,
        offset_of!(CanvasStyle, clip_max_y),
        FieldValue::F32(f32::INFINITY),
    );
    assert!(rejected.result.is_err());
    panel.frame();
    assert!(Arc::ptr_eq(&panel.output().entries, &view.entries));
}

/// Out-of-domain Canvas leaf inserts are rejected without effect. A zero-size `CanvasBitmap` is
/// accepted today (the legacy Surface bitmap item rejected it with `InvalidField`);
/// zero-size `CanvasBox` extents are accepted as well.
#[test]
fn canvas_leaf_inserts_and_live_writes_reject_out_of_domain_values_without_effect() {
    let mut panel = GuiPanel::with_canvas(PANEL, None);
    let root = panel.root_entity;
    let leaf = panel.create(
        Some(root),
        vec![
            ComponentValue::CanvasBox(CanvasBox::default()),
            ComponentValue::CanvasStyle(CanvasStyle {
                x: 5.0,
                opacity: 0.75,
                ..Default::default()
            }),
        ],
    );
    panel.frame();
    let before = panel.output();
    let insert =
        |value: ComponentValue| vec![Command::insert_value(EntityRef::Handle(leaf), value)];

    let mut invalid = Vec::new();
    for (width, radius) in [
        (-1.0, 0.0),
        (f32::NAN, 0.0),
        (1.0, -0.1),
        (1.0, f32::INFINITY),
    ] {
        invalid.push(ComponentValue::CanvasBox(CanvasBox {
            width,
            radius_x: radius,
            ..Default::default()
        }));
    }
    for style in [
        CanvasStyle {
            x: 10.0,
            opacity: 1.5,
            ..Default::default()
        },
        CanvasStyle {
            x: 10.0,
            opacity: -0.1,
            ..Default::default()
        },
        CanvasStyle {
            x: 10.0,
            red: 2.0,
            ..Default::default()
        },
        CanvasStyle {
            x: 10.0,
            alpha: f32::NAN,
            ..Default::default()
        },
    ] {
        invalid.push(ComponentValue::CanvasStyle(style));
    }
    for glyph in [
        CanvasGlyphRow {
            glyph_id: 1,
            position: [0.0, 0.0],
            color: Some([-0.1, 0.0, 0.0, 1.0]),
        },
        CanvasGlyphRow {
            glyph_id: 1,
            position: [f32::NAN, 0.0],
            color: None,
        },
    ] {
        let mut glyphs = Rows::new();
        glyphs.push(glyph).unwrap();
        invalid.push(ComponentValue::CanvasGlyphRun(CanvasGlyphRun {
            source: "canvas-revision:///font.ippf".into(),
            glyphs,
            ..Default::default()
        }));
    }
    invalid.push(ComponentValue::CanvasGlyphRun(CanvasGlyphRun {
        font_size: 0.0,
        ..Default::default()
    }));
    invalid.push(ComponentValue::CanvasText(CanvasText {
        font_size: 0.0,
        ..Default::default()
    }));
    for width in [-1.0, f32::NAN] {
        invalid.push(ComponentValue::CanvasBitmap(CanvasBitmap {
            width,
            ..Default::default()
        }));
    }
    for value in invalid {
        let outcome = panel.apply(insert(value.clone()));
        assert!(outcome.result.is_err(), "{value:?} accepted");
    }
    panel.frame();
    let after = panel.output();
    assert!(Arc::ptr_eq(&after.entries, &before.entries));
    let style = content(&after, leaf).unwrap().style().to_owned();
    assert_eq!((style.position, style.opacity), ([5.0, 0.0], 0.75));

    // A finite out-of-domain live field write is refused without effect too.
    let opacity = |value: f32| {
        vec![Command::SetField {
            entity: EntityRef::Handle(leaf),
            component: ComponentValue::CANVAS_STYLE,
            field: FieldWrite {
                offset: offset_of!(CanvasStyle, opacity) as u32,
                value: FieldValue::F32(value),
            },
        }]
    };
    assert_eq!(
        panel.apply(opacity(1.5)).result.unwrap_err().reason,
        ErrorReason::InvalidValue
    );
    panel.frame();
    let refused = panel.output();
    assert!(Arc::ptr_eq(&refused.entries, &before.entries));
    let unchanged = content(&refused, leaf).unwrap().style().to_owned();
    assert_eq!((unchanged.position, unchanged.opacity), ([5.0, 0.0], 0.75));
    panel.apply(opacity(0.5)).result.unwrap();
    panel.frame();
    let corrected = content(&panel.output(), leaf).unwrap().style().to_owned();
    assert_eq!((corrected.position, corrected.opacity), ([5.0, 0.0], 0.5));

    // An empty box is a valid shape; an empty bitmap is refused.
    let outcome = panel.apply(insert(ComponentValue::CanvasBox(CanvasBox {
        width: 0.0,
        height: 0.0,
        ..Default::default()
    })));
    assert!(outcome.result.is_ok(), "{outcome:?}");
    let outcome = panel.apply(insert(ComponentValue::CanvasBitmap(CanvasBitmap {
        width: 0.0,
        height: 0.0,
        ..Default::default()
    })));
    assert_eq!(
        outcome.result.unwrap_err().reason,
        ErrorReason::InvalidValue
    );
}

#[test]
fn surface_cache_policy_survives_world_save_and_load_while_canvas_output_is_rebuilt() {
    let mut host = crate::support::task_scheduler::host();
    let world = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, CANVAS, SURFACE]))
        .unwrap();
    let (canvas, canvas_entity) = canvas_root(&mut host, world, PANEL, None);
    create(&mut host, world, Some(canvas_entity), shape(20.0, 10.0));
    let policy = SurfaceCache {
        direct_distance: 0.5,
        texels_per_metre: 128.0,
        max_refresh_hz: 2.0,
    };
    create(
        &mut host,
        world,
        None,
        vec![
            ComponentValue::FlatSurface(FlatSurface::default()),
            ComponentValue::SurfaceCache(policy),
        ],
    );
    host.frame_for_test(0.125).unwrap();
    let original = output(&host, canvas);

    let bytes = host.save_world(world, 44, Default::default()).unwrap();
    let restored = host
        .load_world(
            &bytes,
            44,
            WorldLoadOptions {
                symbolic_id: Some("surface-cache-copy".into()),
                ..Default::default()
            },
            WorldLimits::default(),
            Default::default(),
        )
        .unwrap()
        .root
        .id();
    let entities = host.world_mut(restored).unwrap().entities();
    let anchor = entities
        .iter()
        .find(|entity| {
            entity
                .components
                .contains(&ComponentValue::SurfaceCache(policy))
        })
        .unwrap()
        .id;
    let restored_canvas = OutputRef::canvas(host.world_ref(restored).unwrap());
    // The restored World keeps the saved canvas state.
    assert_eq!(
        host.world_mut(restored)
            .unwrap()
            .canvas_state()
            .unwrap()
            .state,
        PANEL
    );
    host.frame_for_test(0.125).unwrap();

    // Derived Canvas output is evaluated again by the restored World.
    let rebuilt = output(&host, restored_canvas);
    assert_ne!(rebuilt.selection, original.selection);
    assert_eq!(rebuilt.entries.len(), original.entries.len());
    assert_eq!(
        primitive(&rebuilt.entries[0]).style().position,
        primitive(&original.entries[0]).style().position
    );

    // The restored policy reaches the attachment publication once presented.
    let child = host.create_world(Default::default(), CANVAS).unwrap();
    let (child_canvas, _) = canvas_root(&mut host, child, PANEL, None);
    apply(
        &mut host,
        restored,
        vec![Command::insert_value(
            EntityRef::Handle(anchor),
            ComponentValue::WorldAttachment(WorldAttachment::surface(child_canvas)),
        )],
    )
    .result
    .unwrap();
    host.frame_for_test(0.125).unwrap();
    let attachment = host
        .publication(host.latest_publication(restored).unwrap())
        .unwrap()
        .attachments[0]
        .clone();
    assert_eq!(
        attachment.surface_cache_policy,
        Some(SurfaceCachePolicy::new(&policy).unwrap())
    );
}
