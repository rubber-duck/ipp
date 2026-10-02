//! Ordinary control skin paint: theme and override resolution, shape materials, skin
//! assets and interaction states observed in the real Host's Canvas publication, and
//! the Background that a skin paints on an entity that is not a control.
//!
//! Expectations are hand-computed from the skin rules in `docs/architecture/gui.md`
//! and the control geometry constants, never read back from the implementation.

mod support;

use ipp_core::components::rows::Rows;
use ipp_core::components::{
    CanvasBox, CanvasStyle, GuiBehavior, GuiButton, GuiCheckbox, GuiColor, GuiLayout, GuiSlider,
    GuiTextInput,
};
use ipp_core::services::asset_management::AssetSource;
use ipp_core::services::asset_management::drawing::DRAWING_TYPE;
use ipp_core::services::gui_input::{
    GuiDeliveryError, GuiDeliveryPermit, GuiDeliveryTerminal, GuiInputContext, GuiInputService,
    GuiInputSession, GuiPointerLease,
};
use ipp_core::systems::canvas::CanvasSystem;
use ipp_core::systems::canvas::{
    CanvasBoxShape, CanvasHitKind, CanvasPaintEntry, CanvasPart, CanvasPrimitive,
    CanvasPublication, CanvasShapeChecker, CanvasShapeFill, CanvasShapeGlow,
};
use ipp_core::systems::gui::local::{
    GuiInteractionPart, GuiInteractionUpdate, GuiLocalAction, GuiLocalCommand, GuiLocalEffect,
    GuiNumberStep,
};
use ipp_core::systems::gui::presentation::{
    GuiCanvasPublication, GuiFont, GuiPaintPart, GuiSkin, GuiTheme,
};
use ipp_core::systems::gui::{
    GuiPartId, GuiPartVariant, GuiPrimitivePart, GuiSkinState, GuiSystem,
};
use ipp_core::*;
use std::sync::Arc;
use support::CanvasTestHost;
use support::gui_panel::{ControlRead, ControlValue, read_control, replacement};
use support::selection::{ASSETS, GUI_LAYOUT, select};

struct Applied;

impl GuiDeliveryPermit for Applied {
    fn prepare(&mut self, _: Option<&GuiLocalEffect>) -> Result<(), GuiDeliveryError> {
        Ok(())
    }

    fn settle(self: Box<Self>, terminal: GuiDeliveryTerminal) {
        assert!(
            matches!(
                terminal,
                GuiDeliveryTerminal::Applied(_) | GuiDeliveryTerminal::Written { .. }
            ),
            "{terminal:?}"
        );
    }
}

/// One Canvas World of 300 x 100 logical units presented as a root output.
struct Panel {
    host: HostRuntime,
    world: WorldId,
    root: OutputRef,
    root_entity: EntityId,
    input: GuiInputService,
    session: GuiInputSession,
    context: Option<GuiInputContext>,
    request: u64,
}

fn apply(host: &mut HostRuntime, world: WorldId, operations: Vec<Command>) -> BatchOutcome {
    host.world_mut(world)
        .unwrap()
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();

    host.frame(0.0)
        .unwrap()
        .worlds
        .remove(&world)
        .unwrap()
        .unwrap()
        .outcomes
        .remove(0)
}

fn create(
    host: &mut HostRuntime,
    world: WorldId,
    values: Vec<ComponentValue>,
    parent: Option<EntityId>,
) -> EntityId {
    let mut operations = vec![Command::Create {
        alias: 1,
        metadata: Default::default(),
        adopt: false,
    }];
    operations.extend(
        values
            .into_iter()
            .map(|value| Command::insert_value(EntityRef::Alias(1), value)),
    );
    if let Some(parent) = parent {
        operations.push(Command::PlaceEntity {
            entity: EntityRef::Alias(1),
            placement: EntityPlacementRef {
                parent: Some(EntityRef::Handle(parent)),
                before: None,
            },
        });
    }
    apply(host, world, operations).result.unwrap()[0].1
}

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

fn primitive(entry: &CanvasPaintEntry) -> &CanvasPrimitive {
    let CanvasPaintEntry::Primitive {
        primitive,
        ..
    } = entry
    else {
        panic!("expected a control primitive")
    };
    primitive
}

fn parts_of(canvas: &CanvasPublication, entity: EntityId) -> Vec<CanvasPart> {
    canvas
        .entries
        .iter()
        .map(|entry| primitive(entry).style().identity)
        .filter(|identity| identity.target.entity == entity)
        .map(|identity| identity.part)
        .collect()
}

fn part_of(canvas: &CanvasPublication, entity: EntityId, part: CanvasPart) -> &CanvasPrimitive {
    canvas
        .entries
        .iter()
        .map(|entry| primitive(entry))
        .find(|primitive| {
            let identity = primitive.style().identity;
            identity.target.entity == entity && identity.part == part
        })
        .unwrap_or_else(|| panic!("{part:?} of {entity:?} must paint"))
}

struct PaintedBox {
    position: [f32; 2],
    scale: [f32; 2],
    opacity: f32,
    size: [f32; 2],
    corner_radius: [f32; 2],
    border_width: f32,
    border_color: [f32; 4],
    fill: CanvasShapeFill,
    glow: Option<CanvasShapeGlow>,
    shape: CanvasBoxShape,
}

fn painted_box(primitive: &CanvasPrimitive) -> PaintedBox {
    let CanvasPrimitive::Box {
        style,
        size,
        corner_radius,
        border_width,
        border_color,
        fill,
        glow,
        shape,
    } = primitive
    else {
        panic!("expected box paint, found {primitive:?}")
    };
    PaintedBox {
        position: style.position,
        scale: style.scale,
        opacity: style.opacity,
        size: *size,
        corner_radius: *corner_radius,
        border_width: *border_width,
        border_color: *border_color,
        fill: *fill,
        glow: *glow,
        shape: *shape,
    }
}

impl Panel {
    fn new() -> Self {
        let mut host = HostRuntime::new();
        let world = host
            .create_world(Default::default(), &select(&[ASSETS, GUI_LAYOUT]))
            .unwrap();
        let root_entity = create(
            &mut host,
            world,
            vec![ComponentValue::GuiLayout(GuiLayout {
                kind: 1,
                ..Default::default()
            })],
            None,
        );
        let world_ref = host.world_ref(world).unwrap();
        let root = host.canvas_output(world_ref, [300.0, 100.0], 100.0);
        // These tests resolve each state's paint; the skin motion suite owns
        // the transitions between states.
        host.world_mut(world)
            .unwrap()
            .enqueue_gui_preferences_update(ipp_core::systems::gui::GuiPreferencesUpdate {
                reduced_motion: Some(true),
            })
            .unwrap();
        let input = GuiInputService::default();
        let session = input.open_session().unwrap();
        Self {
            host,
            world,
            root,
            root_entity,
            input,
            session,
            context: None,
            request: 0,
        }
    }

    fn apply(&mut self, operations: Vec<Command>) -> BatchOutcome {
        apply(&mut self.host, self.world, operations)
    }

    fn insert(&mut self, entity: EntityId, value: ComponentValue) {
        self.apply(vec![Command::insert_value(
            EntityRef::Handle(entity),
            value,
        )])
        .result
        .unwrap();
    }

    fn entity(&mut self, values: Vec<ComponentValue>) -> EntityId {
        create(&mut self.host, self.world, values, None)
    }

    /// A 50 x 20 control laid out in the root row.
    fn control(&mut self, value: ComponentValue) -> EntityId {
        self.laid_out(
            value,
            GuiLayout {
                width: 50.0,
                height: 20.0,
                ..Default::default()
            },
        )
    }

    /// A control with its own layout in the root row.
    fn laid_out(&mut self, value: ComponentValue, layout: GuiLayout) -> EntityId {
        let parent = self.root_entity;
        create(
            &mut self.host,
            self.world,
            vec![value, ComponentValue::GuiLayout(layout)],
            Some(parent),
        )
    }

    /// Give `entity` an inherited label font size without a font asset.
    fn font_size(&mut self, entity: EntityId, size: f32) {
        self.insert(
            entity,
            ComponentValue::GuiFont(GuiFont {
                source: "".into(),
                variant: 0,
                font_size: size,
            }),
        );
    }

    /// Give the root the test font at `size` units per em; [`Panel::load`]
    /// then completes it. Its line is 1.2 em and its `A` advances 0.6 em.
    fn font(&mut self, size: f32) {
        self.host
            .register_stream_resource_provider("gui-font")
            .unwrap();
        let root = self.root_entity;
        self.insert(
            root,
            ComponentValue::GuiFont(GuiFont {
                source: "gui-font:///body.ippf".into(),
                variant: 0,
                font_size: size,
            }),
        );
    }

    fn theme(&mut self, parts: Rows<GuiPaintPart>) -> EntityId {
        self.entity(vec![ComponentValue::GuiTheme(GuiTheme {
            parts,
            ..Default::default()
        })])
    }

    fn skin(&mut self, entity: EntityId, theme: EntityId, parts: Rows<GuiPaintPart>) {
        self.insert(
            entity,
            ComponentValue::GuiSkin(GuiSkin {
                theme,
                parts,
            }),
        );
    }

    fn frame(&mut self) {
        // Advance every loader as a render-capable Host's service phase does;
        // headless evaluation progress alone never loads texture payloads.
        self.host.progress_assets();
        let frame = self.host.frame(0.1).unwrap();
        assert!(
            frame.worlds.values().all(Result::is_ok),
            "{:?}",
            frame.worlds
        );
        assert!(
            frame.publication_errors.is_empty(),
            "{:?}",
            frame.publication_errors
        );
    }

    fn canvas(&self) -> CanvasPublication {
        self.host
            .publication(self.host.latest_publication(self.world).unwrap())
            .unwrap()
            .output(self.root)
            .unwrap()
            .data::<CanvasPublication>()
            .unwrap()
            .clone()
    }

    fn present(&mut self) {
        self.host
            .set_root_output(
                self.root,
                WorldViewport {
                    width: 300,
                    height: 100,
                    device_pixel_ratio: 1.0,
                },
            )
            .unwrap();
        self.frame();
        self.context = Some(
            self.input
                .bind_context(&self.host, &self.session, self.root.world())
                .unwrap()
                .context,
        );
    }

    fn routed(&mut self, entity: EntityId) -> ipp_core::services::gui_input::GuiInputCommand {
        self.request += 1;
        let target = self.read(entity).target;
        self.input
            .reserve_routed(
                &self.host,
                self.context.as_ref().expect("presented panel"),
                target,
                self.request,
                &[],
                Box::new(Applied),
            )
            .unwrap()
    }

    /// Read the control the way a client does.
    fn read(&mut self, entity: EntityId) -> ControlRead {
        read_control(&mut self.host, self.world, entity).unwrap()
    }

    fn feedback(
        &mut self,
        entity: EntityId,
        lease: &mut Option<GuiPointerLease>,
        update: GuiInteractionUpdate,
    ) {
        let input = self.routed(entity);
        let lease = lease
            .get_or_insert_with(|| self.input.pointer_lease(&input, 1).unwrap())
            .clone();
        let command = GuiLocalCommand::interaction(input, lease, update).unwrap();
        self.host
            .world_mut(self.world)
            .unwrap()
            .enqueue_system_command(GuiSystem::ID, 800, command)
            .unwrap();
    }

    /// Physical focus; `visible` distinguishes keyboard from pointer-press focus.
    fn focus(&mut self, entity: EntityId, visible: bool) {
        let input = self.routed(entity);
        let command = GuiLocalCommand::focus(input, 0, visible).unwrap();
        self.host
            .world_mut(self.world)
            .unwrap()
            .enqueue_system_command(GuiSystem::ID, 800, command)
            .unwrap();
    }

    /// Queue one `GuiAction` command; the next frame applies it.
    fn semantic(&mut self, entity: EntityId, action: GuiLocalAction) {
        self.request += 1;
        let target = self.read(entity).target;
        self.host
            .world_mut(self.world)
            .unwrap()
            .enqueue(Batch {
                id: 1 << 32 | self.request,
                operations: vec![Command::GuiAction {
                    target: GuiActionTarget {
                        entity: EntityRef::Handle(target.entity),
                        component: target.component,
                        incarnation: target.incarnation,
                    },
                    action,
                }],
            })
            .unwrap();
    }

    /// Queue a client's compare-and-set replacement of the control's value.
    fn replace(&mut self, entity: EntityId, value: ControlValue) {
        let read = self.read(entity);
        self.host
            .world_mut(self.world)
            .unwrap()
            .enqueue(Batch {
                id: 7,
                operations: vec![replacement(&read, value)],
            })
            .unwrap();
    }

    /// Complete `expected` acquisitions from `bytes(uri)` as a provider issues
    /// them, then wait for dependent paint.
    fn load(&mut self, bytes: impl Fn(&str) -> Vec<u8>, expected: usize) {
        let mut sources = Vec::new();
        for _ in 0..32 {
            for request in self.host.take_resource_requests() {
                self.host
                    .complete_resource(request.id, Ok(bytes(&request.source)))
                    .unwrap();
                sources.push(request.source);
            }
            if sources.len() >= expected {
                break;
            }
            self.frame();
        }
        assert_eq!(sources.len(), expected, "{sources:?}");
        for _ in 0..8 {
            self.frame();
        }
    }
}

/// Drawing whose view box is `[10, 20, 42, 44]`: a nonunit, nonzero-origin extent.
fn drawing_bytes() -> Vec<u8> {
    let mut bytes = b"IPPD".to_vec();
    bytes.extend(1_u32.to_le_bytes());
    for value in [10.0_f32, 20.0, 42.0, 44.0, 10.0, 20.0, 42.0, 44.0, 0.05] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(0_u32.to_le_bytes());
    bytes
}

/// One opaque white 1 x 1 RGBA texture.
fn texture_bytes() -> Vec<u8> {
    let mut bytes = b"IPPT".to_vec();
    for value in [3_u32, 1, 1] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend([255_u8; 4]);
    bytes
}

fn asset(kind: ipp_core::services::asset_management::AssetTypeId, uri: &str) -> AssetSource {
    AssetSource {
        kind,
        uri: uri.into(),
        variant: 0,
    }
}

#[test]
fn theme_materials_paint_corner_border_gradients_and_glow_in_logical_units() {
    let mut panel = Panel::new();
    let button = panel.control(ComponentValue::GuiButton(GuiButton::default()));
    let background = GuiPartId::base(GuiPrimitivePart::Background);
    // The theme sits on the button's default look, so it uncuts the corners
    // its radii round.
    let linear = GuiPaintPart {
        corner_cut: Some([0.0; 4]),
        corner_radius: Some([5.0, 8.0]),
        border_width: Some(1.0),
        border_color: Some([0.2, 0.4, 0.8, 1.0]),
        fill_mode: Some(1.0),
        gradient_start: Some([0.0, 0.0]),
        gradient_end: Some([50.0, 20.0]),
        gradient_color0: Some([1.0, 0.0, 0.0, 1.0]),
        gradient_color1: Some([0.0, 0.0, 1.0, 1.0]),
        glow_color: Some([1.0, 0.5, 0.0, 1.0]),
        glow_intensity: Some(2.0),
        glow_radius: Some(5.0),
        glow_falloff: Some(1.5),
        ..row(background)
    };
    let theme = panel.theme(rows([linear.clone()]));
    panel.skin(button, theme, Rows::new());
    panel.frame();

    let expected_glow = Some(CanvasShapeGlow {
        color: [1.0, 0.5, 0.0, 1.0],
        intensity: 2.0,
        radius: 5.0,
        inner_radius: 0.0,
        falloff: 1.5,
    });
    let canvas = panel.canvas();
    let painted = painted_box(part_of(&canvas, button, CanvasPart::Background));
    // Layout supplies the 50 x 20 shape independently of its corner and border.
    assert_eq!(painted.size, [50.0, 20.0]);
    assert_eq!(painted.shape, CanvasBoxShape::RECT);
    assert_eq!(painted.corner_radius, [5.0, 8.0]);
    assert_eq!(painted.border_width, 1.0);
    assert_eq!(painted.border_color, [0.2, 0.4, 0.8, 1.0]);
    assert_eq!(
        painted.fill,
        CanvasShapeFill::LinearGradient {
            start: [0.0, 0.0],
            end: [50.0, 20.0],
            start_color: [1.0, 0.0, 0.0, 1.0],
            end_color: [0.0, 0.0, 1.0, 1.0],
        }
    );
    assert_eq!(painted.glow, expected_glow);

    // A radial fill takes its centre from the gradient start and its own radius;
    // a missing second stop repeats the first.
    let radial = GuiPaintPart {
        fill_mode: Some(2.0),
        gradient_start: Some([25.0, 10.0]),
        gradient_radius: Some(12.0),
        gradient_color1: None,
        ..linear
    };
    panel.insert(
        theme,
        ComponentValue::GuiTheme(GuiTheme {
            parts: rows([radial]),
            ..Default::default()
        }),
    );
    panel.frame();
    let canvas = panel.canvas();
    assert_eq!(
        painted_box(part_of(&canvas, button, CanvasPart::Background)).fill,
        CanvasShapeFill::RadialGradient {
            center: [25.0, 10.0],
            radius: 12.0,
            start_color: [1.0, 0.0, 0.0, 1.0],
            end_color: [1.0, 0.0, 0.0, 1.0],
        }
    );

    // Material lengths are canvas logical units: doubling the canvas density
    // changes the physical mapping, never the published material values.
    panel
        .host
        .world_mut(panel.world)
        .unwrap()
        .enqueue_canvas_state_update(CanvasStateUpdate {
            extent: None,
            units_per_metre: Some(200.0),
        })
        .unwrap();
    panel.frame();
    let dense = panel.canvas();
    assert_eq!(dense.units_per_metre, 200.0);
    let painted = painted_box(part_of(&dense, button, CanvasPart::Background));
    assert_eq!(painted.size, [50.0, 20.0]);
    assert_eq!(painted.corner_radius, [5.0, 8.0]);
    assert_eq!(painted.border_width, 1.0);
    assert_eq!(painted.glow, expected_glow);
}

#[test]
fn glow_paints_beyond_the_box_without_widening_the_hit_target_or_reflowing() {
    let mut panel = Panel::new();
    let button = panel.control(ComponentValue::GuiButton(GuiButton::default()));
    let background = GuiPartId::base(GuiPrimitivePart::Background);
    let theme = panel.theme(rows([GuiPaintPart {
        color: Some([0.2, 0.2, 0.2, 1.0]),
        ..row(background)
    }]));
    panel.skin(button, theme, Rows::new());
    panel.frame();
    let plain = panel.canvas();
    assert_eq!(
        painted_box(part_of(&plain, button, CanvasPart::Background)).glow,
        None
    );

    // A material edit on the shared theme repaints without reflow.
    panel.insert(
        theme,
        ComponentValue::GuiTheme(GuiTheme {
            parts: rows([GuiPaintPart {
                color: Some([0.2, 0.2, 0.2, 1.0]),
                glow_intensity: Some(1.0),
                glow_radius: Some(10.0),
                ..row(background)
            }]),
            ..Default::default()
        }),
    );
    panel.frame();
    let glowing = panel.canvas();
    let painted = painted_box(part_of(&glowing, button, CanvasPart::Background));
    assert_eq!(painted.glow.map(|glow| glow.radius), Some(10.0));
    assert_eq!(painted.size, [50.0, 20.0]);
    assert_eq!(painted.position, [0.0, 0.0]);
    assert!(glowing.paint_revision > plain.paint_revision);
    assert_eq!(glowing.layout_revision, plain.layout_revision);
    assert_eq!(
        panel
            .host
            .world_mut(panel.world)
            .unwrap()
            .gui_entity_layout_statistics()
            .unwrap()
            .latest
            .reflows,
        0
    );

    // The hit region stays the laid-out rectangle.
    let hit = glowing
        .hits
        .iter()
        .find(|hit| hit.target.entity == button)
        .unwrap();
    assert_eq!(hit.bounds, [0.0, 0.0, 50.0, 20.0]);
    assert_eq!(
        hit.clip,
        plain
            .hits
            .iter()
            .find(|hit| hit.target.entity == button)
            .unwrap()
            .clip
    );
}

#[test]
fn cut_corners_accents_strokes_and_inner_glow_resolve_per_property_through_states() {
    let mut panel = Panel::new();
    let button = panel.control(ComponentValue::GuiButton(GuiButton::default()));
    let checkbox = panel.control(ComponentValue::GuiCheckbox(GuiCheckbox {
        checked: true,
        ..Default::default()
    }));
    let background = GuiPrimitivePart::Background;
    let frame = panel.theme(rows([
        GuiPaintPart {
            color: Some([0.0, 0.07, 0.11, 1.0]),
            corner_radius: Some([3.0, 3.0]),
            border_width: Some(1.0),
            border_color: Some([0.0, 0.95, 0.98, 1.0]),
            corner_cut: Some([6.0, 0.0, 6.0, 0.0]),
            corner_accent: Some([0.0, 8.0, 0.0, 8.0]),
            corner_accent_width: Some(3.0),
            glow_color: Some([0.0, 0.95, 0.98, 1.0]),
            glow_intensity: Some(0.8),
            glow_radius: Some(4.0),
            glow_inner_radius: Some(2.0),
            // Stated, because the default look's lit hover edge falls off at 2.5.
            glow_falloff: Some(1.0),
            ..row(GuiPartId::base(background))
        },
        GuiPaintPart {
            glow_inner_radius: Some(5.0),
            ..row(GuiPartId::state(background, GuiSkinState::Hovered))
        },
    ]));
    // A check mark: two strokes of the checked indicator, glowing only inward.
    let check = [[0.2, 0.55, 0.42, 0.78], [0.42, 0.78, 0.82, 0.25]];
    let mark = panel.theme(rows([
        GuiPaintPart {
            color: Some([0.0, 0.0, 0.0, 1.0]),
            border_width: Some(2.0),
            shape: Some(1.0),
            stroke_a: Some(check[0]),
            stroke_b: Some(check[1]),
            glow_intensity: Some(1.0),
            glow_inner_radius: Some(1.0),
            ..row(GuiPartId::base(GuiPrimitivePart::Icon))
        },
        GuiPaintPart {
            border_width: Some(1.5),
            corner_accent: Some([4.0; 4]),
            // Uncuts the default box's corners.
            corner_cut: Some([0.0; 4]),
            ..row(GuiPartId::base(background))
        },
    ]));
    panel.skin(button, frame, Rows::new());
    panel.skin(checkbox, mark, Rows::new());
    panel.present();

    let frame_shape = CanvasBoxShape::Rect {
        corner_cut: [6.0, 0.0, 6.0, 0.0],
        corner_accent: [0.0, 8.0, 0.0, 8.0],
        corner_accent_width: 3.0,
        checker: None,
    };
    let glow = |inner_radius| CanvasShapeGlow {
        color: [0.0, 0.95, 0.98, 1.0],
        intensity: 0.8,
        radius: 4.0,
        inner_radius,
        falloff: 1.0,
    };
    let idle = panel.canvas();
    let painted = painted_box(part_of(&idle, button, CanvasPart::Background));
    assert_eq!(painted.size, [50.0, 20.0]);
    assert_eq!(painted.shape, frame_shape);
    assert_eq!(painted.corner_radius, [3.0, 3.0]);
    assert_eq!(painted.border_width, 1.0);
    assert_eq!(painted.glow, Some(glow(2.0)));

    // The stroke takes the part's colour and its border width as thickness, and
    // an inner radius alone is a glow.
    let stroke = painted_box(part_of(&idle, checkbox, CanvasPart::Icon));
    assert_eq!(stroke.size, [10.0, 10.0]);
    assert_eq!(
        stroke.shape,
        CanvasBoxShape::Stroke {
            segments: check,
        }
    );
    assert_eq!(stroke.fill, CanvasShapeFill::Solid([0.0, 0.0, 0.0, 1.0]));
    assert_eq!(stroke.border_width, 2.0);
    assert_eq!(
        stroke.glow,
        Some(CanvasShapeGlow {
            color: [1.0; 4],
            intensity: 1.0,
            radius: 0.0,
            inner_radius: 1.0,
            falloff: 1.0,
        })
    );

    // Spans without their own width keep the ordinary border width.
    assert_eq!(
        painted_box(part_of(&idle, checkbox, CanvasPart::Background)).shape,
        CanvasBoxShape::Rect {
            corner_cut: [0.0; 4],
            corner_accent: [4.0; 4],
            corner_accent_width: 1.5,
            checker: None,
        }
    );

    // Hover replaces only the inner reach; the contour and hit rectangle stay.
    let mut lease = None;
    panel.feedback(button, &mut lease, GuiInteractionUpdate::Hover(true));
    panel.frame();
    let hovered = panel.canvas();
    let painted = painted_box(part_of(&hovered, button, CanvasPart::Background));
    assert_eq!(painted.shape, frame_shape);
    assert_eq!(painted.glow, Some(glow(5.0)));
    assert_eq!(hovered.layout_revision, idle.layout_revision);
    let hit = |canvas: &CanvasPublication| {
        canvas
            .hits
            .iter()
            .find(|hit| hit.target.entity == button)
            .unwrap()
            .bounds
    };
    assert_eq!(hit(&hovered), [0.0, 0.0, 50.0, 20.0]);
    assert_eq!(hit(&hovered), hit(&idle));

    // A per-control override uncuts the corners while the theme's accents remain.
    panel.skin(
        button,
        frame,
        rows([GuiPaintPart {
            corner_cut: Some([0.0; 4]),
            ..row(GuiPartId::base(background))
        }]),
    );
    panel.frame();
    assert_eq!(
        painted_box(part_of(&panel.canvas(), button, CanvasPart::Background)).shape,
        CanvasBoxShape::Rect {
            corner_cut: [0.0; 4],
            corner_accent: [0.0, 8.0, 0.0, 8.0],
            corner_accent_width: 3.0,
            checker: None,
        }
    );

    // Out-of-range shape values are refused at the write and leave paint alone.
    for invalid in [
        GuiPaintPart {
            shape: Some(0.5),
            ..row(GuiPartId::base(background))
        },
        GuiPaintPart {
            stroke_a: Some([0.0, 0.0, 1.5, 1.0]),
            ..row(GuiPartId::base(background))
        },
        GuiPaintPart {
            corner_accent: Some([-1.0, 0.0, 0.0, 0.0]),
            ..row(GuiPartId::base(background))
        },
    ] {
        let outcome = panel.apply(vec![Command::insert_value(
            EntityRef::Handle(frame),
            ComponentValue::GuiTheme(GuiTheme {
                parts: rows([invalid]),
                ..Default::default()
            }),
        )]);
        assert!(outcome.result.is_err(), "{outcome:?}");
    }
    panel.frame();
    assert_eq!(
        painted_box(part_of(&panel.canvas(), button, CanvasPart::Background)).glow,
        Some(glow(5.0))
    );
}

#[test]
fn solid_states_replace_an_inherited_gradient_while_glow_resolves_separately() {
    let mut panel = Panel::new();
    let button = panel.control(ComponentValue::GuiButton(GuiButton::default()));
    let background = GuiPrimitivePart::Background;
    let theme = panel.theme(rows([
        GuiPaintPart {
            color: Some([0.1, 0.2, 0.3, 1.0]),
            fill_mode: Some(1.0),
            gradient_color0: Some([1.0, 0.0, 0.0, 1.0]),
            gradient_color1: Some([0.0, 0.0, 1.0, 1.0]),
            glow_intensity: Some(0.5),
            glow_radius: Some(4.0),
            // Stated over the default look's lit hover edge, which would
            // otherwise recolour the glow and reach inward on hover.
            glow_color: Some([1.0; 4]),
            glow_inner_radius: Some(0.0),
            glow_falloff: Some(1.0),
            ..row(GuiPartId::base(background))
        },
        GuiPaintPart {
            color: Some([0.0, 1.0, 0.0, 1.0]),
            ..row(GuiPartId::state(background, GuiSkinState::Hovered))
        },
        GuiPaintPart {
            color: Some([0.9, 0.8, 0.1, 1.0]),
            fill_mode: Some(0.0),
            glow_intensity: Some(0.0),
            ..row(GuiPartId::state(background, GuiSkinState::Pressed))
        },
        GuiPaintPart {
            color: Some([0.4, 0.4, 0.4, 0.5]),
            fill_mode: Some(0.0),
            ..row(GuiPartId::state(background, GuiSkinState::Disabled))
        },
    ]));
    panel.skin(button, theme, Rows::new());
    panel.present();
    let paint = |panel: &Panel| {
        let painted = painted_box(part_of(&panel.canvas(), button, CanvasPart::Background));
        (painted.fill, painted.glow)
    };
    let gradient = CanvasShapeFill::LinearGradient {
        start: [0.0, 0.0],
        end: [1.0, 1.0],
        start_color: [1.0, 0.0, 0.0, 1.0],
        end_color: [0.0, 0.0, 1.0, 1.0],
    };
    let base_glow = Some(CanvasShapeGlow {
        color: [1.0; 4],
        intensity: 0.5,
        radius: 4.0,
        inner_radius: 0.0,
        falloff: 1.0,
    });
    assert_eq!(paint(&panel), (gradient, base_glow));

    // A colour-only state inherits the base fill mode, so its gradient hides the colour.
    let mut lease = None;
    panel.feedback(button, &mut lease, GuiInteractionUpdate::Hover(true));
    panel.frame();
    assert_eq!(paint(&panel), (gradient, base_glow));

    // Explicit solid mode paints the state colour, and zero intensity is how a
    // state suppresses an inherited glow.
    panel.feedback(button, &mut lease, GuiInteractionUpdate::Press);
    panel.frame();
    assert_eq!(
        paint(&panel),
        (CanvasShapeFill::Solid([0.9, 0.8, 0.1, 1.0]), None)
    );

    // Glow resolves independently of the fill: disabled keeps the base glow.
    panel.insert(
        button,
        ComponentValue::GuiBehavior(GuiBehavior {
            enabled: false,
            ..Default::default()
        }),
    );
    panel.frame();
    assert_eq!(
        paint(&panel),
        (CanvasShapeFill::Solid([0.4, 0.4, 0.4, 0.5]), base_glow)
    );
}

#[test]
fn background_label_icon_and_focus_ring_resolve_as_independent_parts() {
    let mut panel = Panel::new();
    panel
        .host
        .register_stream_resource_provider("gui-font")
        .unwrap();
    let root = panel.root_entity;
    panel.insert(
        root,
        ComponentValue::GuiFont(GuiFont {
            source: "gui-font:///body.ippf".into(),
            variant: 0,
            font_size: 10.0,
        }),
    );
    let button = panel.control(ComponentValue::GuiButton(GuiButton {
        label: "A".into(),
        ..Default::default()
    }));
    let input = panel.control(ComponentValue::GuiTextInput(GuiTextInput::default()));
    let theme = panel.theme(rows([
        GuiPaintPart {
            color: Some([1.0, 0.0, 0.0, 1.0]),
            ..row(GuiPartId::base(GuiPrimitivePart::Background))
        },
        GuiPaintPart {
            color: Some([0.0, 0.0, 1.0, 1.0]),
            ..row(GuiPartId::base(GuiPrimitivePart::Label))
        },
        GuiPaintPart {
            color: Some([0.0, 1.0, 0.0, 1.0]),
            ..row(GuiPartId::base(GuiPrimitivePart::FocusRing))
        },
        // Variant rows style a checkbox's value and a button's selection;
        // other controls never paint an icon from variant-only rows.
        GuiPaintPart {
            color: Some([0.8, 0.9, 1.0, 1.0]),
            ..row(GuiPartId::variant(
                GuiPrimitivePart::Icon,
                GuiSkinState::Idle,
                GuiPartVariant::Checked,
            ))
        },
        GuiPaintPart {
            color: Some([0.1, 0.2, 0.3, 1.0]),
            ..row(GuiPartId::variant(
                GuiPrimitivePart::Icon,
                GuiSkinState::Idle,
                GuiPartVariant::Unchecked,
            ))
        },
    ]));
    panel.skin(button, theme, Rows::new());
    panel.skin(input, theme, Rows::new());
    panel.load(|_| support::canvas_font_bytes(), 1);
    panel.semantic(button, GuiLocalAction::Focus(0));
    panel.frame();

    let canvas = panel.canvas();
    // The unselected button paints its icon from the unchecked row.
    assert_eq!(
        parts_of(&canvas, button),
        [
            CanvasPart::Background,
            CanvasPart::Icon,
            CanvasPart::Label,
            CanvasPart::FocusRing
        ]
    );
    assert_eq!(
        painted_box(part_of(&canvas, button, CanvasPart::Icon)).fill,
        CanvasShapeFill::Solid([0.1, 0.2, 0.3, 1.0])
    );
    // An empty text input publishes an empty label run, and has no variant
    // whose rows could paint it an icon.
    assert_eq!(
        parts_of(&canvas, input),
        [CanvasPart::Background, CanvasPart::Label]
    );
    assert_eq!(
        painted_box(part_of(&canvas, button, CanvasPart::Background)).fill,
        CanvasShapeFill::Solid([1.0, 0.0, 0.0, 1.0])
    );
    assert_eq!(
        painted_box(part_of(&canvas, input, CanvasPart::Background)).fill,
        CanvasShapeFill::Solid([1.0, 0.0, 0.0, 1.0])
    );
    let CanvasPrimitive::Glyphs {
        style,
        glyphs,
        ..
    } = part_of(&canvas, button, CanvasPart::Label)
    else {
        panic!("button label must paint glyphs")
    };
    assert_eq!(glyphs.len(), 1);
    // The label part tints the glyph run without borrowing the background colour.
    assert_eq!(style.color, [0.0, 0.0, 1.0, 1.0]);
    let ring = painted_box(part_of(&canvas, button, CanvasPart::FocusRing));
    assert_eq!(ring.border_color, [0.0, 1.0, 0.0, 1.0]);
    // The theme recolours the ring; its width is the lit line of the button's
    // default look, 1.5 at the looks' 16-unit em drawn at this 10-unit font.
    assert!((ring.border_width - 1.5 * 10.0 / 16.0).abs() < 1e-6);
    assert_eq!(ring.fill, CanvasShapeFill::Solid([0.0; 4]));
    assert_eq!(ring.size, [50.0, 20.0]);

    // A hovered-state row restyles only the hovered control and part.
    panel.insert(
        theme,
        ComponentValue::GuiTheme(GuiTheme {
            parts: rows([
                GuiPaintPart {
                    color: Some([1.0, 0.0, 0.0, 1.0]),
                    ..row(GuiPartId::base(GuiPrimitivePart::Background))
                },
                GuiPaintPart {
                    color: Some([0.0, 0.0, 1.0, 1.0]),
                    ..row(GuiPartId::base(GuiPrimitivePart::Label))
                },
                GuiPaintPart {
                    color: Some([0.0, 1.0, 0.0, 1.0]),
                    opacity: Some(0.5),
                    ..row(GuiPartId::state(
                        GuiPrimitivePart::Background,
                        GuiSkinState::Hovered,
                    ))
                },
            ]),
            ..Default::default()
        }),
    );
    panel.present();
    let before = panel.canvas();
    let mut lease = None;
    panel.feedback(input, &mut lease, GuiInteractionUpdate::Hover(true));
    panel.frame();
    let hovered = panel.canvas();
    let painted = painted_box(part_of(&hovered, input, CanvasPart::Background));
    assert_eq!(painted.fill, CanvasShapeFill::Solid([0.0, 1.0, 0.0, 1.0]));
    assert_eq!(painted.opacity, 0.5);
    let untouched = painted_box(part_of(&hovered, button, CanvasPart::Background));
    assert_eq!(untouched.fill, CanvasShapeFill::Solid([1.0, 0.0, 0.0, 1.0]));
    assert_eq!(untouched.opacity, 1.0);
    // The theme sets no border, so the button keeps its default idle line.
    assert!((untouched.border_width - 1.25 * 10.0 / 16.0).abs() < 1e-6);
    assert!(Arc::ptr_eq(&before.entries[0], &hovered.entries[0]));
    assert_eq!(before.layout_revision, hovered.layout_revision);
}

#[test]
fn pointer_focus_keeps_the_ring_only_on_text_inputs() {
    let mut panel = Panel::new();
    let button = panel.control(ComponentValue::GuiButton(GuiButton::default()));
    let input = panel.control(ComponentValue::GuiTextInput(GuiTextInput::default()));
    panel.present();
    let ring =
        |panel: &Panel, entity| parts_of(&panel.canvas(), entity).contains(&CanvasPart::FocusRing);

    // Pointer-press focus omits the ring on a button...
    panel.focus(button, false);
    panel.frame();
    assert!(panel.read(button).focused);
    assert!(!ring(&panel, button));

    // ...but keeps it on a text input, where keyboard input lands.
    panel.focus(input, false);
    panel.frame();
    assert!(!ring(&panel, button));
    assert!(ring(&panel, input));

    // Keyboard focus paints the ring everywhere.
    panel.focus(button, true);
    panel.frame();
    assert!(ring(&panel, button));
    assert!(!ring(&panel, input));
}

#[test]
fn label_skin_assets_are_rejected_so_the_measured_font_never_swaps() {
    let mut panel = Panel::new();
    panel
        .host
        .register_stream_resource_provider("gui-font")
        .unwrap();
    let root = panel.root_entity;
    panel.insert(
        root,
        ComponentValue::GuiFont(GuiFont {
            source: "gui-font:///measured.ippf".into(),
            variant: 0,
            font_size: 10.0,
        }),
    );
    let button = panel.control(ComponentValue::GuiButton(GuiButton {
        label: "AA".into(),
        ..Default::default()
    }));
    let theme = panel.theme(Rows::new());
    panel.skin(button, theme, Rows::new());
    panel.load(|_| support::canvas_font_bytes(), 1);
    let measured = panel.canvas();
    let CanvasPrimitive::Glyphs {
        font,
        glyphs,
        ..
    } = part_of(&measured, button, CanvasPart::Label)
    else {
        panic!("label glyphs")
    };
    let (font, glyphs) = (*font, glyphs.clone());

    // Neither a per-control override nor a shared theme may attach any asset,
    // font or otherwise, to the Label part.
    let label = GuiPartId::base(GuiPrimitivePart::Label);
    for kind in [
        ipp_core::services::asset_management::font::FONT_TYPE,
        DRAWING_TYPE,
    ] {
        let parts = rows([GuiPaintPart {
            asset: Some(asset(kind, "gui-font:///other.ippf")),
            ..row(label)
        }]);
        let skin = panel.apply(vec![Command::insert_value(
            EntityRef::Handle(button),
            ComponentValue::GuiSkin(GuiSkin {
                parts: parts.clone(),
                ..Default::default()
            }),
        )]);
        assert_eq!(skin.result.unwrap_err().reason, ErrorReason::InvalidValue);
        let shared = panel.apply(vec![Command::insert_value(
            EntityRef::Handle(theme),
            ComponentValue::GuiTheme(GuiTheme {
                parts,
                ..Default::default()
            }),
        )]);
        assert_eq!(shared.result.unwrap_err().reason, ErrorReason::InvalidValue);
    }
    panel.frame();
    assert!(panel.host.take_resource_requests().is_empty());
    let after = panel.canvas();
    let CanvasPrimitive::Glyphs {
        font: after_font,
        glyphs: after_glyphs,
        ..
    } = part_of(&after, button, CanvasPart::Label)
    else {
        panic!("label glyphs")
    };
    assert_eq!(*after_font, font);
    assert!(Arc::ptr_eq(after_glyphs, &glyphs));
    assert_eq!(after.layout_revision, measured.layout_revision);
}

/// Canvas-space top-left corner of a control's own hit.
fn control_origin(canvas: &CanvasPublication, entity: EntityId) -> [f32; 2] {
    let hit = canvas
        .hits
        .iter()
        .find(|hit| hit.target.entity == entity && hit.kind == CanvasHitKind::Entity)
        .unwrap_or_else(|| panic!("{entity:?} has no hit"));
    [hit.bounds[0], hit.bounds[1]]
}

/// A painted part's position relative to its control's corner.
fn local(canvas: &CanvasPublication, entity: EntityId, position: [f32; 2]) -> [f32; 2] {
    let origin = control_origin(canvas, entity);
    [position[0] - origin[0], position[1] - origin[1]]
}

/// A painted box part's control-local `[x, y, width, height]`.
fn local_rect(canvas: &CanvasPublication, entity: EntityId, part: CanvasPart) -> [f32; 4] {
    let painted = painted_box(part_of(canvas, entity, part));
    let [x, y] = local(canvas, entity, painted.position);
    [x, y, painted.size[0], painted.size[1]]
}

fn label_origin(canvas: &CanvasPublication, entity: EntityId) -> [f32; 2] {
    local(
        canvas,
        entity,
        part_of(canvas, entity, CanvasPart::Label).style().position,
    )
}

#[test]
fn labels_sit_by_role_centred_inset_or_beside_the_checkbox_box() {
    // At 10 units per em the test font's line is 12 high and "AA" 12 wide.
    let mut panel = Panel::new();
    panel.font(10.0);
    let button = panel.control(ComponentValue::GuiButton(GuiButton {
        label: "AA".into(),
        ..Default::default()
    }));
    let padded = panel.laid_out(
        ComponentValue::GuiButton(GuiButton {
            label: "AA".into(),
            ..Default::default()
        }),
        GuiLayout {
            width: 50.0,
            height: 20.0,
            padding_left: 10.0,
            padding_top: 2.0,
            ..Default::default()
        },
    );
    let input = panel.control(ComponentValue::GuiTextInput(GuiTextInput {
        text: "A".into(),
        ..Default::default()
    }));
    let labelled = panel.control(ComponentValue::GuiCheckbox(GuiCheckbox {
        label: "A".into(),
        checked: true,
    }));
    let bare = panel.control(ComponentValue::GuiCheckbox(GuiCheckbox {
        checked: true,
        ..Default::default()
    }));
    panel.load(|_| support::canvas_font_bytes(), 1);
    let canvas = panel.canvas();

    // A button centres its line in the box inside its padding.
    assert_eq!(label_origin(&canvas, button), [19.0, 4.0]);
    assert_eq!(label_origin(&canvas, padded), [10.0 + 14.0, 2.0 + 3.0]);

    // A text input insets its line one em and centres it vertically, and
    // publishes the same origin for pointer text hits.
    assert_eq!(label_origin(&canvas, input), [10.0, 4.0]);
    let gui = panel
        .host
        .publication(panel.host.latest_publication(panel.world).unwrap())
        .unwrap()
        .chunk(CanvasSystem::ID)
        .unwrap()
        .data::<GuiCanvasPublication>()
        .unwrap()
        .clone();
    let observed = gui
        .views
        .values()
        .flat_map(|view| view.controls.iter())
        .find(|control| control.record.target.entity == input)
        .unwrap();
    assert_eq!(observed.text.as_ref().unwrap().1, [10.0, 4.0]);

    // A labelled checkbox draws its box as the square of its height and its
    // label half an em beside it; without a label the box is the control.
    assert_eq!(
        local_rect(&canvas, labelled, CanvasPart::Background),
        [0.0, 0.0, 20.0, 20.0]
    );
    assert_eq!(
        local_rect(&canvas, labelled, CanvasPart::Icon),
        [5.0, 5.0, 10.0, 10.0]
    );
    assert_eq!(label_origin(&canvas, labelled), [25.0, 4.0]);
    assert_eq!(
        local_rect(&canvas, bare, CanvasPart::Background),
        [0.0, 0.0, 50.0, 20.0]
    );
    assert_eq!(
        local_rect(&canvas, bare, CanvasPart::Icon),
        [20.0, 5.0, 10.0, 10.0]
    );
}

#[test]
fn a_labelled_checkbox_measures_its_box_gap_and_label() {
    let mut panel = Panel::new();
    panel.font(10.0);
    let labelled = panel.laid_out(
        ComponentValue::GuiCheckbox(GuiCheckbox {
            label: "AA".into(),
            ..Default::default()
        }),
        GuiLayout::default(),
    );
    let bare = panel.laid_out(
        ComponentValue::GuiCheckbox(GuiCheckbox::default()),
        GuiLayout::default(),
    );
    panel.load(|_| support::canvas_font_bytes(), 1);
    let size = |panel: &mut Panel, entity| {
        panel
            .host
            .world_mut(panel.world)
            .unwrap()
            .gui_entity_layout(entity)
            .unwrap()
            .size
    };
    // The 2 em box of a small control, a 0.5 em gap and the 12-unit label.
    assert_eq!(size(&mut panel, labelled), [20.0 + 5.0 + 12.0, 20.0]);
    assert_eq!(size(&mut panel, bare), [20.0, 20.0]);
}

#[test]
fn the_focus_ring_follows_a_slider_thumb_and_a_checkbox_box() {
    let mut panel = Panel::new();
    panel.font(10.0);
    let slider = panel.control(ComponentValue::GuiSlider(GuiSlider {
        value: 0.5,
        ..Default::default()
    }));
    let labelled = panel.control(ComponentValue::GuiCheckbox(GuiCheckbox {
        label: "A".into(),
        ..Default::default()
    }));
    let bare = panel.control(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
    let button = panel.control(ComponentValue::GuiButton(GuiButton::default()));
    panel.load(|_| support::canvas_font_bytes(), 1);

    // The 15-unit thumb of a half-way 50 x 20 slider is centred at 25.
    for (control, ring) in [
        (slider, [17.5, 2.5, 15.0, 15.0]),
        (labelled, [0.0, 0.0, 20.0, 20.0]),
        (bare, [0.0, 0.0, 50.0, 20.0]),
        (button, [0.0, 0.0, 50.0, 20.0]),
    ] {
        panel.semantic(control, GuiLocalAction::Focus(0));
        panel.frame();
        let canvas = panel.canvas();
        assert_eq!(
            local_rect(&canvas, control, CanvasPart::FocusRing),
            ring,
            "{control:?}"
        );
    }
}

#[test]
fn a_range_paints_a_thumb_per_value_the_fill_between_them_and_the_ring_on_the_focused_one() {
    let mut panel = Panel::new();
    let slider = panel.control(ComponentValue::GuiSlider(GuiSlider {
        value: 0.25,
        upper: 0.75,
        range: true,
        // A range ignores the origin.
        origin: 0.5,
        ..Default::default()
    }));
    panel.frame();

    // The 50 x 20 slider's 15-unit thumbs travel by their centres from 7.5
    // to 42.5: the lower one centred at 16.25 and the upper one at 33.75.
    // The fill, one scroll bar (7 units) thick, runs between the centres.
    let lower = [8.75, 2.5, 15.0, 15.0];
    let upper = [26.25, 2.5, 15.0, 15.0];
    let canvas = panel.canvas();
    assert_eq!(local_rect(&canvas, slider, CanvasPart::Icon), lower);
    assert_eq!(local_rect(&canvas, slider, CanvasPart::PartIcon(1)), upper);
    assert_eq!(
        local_rect(&canvas, slider, CanvasPart::Fill),
        [16.25, 6.5, 17.5, 7.0]
    );
    assert!(!parts_of(&canvas, slider).contains(&CanvasPart::FocusRing));

    // The ring paints on the thumb focus names.
    for (part, ring) in [(1, upper), (0, lower)] {
        panel.semantic(slider, GuiLocalAction::Focus(part));
        panel.frame();
        let canvas = panel.canvas();
        assert_eq!(
            local_rect(&canvas, slider, CanvasPart::FocusRing),
            ring,
            "part {part}"
        );
    }

    // A press on the upper thumb fills only that thumb with the accent.
    panel.present();
    let mut lease: Option<GuiPointerLease> = None;
    for update in [
        GuiInteractionUpdate::Hover(true),
        GuiInteractionUpdate::Press,
    ] {
        let input = panel.routed(slider);
        let lease = lease
            .get_or_insert_with(|| panel.input.pointer_lease(&input, 1).unwrap())
            .clone();
        let command = GuiLocalCommand::part_interaction(
            input,
            lease,
            update,
            GuiInteractionPart::FocusPart(1),
        )
        .unwrap();
        panel
            .host
            .world_mut(panel.world)
            .unwrap()
            .enqueue_system_command(GuiSystem::ID, 800, command)
            .unwrap();
    }
    panel.frame();
    let canvas = panel.canvas();
    let fill = |part| painted_box(part_of(&canvas, slider, part)).fill;
    assert_eq!(
        fill(CanvasPart::PartIcon(1)),
        CanvasShapeFill::Solid(srgb(ACCENT))
    );
    assert_eq!(
        fill(CanvasPart::Icon),
        CanvasShapeFill::Solid(srgb(SURFACE))
    );
}

#[test]
fn a_numeric_input_paints_step_parts_as_divisions_and_mutes_the_direction_at_its_bound() {
    // At 10 units per em the fallback glyph advances half an em, so "1.25"
    // is 20 wide and its line 12 high.
    let mut panel = Panel::new();
    panel.font(10.0);
    let number = |value| {
        ComponentValue::GuiTextInput(GuiTextInput {
            numeric: true,
            value,
            min: -4.0,
            max: 4.0,
            step: 0.25,
            precision: 2,
            step_parts: true,
            ..Default::default()
        })
    };
    let wide = GuiLayout {
        width: 100.0,
        height: 20.0,
        ..Default::default()
    };
    let middle = panel.laid_out(number(1.25), wide);
    let bound = panel.laid_out(number(4.0), wide);
    let plain = panel.laid_out(
        ComponentValue::GuiTextInput(GuiTextInput {
            numeric: true,
            value: 1.25,
            precision: 2,
            ..Default::default()
        }),
        wide,
    );
    panel.load(|_| support::canvas_font_bytes(), 1);
    let canvas = panel.canvas();

    // Each part is the square of the field's height at its end, its mark
    // three quarters of an em centred in it, and the number is centred
    // between the parts.
    for (part, rect) in [
        (CanvasPart::Decrement, [0.0, 0.0, 20.0, 20.0]),
        (CanvasPart::Increment, [80.0, 0.0, 20.0, 20.0]),
        (CanvasPart::DecrementMark, [6.25, 6.25, 7.5, 7.5]),
        (CanvasPart::IncrementMark, [86.25, 6.25, 7.5, 7.5]),
    ] {
        assert_eq!(local_rect(&canvas, middle, part), rect, "{part:?}");
    }
    assert_eq!(label_origin(&canvas, middle), [40.0, 4.0]);

    // A number field without step parts keeps the text inset.
    assert!(!parts_of(&canvas, plain).contains(&CanvasPart::Decrement));
    assert_eq!(label_origin(&canvas, plain), [10.0, 4.0]);

    // The mark of the direction at its bound is muted; the other is content.
    let mark =
        |canvas: &CanvasPublication, entity, part| painted_box(part_of(canvas, entity, part)).fill;
    let text = CanvasShapeFill::Solid(srgb(0xe5f5f7));
    assert_eq!(mark(&canvas, middle, CanvasPart::IncrementMark), text);
    assert_eq!(mark(&canvas, bound, CanvasPart::DecrementMark), text);
    assert_eq!(
        mark(&canvas, bound, CanvasPart::IncrementMark),
        CanvasShapeFill::Solid(srgb(NEUTRAL))
    );

    // A pointer over the increment part lights that part alone, and a press
    // fills it with the accent and its mark with the surface colour.
    panel.present();
    let mut lease: Option<GuiPointerLease> = None;
    for update in [
        GuiInteractionUpdate::Hover(true),
        GuiInteractionUpdate::Press,
    ] {
        let input = panel.routed(middle);
        let lease = lease
            .get_or_insert_with(|| panel.input.pointer_lease(&input, 1).unwrap())
            .clone();
        let command = GuiLocalCommand::part_interaction(
            input,
            lease,
            update,
            GuiInteractionPart::Step(GuiNumberStep::Increment),
        )
        .unwrap();
        panel
            .host
            .world_mut(panel.world)
            .unwrap()
            .enqueue_system_command(GuiSystem::ID, 800, command)
            .unwrap();
        panel.frame();
        let canvas = panel.canvas();
        let increment = painted_box(part_of(&canvas, middle, CanvasPart::Increment));
        let field = painted_box(part_of(&canvas, middle, CanvasPart::Background));
        assert_eq!(increment.border_color, srgb(ACCENT), "{update:?}");
        assert_eq!(field.border_color, srgb(NEUTRAL), "{update:?}");
        assert!(field.glow.is_none(), "{update:?}");
    }
    let canvas = panel.canvas();
    assert_eq!(
        painted_box(part_of(&canvas, middle, CanvasPart::Increment)).fill,
        CanvasShapeFill::Solid(srgb(ACCENT))
    );
    assert_eq!(
        mark(&canvas, middle, CanvasPart::IncrementMark),
        CanvasShapeFill::Solid(srgb(SURFACE))
    );
    assert_eq!(mark(&canvas, middle, CanvasPart::DecrementMark), text);
}

#[test]
fn checkbox_indicator_alignment_clamps_to_its_end_cells() {
    // A 50 x 20 checkbox paints a 10 x 10 indicator whose centre travels
    // between x = 10 (align -1) and x = 40 (align +1).
    for (align_x, expected_x) in [(-4.0, 5.0), (-1.0, 5.0), (1.0, 35.0), (4.0, 35.0)] {
        let mut panel = Panel::new();
        let checkbox = panel.control(ComponentValue::GuiCheckbox(GuiCheckbox {
            checked: true,
            ..Default::default()
        }));
        panel.skin(
            checkbox,
            EntityId::from_bits(0),
            rows([GuiPaintPart {
                align_x: Some(align_x),
                ..row(GuiPartId::base(GuiPrimitivePart::Icon))
            }]),
        );
        panel.frame();
        let painted = painted_box(part_of(&panel.canvas(), checkbox, CanvasPart::Icon));
        assert_eq!(painted.size, [10.0, 10.0]);
        assert_eq!(painted.position, [expected_x, 5.0], "align_x={align_x}");
    }
}

#[test]
fn slider_fill_spans_the_rail_start_to_the_thumb_centre_with_a_stable_identity() {
    let mut panel = Panel::new();
    let slider = panel.control(ComponentValue::GuiSlider(GuiSlider::default()));
    let theme = panel.theme(rows([GuiPaintPart {
        color: Some([0.2, 0.9, 0.8, 1.0]),
        ..row(GuiPartId::base(GuiPrimitivePart::Fill))
    }]));
    panel.skin(slider, theme, Rows::new());
    panel.frame();
    // The 50 x 20 slider carries a 15 x 15 thumb whose centre travels from 7.5
    // to 42.5 over a rail one scroll bar thick (half the default 14-unit
    // font), centred vertically. The fill starts flush with the rail and ends
    // under the thumb centre.
    let rail = 7.0_f32;
    let top = (20.0 - rail) * 0.5;
    let mut identity = None;
    for value in [0.0_f32, 0.5, 1.0] {
        if value > 0.0 {
            panel.replace(slider, ControlValue::Scalar(value));
            panel.frame();
        }
        let canvas = panel.canvas();
        let background = painted_box(part_of(&canvas, slider, CanvasPart::Background));
        assert_eq!(
            (background.position, background.size),
            ([0.0, top], [50.0, rail])
        );
        let fill = part_of(&canvas, slider, CanvasPart::Fill);
        let painted = painted_box(fill);
        assert_eq!(painted.position, [0.0, top]);
        assert_eq!(painted.size, [7.5 + value * 35.0, rail], "value={value}");
        assert_eq!(painted.fill, CanvasShapeFill::Solid([0.2, 0.9, 0.8, 1.0]));
        identity.get_or_insert(fill.style().identity);
        assert_eq!(Some(fill.style().identity), identity);
        let thumb = painted_box(part_of(&canvas, slider, CanvasPart::Icon));
        assert_eq!(thumb.position[0] + thumb.size[0] * 0.5, 7.5 + value * 35.0);
    }
}

#[test]
fn texture_and_drawing_skin_assets_become_bitmap_and_drawing_parts() {
    let mut panel = Panel::new();
    panel
        .host
        .register_stream_resource_provider("gui-skin")
        .unwrap();
    let checkbox = panel.control(ComponentValue::GuiCheckbox(GuiCheckbox {
        checked: true,
        ..Default::default()
    }));
    // The part colour tints an asset and the default look supplies one, so an
    // untinted asset states white.
    let theme = panel.theme(rows([
        GuiPaintPart {
            asset: Some(asset(ipp_core::TEXTURE_TYPE, "gui-skin:///panel.ippt")),
            color: Some([1.0; 4]),
            ..row(GuiPartId::base(GuiPrimitivePart::Background))
        },
        GuiPaintPart {
            asset: Some(asset(DRAWING_TYPE, "gui-skin:///check.ippd")),
            color: Some([1.0, 0.5, 0.0, 1.0]),
            ..row(GuiPartId::base(GuiPrimitivePart::Icon))
        },
    ]));
    panel.skin(checkbox, theme, Rows::new());
    panel.frame();
    // Neither part paints before its resource is ready.
    assert!(parts_of(&panel.canvas(), checkbox).is_empty());
    panel.load(
        |uri| {
            if uri.ends_with(".ippt") {
                texture_bytes()
            } else {
                drawing_bytes()
            }
        },
        2,
    );
    let canvas = panel.canvas();
    assert_eq!(
        parts_of(&canvas, checkbox),
        [CanvasPart::Background, CanvasPart::Icon]
    );
    let CanvasPrimitive::Bitmap {
        style,
        bitmap,
        size,
    } = part_of(&canvas, checkbox, CanvasPart::Background)
    else {
        panic!("a texture skin asset must paint a bitmap part")
    };
    assert_eq!(*size, [50.0, 20.0]);
    assert_eq!(style.position, [0.0, 0.0]);
    let publication = panel.host.latest_publication(panel.world).unwrap();
    assert!(
        panel
            .host
            .publication_resource(publication, *bitmap)
            .is_some()
    );
    assert_eq!(style.color, [1.0; 4]);
    let CanvasPrimitive::Drawing {
        style,
        drawing,
    } = part_of(&canvas, checkbox, CanvasPart::Icon)
    else {
        panic!("a drawing skin asset must paint a drawing part")
    };
    // The resolved part colour tints the asset.
    assert_eq!(style.color, [1.0, 0.5, 0.0, 1.0]);
    assert!(
        panel
            .host
            .publication_resource(publication, *drawing)
            .is_some()
    );
    assert_ne!(*drawing, *bitmap);
}

#[test]
fn drawing_skin_asset_fits_its_view_box_to_the_scaled_part() {
    let mut panel = Panel::new();
    panel
        .host
        .register_stream_resource_provider("gui-skin")
        .unwrap();
    let checkbox = panel.control(ComponentValue::GuiCheckbox(GuiCheckbox {
        checked: true,
        ..Default::default()
    }));
    panel.skin(
        checkbox,
        EntityId::from_bits(0),
        rows([GuiPaintPart {
            asset: Some(asset(DRAWING_TYPE, "gui-skin:///check.ippd")),
            scale: Some([2.0, 0.5]),
            ..row(GuiPartId::base(GuiPrimitivePart::Icon))
        }]),
    );
    panel.load(|_| drawing_bytes(), 1);
    let canvas = panel.canvas();
    let CanvasPrimitive::Drawing {
        style,
        ..
    } = part_of(&canvas, checkbox, CanvasPart::Icon)
    else {
        panic!("ready drawing indicator")
    };
    // The 10 x 10 indicator centred at (25, 10) scales about that centre to a
    // 20 x 5 extent, and the [10, 20, 42, 44] view box fills exactly that extent.
    let view_box = [10.0, 20.0, 42.0, 44.0];
    let rendered_min = [
        style.position[0] + view_box[0] * style.scale[0],
        style.position[1] + view_box[1] * style.scale[1],
    ];
    let rendered_max = [
        style.position[0] + view_box[2] * style.scale[0],
        style.position[1] + view_box[3] * style.scale[1],
    ];
    for (actual, expected) in rendered_min
        .into_iter()
        .chain(rendered_max)
        .zip([15.0, 7.5, 35.0, 12.5])
    {
        assert!(
            (actual - expected).abs() < 1.0e-4,
            "{rendered_min:?}..{rendered_max:?}"
        );
    }
}

#[test]
fn control_backgrounds_sit_on_their_default_look_and_transparent_colours_stay_transparent() {
    let mut panel = Panel::new();
    // The looks' em, so their lengths are the design language's.
    let root = panel.root_entity;
    panel.font_size(root, 16.0);
    let button = panel.control(ComponentValue::GuiButton(GuiButton::default()));
    let background = GuiPartId::base(GuiPrimitivePart::Background);
    let theme = panel.theme(rows([GuiPaintPart {
        opacity: Some(0.5),
        ..row(background)
    }]));
    panel.skin(button, theme, Rows::new());
    panel.frame();
    // An ordinary control always paints its background: an opacity-only row
    // fades the default look's interior (sRGB #00131c) and keeps its line.
    let painted = painted_box(part_of(&panel.canvas(), button, CanvasPart::Background));
    assert_eq!(
        painted.fill,
        CanvasShapeFill::Solid([0.0, 0.006_512_090_6, 0.011_612_245, 1.0])
    );
    assert_eq!(painted.border_width, 1.25);
    assert_eq!(painted.opacity, 0.5);
    assert_eq!(painted.scale, [1.0, 1.0]);

    panel.insert(
        theme,
        ComponentValue::GuiTheme(GuiTheme {
            parts: rows([GuiPaintPart {
                color: Some([0.0; 4]),
                opacity: Some(0.5),
                ..row(background)
            }]),
            ..Default::default()
        }),
    );
    panel.frame();
    let painted = painted_box(part_of(&panel.canvas(), button, CanvasPart::Background));
    assert_eq!(painted.fill, CanvasShapeFill::Solid([0.0; 4]));
    assert_eq!(painted.opacity, 0.5);
}

impl Panel {
    /// Wait for the provider to request `uri`, completing no other request.
    fn request(&mut self, uri: &str) -> u64 {
        for _ in 0..32 {
            let requests = self.host.take_resource_requests();
            assert!(
                requests
                    .iter()
                    .all(|request| request.source == std::sync::Arc::<str>::from(&*uri)),
                "{requests:?}"
            );
            if let Some(request) = requests.first() {
                return request.id;
            }
            self.frame();
        }
        panic!("{uri} was never requested");
    }

    fn complete(&mut self, id: u64, bytes: Vec<u8>) {
        self.host.complete_resource(id, Ok(bytes)).unwrap();
        for _ in 0..8 {
            self.frame();
        }
    }

    fn ready(&self, uri: &str) -> Option<ipp_core::services::asset_management::AssetKey> {
        self.host.asset_resources().find(&asset(DRAWING_TYPE, uri))
    }

    /// The exact drawing painted for `entity`'s background, if any.
    fn background_drawing(
        &self,
        entity: EntityId,
    ) -> Option<ipp_core::services::asset_management::AssetKey> {
        self.canvas()
            .entries
            .iter()
            .map(|entry| primitive(entry))
            .find_map(|primitive| match primitive {
                CanvasPrimitive::Drawing {
                    style,
                    drawing,
                } if style.identity.target.entity == entity
                    && style.identity.part == CanvasPart::Background =>
                {
                    Some(*drawing)
                }
                _ => None,
            })
    }

    fn set_background_asset(&mut self, theme: EntityId, uri: Option<&str>) {
        self.insert(
            theme,
            ComponentValue::GuiTheme(GuiTheme {
                parts: rows([GuiPaintPart {
                    asset: uri.map(|uri| asset(DRAWING_TYPE, uri)),
                    color: uri.is_none().then_some([0.3, 0.3, 0.3, 1.0]),
                    ..row(GuiPartId::base(GuiPrimitivePart::Background))
                }]),
                ..Default::default()
            }),
        );
        self.frame();
    }
}

/// A button whose shared theme paints its background from a ready drawing.
/// The Host keeps no idle residency, so only live demand or a completed
/// publication's lease keeps a skin asset resident.
fn retained_skin_panel(prefix: &str) -> (Panel, EntityId, EntityId, String) {
    let mut panel = Panel::new();
    panel
        .host
        .asset_resources_mut()
        .set_idle_resident_bytes_target(0);
    panel
        .host
        .register_stream_resource_provider("gui-skin")
        .unwrap();
    let button = panel.control(ComponentValue::GuiButton(GuiButton::default()));
    let ready = format!("gui-skin:///{prefix}-ready.ippd");
    let theme = panel.theme(Rows::new());
    panel.skin(button, theme, Rows::new());
    panel.set_background_asset(theme, Some(&ready));
    let id = panel.request(&ready);
    panel.complete(id, drawing_bytes());
    assert_eq!(panel.background_drawing(button), panel.ready(&ready));
    assert!(panel.ready(&ready).is_some());
    (panel, button, theme, ready)
}

#[test]
fn pending_skin_replacement_retains_the_last_ready_asset_until_its_successor_is_ready() {
    let (mut panel, button, theme, ready) = retained_skin_panel("replace");
    let ready_key = panel.ready(&ready).unwrap();
    let next = "gui-skin:///replace-next.ippd";
    panel.set_background_asset(theme, Some(next));
    let request = panel.request(next);

    // The theme now demands only the pending replacement; the release barrier
    // keeps the last-ready drawing painted and resident.
    panel.host.flush_resource_lifecycle();
    assert_eq!(panel.ready(&ready), Some(ready_key));
    assert_eq!(panel.background_drawing(button), Some(ready_key));

    // A ready successor replaces it, and the superseded drawing is released.
    panel.complete(request, drawing_bytes());
    let next_key = panel.ready(next).unwrap();
    assert_eq!(panel.background_drawing(button), Some(next_key));
    panel.host.flush_resource_lifecycle();
    assert!(panel.ready(&ready).is_none());

    // Removing the control releases its retained drawing, while the shared
    // theme keeps demanding its pending reference until the theme goes.
    let pending = "gui-skin:///replace-pending.ippd";
    panel.set_background_asset(theme, Some(pending));
    panel.request(pending);
    assert_eq!(panel.background_drawing(button), Some(next_key));
    panel
        .apply(vec![Command::Delete {
            entity: EntityRef::Handle(button),
        }])
        .result
        .unwrap();
    panel.frame();
    panel.host.flush_resource_lifecycle();
    assert!(panel.ready(next).is_none());
    assert!(
        panel
            .host
            .asset_resources()
            .find(&asset(DRAWING_TYPE, pending))
            .is_some()
    );
    panel
        .apply(vec![Command::Delete {
            entity: EntityRef::Handle(theme),
        }])
        .result
        .unwrap();
    panel.frame();
    panel.host.flush_resource_lifecycle();
    assert!(
        panel
            .host
            .asset_resources()
            .find(&asset(DRAWING_TYPE, pending))
            .is_none()
    );
}

#[test]
fn clearing_a_skin_asset_releases_it_before_a_later_pending_replacement() {
    let (mut panel, button, theme, ready) = retained_skin_panel("clear");
    panel.set_background_asset(theme, None);
    assert_eq!(panel.background_drawing(button), None);
    assert_eq!(
        painted_box(part_of(&panel.canvas(), button, CanvasPart::Background)).fill,
        CanvasShapeFill::Solid([0.3, 0.3, 0.3, 1.0])
    );
    panel.host.flush_resource_lifecycle();
    assert!(panel.ready(&ready).is_none());

    // A later pending asset has no ready predecessor to show.
    let pending = "gui-skin:///clear-pending.ippd";
    panel.set_background_asset(theme, Some(pending));
    panel.request(pending);
    assert_eq!(panel.background_drawing(button), None);
    assert!(!parts_of(&panel.canvas(), button).contains(&CanvasPart::Background));
    assert!(panel.ready(&ready).is_none());
}

#[test]
fn unloading_a_retained_skin_asset_cancels_its_recovery_once_paint_drops_it() {
    let (mut panel, button, theme, ready) = retained_skin_panel("unload-retained");
    let ready_key = panel.ready(&ready).unwrap();
    let pending = "gui-skin:///unload-retained-pending.ippd";
    panel.set_background_asset(theme, Some(pending));
    panel.request(pending);
    assert_eq!(panel.background_drawing(button), Some(ready_key));

    // The completed publication still leases the unloaded drawing, so the
    // service may ask to recover it; the next publication drops the retained
    // paint, which cancels that recovery instead of re-acquiring the drawing.
    panel.host.asset_resources_mut().unload(ready_key);
    panel.host.progress_assets();
    let recovery: Vec<_> = panel
        .host
        .take_resource_requests()
        .into_iter()
        .map(|request| {
            assert_eq!(request.source, std::sync::Arc::<str>::from(&*ready));
            assert!(request.recovery);
            request.id
        })
        .collect();
    panel.frame();
    assert_eq!(panel.background_drawing(button), None);
    assert_eq!(panel.host.take_resource_cancellations(), recovery);
    panel.frame();
    assert!(panel.host.take_resource_requests().is_empty());
    assert!(panel.ready(&ready).is_none());
}

#[test]
fn control_canvas_and_world_teardown_release_retained_and_pending_skin_assets() {
    for owner in ["control", "canvas", "world"] {
        let (mut panel, button, theme, ready) = retained_skin_panel(owner);
        let pending = format!("gui-skin:///{owner}-pending.ippd");
        // The override on the control, not the shared theme, demands the replacement.
        panel.skin(
            button,
            theme,
            rows([GuiPaintPart {
                asset: Some(asset(DRAWING_TYPE, &pending)),
                ..row(GuiPartId::base(GuiPrimitivePart::Background))
            }]),
        );
        panel.set_background_asset(theme, None);
        panel.request(&pending);
        assert_eq!(panel.background_drawing(button), panel.ready(&ready));
        match owner {
            "control" => {
                panel
                    .apply(vec![Command::RemoveComponent {
                        entity: EntityRef::Handle(button),
                        component: ComponentValue::GUI_BUTTON,
                    }])
                    .result
                    .unwrap();
                panel.frame();
                panel
                    .apply(vec![Command::RemoveComponent {
                        entity: EntityRef::Handle(button),
                        component: ComponentValue::GUI_SKIN,
                    }])
                    .result
                    .unwrap();
            }
            "canvas" => {
                let root = panel.root_entity;
                panel
                    .apply(vec![Command::Delete {
                        entity: EntityRef::Handle(root),
                    }])
                    .result
                    .unwrap();
                panel
                    .apply(vec![Command::Delete {
                        entity: EntityRef::Handle(button),
                    }])
                    .result
                    .unwrap();
            }
            _ => {
                assert!(panel.host.destroy_world(panel.world));
            }
        }
        if owner != "world" {
            panel.frame();
        }
        panel.host.flush_resource_lifecycle();
        assert!(panel.ready(&ready).is_none(), "{owner}");
        assert!(panel.ready(&pending).is_none(), "{owner}");
    }
}

#[test]
fn a_released_ready_skin_asset_is_purged_then_reacquired_through_demand() {
    let (mut panel, button, _, ready) = retained_skin_panel("reacquire");
    let released = panel.ready(&ready).unwrap();
    panel.host.asset_resources_mut().unload(released);
    panel.host.flush_resource_lifecycle();
    panel.frame();
    assert_ne!(panel.background_drawing(button), Some(released));
    let id = panel.request(&ready);
    panel.complete(id, drawing_bytes());
    let reacquired = panel.ready(&ready).unwrap();
    assert_eq!(panel.background_drawing(button), Some(reacquired));
}

/// Accepts any delivery outcome, for input a disabled control may refuse.
struct Settled;

impl GuiDeliveryPermit for Settled {
    fn prepare(&mut self, _: Option<&GuiLocalEffect>) -> Result<(), GuiDeliveryError> {
        Ok(())
    }

    fn settle(self: Box<Self>, _: GuiDeliveryTerminal) {}
}

fn state_theme(panel: &mut Panel) -> EntityId {
    let background = GuiPrimitivePart::Background;
    panel.theme(rows(
        [
            (GuiSkinState::Idle, [0.1, 0.0, 0.0, 1.0]),
            (GuiSkinState::Hovered, [0.2, 0.0, 0.0, 1.0]),
            (GuiSkinState::Pressed, [0.3, 0.0, 0.0, 1.0]),
            (GuiSkinState::Disabled, [0.4, 0.0, 0.0, 1.0]),
        ]
        .map(|(state, color)| GuiPaintPart {
            color: Some(color),
            ..row(GuiPartId::state(background, state))
        }),
    ))
}

fn background_red(panel: &Panel, entity: EntityId) -> f32 {
    let CanvasShapeFill::Solid(color) =
        painted_box(part_of(&panel.canvas(), entity, CanvasPart::Background)).fill
    else {
        panic!("expected a solid background")
    };
    color[0]
}

#[test]
fn skinned_paint_follows_press_release_drag_off_and_cancel_without_committing() {
    let mut panel = Panel::new();
    let checkbox = panel.control(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
    let theme = state_theme(&mut panel);
    panel.skin(checkbox, theme, Rows::new());
    panel.present();
    assert_eq!(background_red(&panel, checkbox), 0.1);
    let mut lease = None;
    for (update, expected) in [
        (GuiInteractionUpdate::Hover(true), 0.2),
        (GuiInteractionUpdate::Press, 0.3),
        // Release keeps the hover, so paint returns to hovered.
        (GuiInteractionUpdate::Release, 0.2),
        (GuiInteractionUpdate::Hover(false), 0.1),
        // Dragging off while pressed keeps the pressed paint until release.
        (GuiInteractionUpdate::Hover(true), 0.2),
        (GuiInteractionUpdate::Press, 0.3),
        (GuiInteractionUpdate::Hover(false), 0.3),
        (GuiInteractionUpdate::Release, 0.1),
    ] {
        panel.feedback(checkbox, &mut lease, update);
        panel.frame();
        assert_eq!(background_red(&panel, checkbox), expected, "{update:?}");
    }

    // Cancellation drops press and hover paint at once.
    let mut lease = None;
    panel.feedback(checkbox, &mut lease, GuiInteractionUpdate::Hover(true));
    panel.feedback(checkbox, &mut lease, GuiInteractionUpdate::Press);
    panel.frame();
    assert_eq!(background_red(&panel, checkbox), 0.3);
    panel.feedback(checkbox, &mut lease, GuiInteractionUpdate::Cancel);
    panel.frame();
    assert_eq!(background_red(&panel, checkbox), 0.1);
    assert!(!panel.canvas().interaction.requires_direct());

    // Feedback alone never commits the checkbox.
    let snapshot = panel.read(checkbox);
    assert_eq!(snapshot.value, ControlValue::Bool(false));
}

#[test]
fn a_disabled_visible_control_paints_disabled_and_takes_no_pointer_input() {
    let mut panel = Panel::new();
    let checkbox = panel.control(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
    let theme = state_theme(&mut panel);
    panel.skin(checkbox, theme, Rows::new());
    panel.insert(
        checkbox,
        ComponentValue::GuiBehavior(GuiBehavior {
            enabled: false,
            ..Default::default()
        }),
    );
    panel.present();
    let canvas = panel.canvas();
    assert_eq!(background_red(&panel, checkbox), 0.4);
    let hit = canvas
        .hits
        .iter()
        .find(|hit| hit.target.entity == checkbox)
        .unwrap();
    assert!(!hit.eligible);

    // Pointer feedback addressed to the disabled control changes nothing.
    panel.request += 1;
    let snapshot = panel.read(checkbox);
    assert!(!snapshot.enabled && snapshot.visible);
    if let Ok(input) = panel.input.reserve_routed(
        &panel.host,
        panel.context.as_ref().unwrap(),
        snapshot.target,
        panel.request,
        &[],
        Box::new(Settled),
    ) && let Ok(lease) = panel.input.pointer_lease(&input, 1)
        && let Ok(command) = GuiLocalCommand::interaction(input, lease, GuiInteractionUpdate::Press)
    {
        let _ = panel
            .host
            .world_mut(panel.world)
            .unwrap()
            .enqueue_system_command(GuiSystem::ID, 800, command);
    }
    panel.frame();
    let after = panel.canvas();
    assert!(!after.interaction.pressed && !after.interaction.hovered);
    assert_eq!(background_red(&panel, checkbox), 0.4);
    let snapshot = panel.read(checkbox);
    assert!(!snapshot.interaction.pressed);
    assert_eq!(snapshot.value, ControlValue::Bool(false));
}

#[test]
fn a_missing_theme_suppresses_presentation_until_the_theme_returns() {
    let mut panel = Panel::new();
    let checkbox = panel.control(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
    let theme = state_theme(&mut panel);
    panel.skin(checkbox, theme, Rows::new());
    panel.frame();
    assert_eq!(background_red(&panel, checkbox), 0.1);
    let committed = panel.read(checkbox).value;

    // Unlike the removed node lane, which fell back to control defaults, an
    // ordinary skin whose referenced theme is gone presents nothing and takes
    // no input, while its value stays untouched.
    panel
        .apply(vec![Command::RemoveComponent {
            entity: EntityRef::Handle(theme),
            component: ComponentValue::GUI_THEME,
        }])
        .result
        .unwrap();
    panel.frame();
    let canvas = panel.canvas();
    assert!(parts_of(&canvas, checkbox).is_empty());
    assert!(
        !canvas
            .hits
            .iter()
            .find(|hit| hit.target.entity == checkbox)
            .unwrap()
            .eligible
    );
    let snapshot = panel.read(checkbox);
    assert_eq!(snapshot.value, committed);

    // Restoring the theme component restores the themed paint.
    let restored = state_theme(&mut panel);
    panel
        .apply(vec![Command::Delete {
            entity: EntityRef::Handle(restored),
        }])
        .result
        .unwrap();
    panel.insert(
        theme,
        ComponentValue::GuiTheme(GuiTheme {
            parts: rows([GuiPaintPart {
                color: Some([0.1, 0.0, 0.0, 1.0]),
                ..row(GuiPartId::state(
                    GuiPrimitivePart::Background,
                    GuiSkinState::Idle,
                ))
            }]),
            ..Default::default()
        }),
    );
    panel.frame();
    assert_eq!(background_red(&panel, checkbox), 0.1);
    assert!(
        panel
            .canvas()
            .hits
            .iter()
            .find(|hit| hit.target.entity == checkbox)
            .unwrap()
            .eligible
    );
}

#[test]
fn invalid_override_row_writes_are_rejected_without_changing_paint() {
    let mut panel = Panel::new();
    let button = panel.control(ComponentValue::GuiButton(GuiButton::default()));
    let background = GuiPartId::base(GuiPrimitivePart::Background);
    panel.skin(
        button,
        EntityId::from_bits(0),
        rows([GuiPaintPart {
            color: Some([0.5, 0.5, 0.5, 1.0]),
            ..row(background)
        }]),
    );
    panel.frame();
    let before = panel.canvas();
    let opacity = Rows::<GuiPaintPart>::offset(
        0,
        0,
        ipp_core::systems::gui::GuiPartProperty::Opacity.index(),
    )
    .unwrap();
    let write = |value| Command::SetField {
        entity: EntityRef::Handle(button),
        component: ComponentValue::GUI_SKIN,
        field: FieldWrite {
            offset: opacity,
            value: FieldValue::Dynamic(DynamicValue::F32(value)),
        },
    };
    assert_eq!(
        panel.apply(vec![write(1.5)]).result.unwrap_err().reason,
        ErrorReason::InvalidValue
    );
    panel.frame();
    let refused = panel.canvas();
    assert_eq!(refused.paint_revision, before.paint_revision);
    assert!(Arc::ptr_eq(&refused.entries, &before.entries));
    assert_eq!(
        painted_box(part_of(&refused, button, CanvasPart::Background)).opacity,
        1.0
    );

    // A whole override table with a duplicated part is rejected as a unit.
    let duplicated = panel.apply(vec![Command::insert_value(
        EntityRef::Handle(button),
        ComponentValue::GuiSkin(GuiSkin {
            parts: rows([row(background), row(background)]),
            ..Default::default()
        }),
    )]);
    assert_eq!(
        duplicated.result.unwrap_err().reason,
        ErrorReason::InvalidValue
    );

    panel.apply(vec![write(0.5)]).result.unwrap();
    panel.frame();
    let painted = painted_box(part_of(&panel.canvas(), button, CanvasPart::Background));
    assert_eq!(painted.fill, CanvasShapeFill::Solid([0.5, 0.5, 0.5, 1.0]));
    assert_eq!(painted.opacity, 0.5);
}

#[test]
fn a_state_only_icon_paints_only_in_its_state_with_one_identity() {
    let mut panel = Panel::new();
    let button = panel.control(ComponentValue::GuiButton(GuiButton::default()));
    let theme = panel.theme(rows([GuiPaintPart {
        color: Some([0.2, 0.8, 0.5, 1.0]),
        ..row(GuiPartId::state(
            GuiPrimitivePart::Icon,
            GuiSkinState::Hovered,
        ))
    }]));
    panel.skin(button, theme, Rows::new());
    panel.present();
    assert_eq!(parts_of(&panel.canvas(), button), [CanvasPart::Background]);

    let mut lease = None;
    panel.feedback(button, &mut lease, GuiInteractionUpdate::Hover(true));
    panel.frame();
    let hovered = panel.canvas();
    assert_eq!(
        parts_of(&hovered, button),
        [CanvasPart::Background, CanvasPart::Icon]
    );
    let icon = part_of(&hovered, button, CanvasPart::Icon);
    assert_eq!(
        painted_box(icon).fill,
        CanvasShapeFill::Solid([0.2, 0.8, 0.5, 1.0])
    );
    // The 20-high control paints an 11-unit icon inset at its leading edge.
    assert_eq!(painted_box(icon).size, [11.0, 11.0]);
    assert_eq!(painted_box(icon).position, [4.5, 4.5]);
    let identity = icon.style().identity;

    panel.feedback(button, &mut lease, GuiInteractionUpdate::Hover(false));
    panel.frame();
    assert_eq!(parts_of(&panel.canvas(), button), [CanvasPart::Background]);
    panel.feedback(button, &mut lease, GuiInteractionUpdate::Hover(true));
    panel.frame();
    assert_eq!(
        part_of(&panel.canvas(), button, CanvasPart::Icon)
            .style()
            .identity,
        identity
    );
}

/// A skinned 120 x 60 column with 4-unit padding, first in the root row, holding
/// a plain 112 x 10 box and then a 112 x 1 separator skinned by its rows alone.
struct SkinnedColumn {
    container: EntityId,
    content: EntityId,
    separator: EntityId,
}

fn column_layout(width: f32) -> ComponentValue {
    ComponentValue::GuiLayout(GuiLayout {
        kind: 2,
        width,
        height: 60.0,
        padding_top: 4.0,
        padding_right: 4.0,
        padding_bottom: 4.0,
        padding_left: 4.0,
        ..Default::default()
    })
}

impl Panel {
    fn skinned_column(
        &mut self,
        theme: EntityId,
        parts: Rows<GuiPaintPart>,
        separator: Rows<GuiPaintPart>,
    ) -> SkinnedColumn {
        let root = Some(self.root_entity);
        let container = create(
            &mut self.host,
            self.world,
            vec![
                column_layout(120.0),
                ComponentValue::GuiSkin(GuiSkin {
                    theme,
                    parts,
                }),
            ],
            root,
        );
        let content = create(
            &mut self.host,
            self.world,
            vec![
                ComponentValue::GuiLayout(GuiLayout {
                    width: 112.0,
                    height: 10.0,
                    ..Default::default()
                }),
                ComponentValue::CanvasBox(CanvasBox {
                    width: 112.0,
                    height: 10.0,
                    ..Default::default()
                }),
            ],
            Some(container),
        );
        let separator = create(
            &mut self.host,
            self.world,
            vec![
                ComponentValue::GuiLayout(GuiLayout {
                    width: 112.0,
                    height: 1.0,
                    ..Default::default()
                }),
                ComponentValue::GuiSkin(GuiSkin {
                    parts: separator,
                    ..Default::default()
                }),
            ],
            Some(container),
        );
        SkinnedColumn {
            container,
            content,
            separator,
        }
    }

    /// Entities of the controls the latest GUI semantic view lists.
    fn semantic_controls(&self) -> Vec<EntityId> {
        self.host
            .publication(self.host.latest_publication(self.world).unwrap())
            .unwrap()
            .chunk(CanvasSystem::ID)
            .unwrap()
            .data::<GuiCanvasPublication>()
            .unwrap()
            .views
            .values()
            .flat_map(|view| view.controls.iter())
            .map(|control| control.record.target.entity)
            .collect()
    }
}

/// Painter index of one entity's part.
fn paint_index(canvas: &CanvasPublication, entity: EntityId, part: CanvasPart) -> usize {
    canvas
        .entries
        .iter()
        .position(|entry| {
            let identity = primitive(entry).style().identity;
            identity.target.entity == entity && identity.part == part
        })
        .unwrap_or_else(|| panic!("{part:?} of {entity:?} must paint"))
}

/// Geometry and material revisions of one entity's Background entry.
fn background_revisions(canvas: &CanvasPublication, entity: EntityId) -> (u64, u64) {
    let index = paint_index(canvas, entity, CanvasPart::Background);
    let CanvasPaintEntry::Primitive {
        geometry_revision,
        material_revision,
        ..
    } = canvas.entries[index].as_ref()
    else {
        panic!("a Background is a primitive")
    };
    (*geometry_revision, *material_revision)
}

fn base_background(style: GuiPaintPart) -> GuiPaintPart {
    GuiPaintPart {
        part: GuiPartId::base(GuiPrimitivePart::Background)
            .index()
            .unwrap(),
        ..style
    }
}

#[test]
fn a_skinned_entity_that_is_not_a_control_paints_its_base_background_before_its_children() {
    let mut panel = Panel::new();
    let background = GuiPrimitivePart::Background;
    let fill = [0.0, 0.02, 0.035, 1.0];
    let cyan = [0.0, 0.9, 1.0, 1.0];
    let grid_line = [0.21, 0.36, 0.44, 1.0];
    let theme = panel.theme(rows([
        base_background(GuiPaintPart {
            color: Some(fill),
            border_width: Some(1.0),
            border_color: Some([0.0, 0.34, 0.44, 1.0]),
            corner_accent: Some([12.0; 4]),
            corner_accent_width: Some(3.0),
            ..Default::default()
        }),
        GuiPaintPart {
            color: Some([0.0, 1.0, 0.0, 1.0]),
            ..row(GuiPartId::state(background, GuiSkinState::Idle))
        },
        GuiPaintPart {
            color: Some([1.0, 1.0, 0.0, 1.0]),
            ..row(GuiPartId::state(background, GuiSkinState::Hovered))
        },
        GuiPaintPart {
            color: Some([1.0, 0.0, 0.0, 1.0]),
            border_width: Some(5.0),
            ..row(GuiPartId::state(background, GuiSkinState::Disabled))
        },
    ]));
    // The entity's own override row wins over the theme property by property.
    let column = panel.skinned_column(
        theme,
        rows([base_background(GuiPaintPart {
            border_color: Some(cyan),
            ..Default::default()
        })]),
        rows([base_background(GuiPaintPart {
            color: Some(grid_line),
            ..Default::default()
        })]),
    );
    // Disabling the panel selects no disabled row: it has no interaction state.
    panel.insert(
        column.container,
        ComponentValue::GuiBehavior(GuiBehavior {
            enabled: false,
            ..Default::default()
        }),
    );
    // A control sharing the theme still resolves its interaction states.
    let button = panel.control(ComponentValue::GuiButton(GuiButton::default()));
    panel.skin(button, theme, Rows::new());
    panel.present();

    let canvas = panel.canvas();
    let frame = part_of(&canvas, column.container, CanvasPart::Background);
    assert_eq!(
        frame.style().identity.target.component,
        ComponentValue::GUI_SKIN
    );
    let painted = painted_box(frame);
    assert_eq!(painted.position, [0.0, 0.0]);
    assert_eq!(painted.size, [120.0, 60.0]);
    assert_eq!(painted.fill, CanvasShapeFill::Solid(fill));
    assert_eq!(painted.border_width, 1.0);
    assert_eq!(painted.border_color, cyan);
    assert_eq!(
        painted.shape,
        CanvasBoxShape::Rect {
            corner_cut: [0.0; 4],
            corner_accent: [12.0; 4],
            corner_accent_width: 3.0,
            checker: None,
        }
    );
    assert_eq!(painted.glow, None);

    // Rows without a theme style the separator alone; what they leave absent
    // keeps the box primitive's neutral values.
    let line = painted_box(part_of(&canvas, column.separator, CanvasPart::Background));
    assert_eq!(line.position, [4.0, 14.0]);
    assert_eq!(line.size, [112.0, 1.0]);
    assert_eq!(line.fill, CanvasShapeFill::Solid(grid_line));
    assert_eq!(line.border_width, 0.0);
    assert_eq!(line.corner_radius, [0.0, 0.0]);
    assert_eq!(line.shape, CanvasBoxShape::RECT);
    assert_eq!(line.opacity, 1.0);

    // Only the Background paints, and it paints beneath the children in order.
    assert_eq!(
        parts_of(&canvas, column.container),
        [CanvasPart::Background]
    );
    assert_eq!(
        parts_of(&canvas, column.separator),
        [CanvasPart::Background]
    );
    let frame_index = paint_index(&canvas, column.container, CanvasPart::Background);
    let content_index = paint_index(&canvas, column.content, CanvasPart::Content);
    let line_index = paint_index(&canvas, column.separator, CanvasPart::Background);
    assert!(frame_index < content_index && content_index < line_index);

    // Neither is a hit target or a control, so neither routing nor focus
    // traversal can select it; the control beside them is both.
    for entity in [column.container, column.separator] {
        assert!(canvas.hits.iter().all(|hit| hit.target.entity != entity));
    }
    assert_eq!(panel.semantic_controls(), [button]);
    let control = part_of(&canvas, button, CanvasPart::Background);
    assert_eq!(
        control.style().identity.target.component,
        ComponentValue::GUI_BUTTON
    );
    assert_eq!(
        painted_box(control).fill,
        CanvasShapeFill::Solid([0.0, 1.0, 0.0, 1.0])
    );

    // Hovering the control changes its state alone.
    let mut lease = None;
    panel.feedback(button, &mut lease, GuiInteractionUpdate::Hover(true));
    panel.frame();
    let hovered = panel.canvas();
    assert_eq!(
        painted_box(part_of(&hovered, button, CanvasPart::Background)).fill,
        CanvasShapeFill::Solid([1.0, 1.0, 0.0, 1.0])
    );
    assert!(Arc::ptr_eq(
        &canvas.entries[frame_index],
        &hovered.entries[frame_index]
    ));
}

#[test]
fn a_skinned_canvas_box_paints_once_through_its_skin_over_its_size_or_its_layout_bounds() {
    let mut panel = Panel::new();
    let amber = [1.0, 0.95, 0.53, 1.0];
    let parts = || {
        rows([base_background(GuiPaintPart {
            color: Some(amber),
            border_width: Some(2.0),
            border_color: Some([1.0; 4]),
            corner_cut: Some([4.0, 0.0, 4.0, 0.0]),
            ..Default::default()
        })])
    };
    // Raw content placed by its style, outside any layout, keeps its own size.
    let raw = panel.entity(vec![
        ComponentValue::CanvasStyle(CanvasStyle {
            x: 200.0,
            y: 50.0,
            ..Default::default()
        }),
        ComponentValue::CanvasBox(CanvasBox {
            width: 30.0,
            height: 12.0,
            radius_x: 3.0,
            radius_y: 3.0,
        }),
        ComponentValue::GuiSkin(GuiSkin {
            parts: parts(),
            ..Default::default()
        }),
    ]);
    // A laid-out box takes its layout bounds, as its plain paint would.
    let root = Some(panel.root_entity);
    let laid_out = create(
        &mut panel.host,
        panel.world,
        vec![
            ComponentValue::GuiLayout(GuiLayout {
                width: 40.0,
                height: 16.0,
                ..Default::default()
            }),
            ComponentValue::CanvasBox(CanvasBox {
                width: 10.0,
                height: 10.0,
                ..Default::default()
            }),
            ComponentValue::GuiSkin(GuiSkin {
                parts: parts(),
                ..Default::default()
            }),
        ],
        root,
    );
    panel.frame();

    let canvas = panel.canvas();
    assert_eq!(parts_of(&canvas, raw), [CanvasPart::Background]);
    let painted = painted_box(part_of(&canvas, raw, CanvasPart::Background));
    assert_eq!(painted.position, [200.0, 50.0]);
    assert_eq!(painted.size, [30.0, 12.0]);
    // The skin's rows replace the plain white fill and the box radii.
    assert_eq!(painted.fill, CanvasShapeFill::Solid(amber));
    assert_eq!(painted.corner_radius, [0.0, 0.0]);
    assert_eq!(painted.border_width, 2.0);
    assert_eq!(
        painted.shape,
        CanvasBoxShape::Rect {
            corner_cut: [4.0, 0.0, 4.0, 0.0],
            corner_accent: [0.0; 4],
            corner_accent_width: 0.0,
            checker: None,
        }
    );
    assert_eq!(parts_of(&canvas, laid_out), [CanvasPart::Background]);
    let painted = painted_box(part_of(&canvas, laid_out, CanvasPart::Background));
    assert_eq!(painted.position, [0.0, 0.0]);
    assert_eq!(painted.size, [40.0, 16.0]);

    // Without its skin the box paints its plain content again.
    panel
        .apply(vec![Command::RemoveComponent {
            entity: EntityRef::Handle(raw),
            component: ComponentValue::GUI_SKIN,
        }])
        .result
        .unwrap();
    panel.frame();
    let canvas = panel.canvas();
    assert_eq!(parts_of(&canvas, raw), [CanvasPart::Content]);
    let plain = painted_box(part_of(&canvas, raw, CanvasPart::Content));
    assert_eq!(plain.size, [30.0, 12.0]);
    assert_eq!(plain.corner_radius, [3.0, 3.0]);
    assert_eq!(plain.fill, CanvasShapeFill::Solid([1.0; 4]));
}

#[test]
fn arc_rows_paint_ring_arcs_whose_angles_resolve_per_property_through_states() {
    let mut panel = Panel::new();
    let cyan = [0.0, 0.9, 1.0, 1.0];
    let background = GuiPrimitivePart::Background;
    // A raw progress ring styled by its rows alone: a value arc from twelve o'clock.
    let ring = |rows: Rows<GuiPaintPart>, x: f32| {
        vec![
            ComponentValue::CanvasStyle(CanvasStyle {
                x,
                y: 50.0,
                ..Default::default()
            }),
            ComponentValue::CanvasBox(CanvasBox {
                width: 40.0,
                height: 40.0,
                ..Default::default()
            }),
            ComponentValue::GuiSkin(GuiSkin {
                parts: rows,
                ..Default::default()
            }),
        ]
    };
    let value = panel.entity(ring(
        rows([base_background(GuiPaintPart {
            color: Some(cyan),
            border_width: Some(4.0),
            shape: Some(2.0),
            arc_sweep: Some(0.65),
            ..Default::default()
        })]),
        200.0,
    ));
    // Without angles an arc is the whole solid ring.
    let whole = panel.entity(ring(
        rows([base_background(GuiPaintPart {
            shape: Some(2.0),
            ..Default::default()
        })]),
        250.0,
    ));
    // A knob's tick ring as a theme on a control: 37 ticks centred on 48 cells a
    // turn over 270 degrees, whose sweep a hovered state lengthens alone.
    let ticks = panel.theme(rows([
        GuiPaintPart {
            color: Some(cyan),
            border_width: Some(4.0),
            shape: Some(2.0),
            arc_start: Some(0.625 - 1.0 / 96.0),
            arc_sweep: Some(37.0 / 48.0),
            arc_dashes: Some([48.0, 0.25]),
            ..row(GuiPartId::base(background))
        },
        GuiPaintPart {
            arc_sweep: Some(0.25),
            ..row(GuiPartId::state(background, GuiSkinState::Hovered))
        },
    ]));
    let knob = panel.control(ComponentValue::GuiButton(GuiButton::default()));
    // The override turns its start past a whole turn, which paint keeps for the
    // renderer to take modulo one turn.
    panel.skin(
        knob,
        ticks,
        rows([GuiPaintPart {
            arc_start: Some(7.25),
            ..row(GuiPartId::base(background))
        }]),
    );
    panel.present();

    let canvas = panel.canvas();
    let painted = painted_box(part_of(&canvas, value, CanvasPart::Background));
    assert_eq!(painted.size, [40.0, 40.0]);
    assert_eq!(painted.fill, CanvasShapeFill::Solid(cyan));
    assert_eq!(painted.border_width, 4.0);
    assert_eq!(
        painted.shape,
        CanvasBoxShape::Arc {
            start: 0.0,
            sweep: 0.65,
            dashes: 0.0,
            dash_duty: 1.0,
        }
    );
    assert_eq!(
        painted_box(part_of(&canvas, whole, CanvasPart::Background)).shape,
        CanvasBoxShape::Arc {
            start: 0.0,
            sweep: 1.0,
            dashes: 0.0,
            dash_duty: 1.0,
        }
    );
    let tick_ring = |canvas: &CanvasPublication, sweep: f32| {
        assert_eq!(
            painted_box(part_of(canvas, knob, CanvasPart::Background)).shape,
            CanvasBoxShape::Arc {
                start: 7.25,
                sweep,
                dashes: 48.0,
                dash_duty: 0.25,
            }
        );
    };
    tick_ring(&canvas, 37.0 / 48.0);

    // Hover replaces the sweep alone; the hit target stays the control rectangle.
    let mut lease = None;
    panel.feedback(knob, &mut lease, GuiInteractionUpdate::Hover(true));
    panel.frame();
    let hovered = panel.canvas();
    tick_ring(&hovered, 0.25);
    assert_eq!(
        hovered
            .hits
            .iter()
            .find(|hit| hit.target.entity == knob)
            .unwrap()
            .bounds,
        [0.0, 0.0, 50.0, 20.0]
    );

    // Out-of-range arc values are refused at the write and leave paint alone.
    for invalid in [
        GuiPaintPart {
            shape: Some(3.0),
            ..row(GuiPartId::base(background))
        },
        GuiPaintPart {
            arc_dashes: Some([-1.0, 0.5]),
            ..row(GuiPartId::base(background))
        },
        GuiPaintPart {
            arc_dashes: Some([8.0, 1.5]),
            ..row(GuiPartId::base(background))
        },
        GuiPaintPart {
            arc_sweep: Some(f32::INFINITY),
            ..row(GuiPartId::base(background))
        },
    ] {
        let outcome = panel.apply(vec![Command::insert_value(
            EntityRef::Handle(ticks),
            ComponentValue::GuiTheme(GuiTheme {
                parts: rows([invalid]),
                ..Default::default()
            }),
        )]);
        assert!(outcome.result.is_err(), "{outcome:?}");
    }
    panel.frame();
    tick_ring(&panel.canvas(), 0.25);
}

#[test]
fn colour_field_and_checker_rows_resolve_per_property_through_states() {
    let mut panel = Panel::new();
    let background = GuiPrimitivePart::Background;
    let raw = |rows: Rows<GuiPaintPart>, x: f32, size: [f32; 2]| {
        vec![
            ComponentValue::CanvasStyle(CanvasStyle {
                x,
                ..Default::default()
            }),
            ComponentValue::CanvasBox(CanvasBox {
                width: size[0],
                height: size[1],
                ..Default::default()
            }),
            ComponentValue::GuiSkin(GuiSkin {
                parts: rows,
                ..Default::default()
            }),
        ]
    };
    // A saturation-value field takes its hue from its own row; without one it is red.
    let field = panel.entity(raw(
        rows([base_background(GuiPaintPart {
            fill_mode: Some(4.0),
            fill_hue: Some(0.55),
            border_width: Some(1.0),
            ..Default::default()
        })]),
        0.0,
        [96.0, 96.0],
    ));
    let red_field = panel.entity(raw(
        rows([base_background(GuiPaintPart {
            fill_mode: Some(4.0),
            ..Default::default()
        })]),
        100.0,
        [32.0, 32.0],
    ));
    // A vertical hue rail runs from red at its bottom to red at its top.
    let rail = panel.entity(raw(
        rows([base_background(GuiPaintPart {
            fill_mode: Some(3.0),
            gradient_start: Some([0.0, 96.0]),
            gradient_end: Some([0.0, 0.0]),
            ..Default::default()
        })]),
        140.0,
        [24.0, 96.0],
    ));
    // An alpha rail: a colour from transparent to opaque over a checker of the
    // absent colours, and a checker of zero cells, which paints none.
    let cyan = [0.0, 0.9, 1.0, 1.0];
    let alpha = panel.entity(raw(
        rows([base_background(GuiPaintPart {
            fill_mode: Some(1.0),
            gradient_start: Some([0.0, 96.0]),
            gradient_end: Some([0.0, 0.0]),
            gradient_color0: Some([0.0, 0.9, 1.0, 0.0]),
            gradient_color1: Some(cyan),
            checker_size: Some(6.0),
            ..Default::default()
        })]),
        170.0,
        [24.0, 96.0],
    ));
    let unchecked = panel.entity(raw(
        rows([base_background(GuiPaintPart {
            color: Some([1.0, 0.0, 0.0, 0.5]),
            checker_size: Some(0.0),
            checker_color0: Some([1.0; 4]),
            ..Default::default()
        })]),
        200.0,
        [24.0, 24.0],
    ));
    // A swatch control whose theme, designed at a 16-unit em, paints a translucent
    // colour over a checker beside corner accents; its hovered state turns the fill
    // into a saturation-value field whose hue an override sets.
    let light = [0.8, 0.8, 0.8, 1.0];
    let dark = [0.1, 0.1, 0.1, 1.0];
    let swatch_theme = panel.entity(vec![ComponentValue::GuiTheme(GuiTheme {
        parts: rows([
            GuiPaintPart {
                color: Some([1.0, 0.0, 0.0, 0.25]),
                corner_cut: Some([0.0; 4]),
                corner_accent: Some([4.0; 4]),
                checker_size: Some(4.0),
                checker_color0: Some(light),
                checker_color1: Some(dark),
                ..row(GuiPartId::base(background))
            },
            GuiPaintPart {
                fill_mode: Some(4.0),
                ..row(GuiPartId::state(background, GuiSkinState::Hovered))
            },
        ]),
        em: 16.0,
    })]);
    let swatch = panel.control(ComponentValue::GuiButton(GuiButton::default()));
    panel.font_size(swatch, 32.0);
    panel.skin(
        swatch,
        swatch_theme,
        rows([GuiPaintPart {
            fill_hue: Some(2.25),
            ..row(GuiPartId::base(background))
        }]),
    );
    panel.present();

    let canvas = panel.canvas();
    let painted = |canvas: &CanvasPublication, entity| {
        painted_box(part_of(canvas, entity, CanvasPart::Background))
    };
    let field_box = painted(&canvas, field);
    assert_eq!(
        field_box.fill,
        CanvasShapeFill::SaturationValue {
            hue: 0.55
        }
    );
    assert_eq!(field_box.size, [96.0, 96.0]);
    assert_eq!(field_box.border_width, 1.0);
    assert_eq!(field_box.shape, CanvasBoxShape::RECT);
    assert_eq!(
        painted(&canvas, red_field).fill,
        CanvasShapeFill::SaturationValue {
            hue: 0.0
        }
    );
    assert_eq!(
        painted(&canvas, rail).fill,
        CanvasShapeFill::Hue {
            start: [0.0, 96.0],
            end: [0.0, 0.0],
        }
    );
    let alpha_box = painted(&canvas, alpha);
    assert_eq!(
        alpha_box.fill,
        CanvasShapeFill::LinearGradient {
            start: [0.0, 96.0],
            end: [0.0, 0.0],
            start_color: [0.0, 0.9, 1.0, 0.0],
            end_color: cyan,
        }
    );
    let checker = |shape: CanvasBoxShape| {
        let CanvasBoxShape::Rect {
            checker,
            ..
        } = shape
        else {
            panic!("expected a box, found {shape:?}")
        };
        checker
    };
    // Absent colours are the light and mid greys of sRGB #CCCCCC and #999999.
    assert_eq!(
        checker(alpha_box.shape),
        Some(CanvasShapeChecker {
            size: 6.0,
            colors: [srgb(0xCCCCCC), srgb(0x999999)],
        })
    );
    assert_eq!(checker(painted(&canvas, unchecked).shape), None);
    // The theme's cell side follows the font against its em, like its accents; the
    // override's hue waits for a fill that reads it.
    let swatch_box = painted(&canvas, swatch);
    assert_eq!(
        swatch_box.fill,
        CanvasShapeFill::Solid([1.0, 0.0, 0.0, 0.25])
    );
    assert_eq!(
        swatch_box.shape,
        CanvasBoxShape::Rect {
            corner_cut: [0.0; 4],
            corner_accent: [8.0; 4],
            corner_accent_width: swatch_box.border_width,
            checker: Some(CanvasShapeChecker {
                size: 8.0,
                colors: [light, dark],
            }),
        }
    );

    // Hover replaces the fill mode alone: the field takes the override's hue and the
    // checker stays.
    let mut lease = None;
    panel.feedback(swatch, &mut lease, GuiInteractionUpdate::Hover(true));
    panel.frame();
    let hovered = painted(&panel.canvas(), swatch);
    assert_eq!(
        hovered.fill,
        CanvasShapeFill::SaturationValue {
            hue: 2.25
        }
    );
    assert_eq!(checker(hovered.shape), checker(swatch_box.shape));

    // Out-of-range values are refused at the write and leave paint alone.
    for invalid in [
        GuiPaintPart {
            fill_mode: Some(5.0),
            ..row(GuiPartId::base(background))
        },
        GuiPaintPart {
            fill_hue: Some(f32::NAN),
            ..row(GuiPartId::base(background))
        },
        GuiPaintPart {
            checker_size: Some(-6.0),
            ..row(GuiPartId::base(background))
        },
        GuiPaintPart {
            checker_color1: Some([0.0, 0.0, 0.0, 1.5]),
            ..row(GuiPartId::base(background))
        },
    ] {
        let outcome = panel.apply(vec![Command::insert_value(
            EntityRef::Handle(swatch_theme),
            ComponentValue::GuiTheme(GuiTheme {
                parts: rows([invalid]),
                em: 16.0,
            }),
        )]);
        assert!(outcome.result.is_err(), "{outcome:?}");
    }
    panel.frame();
    assert_eq!(painted(&panel.canvas(), swatch).fill, hovered.fill);
}

#[test]
fn a_skinned_background_keeps_its_identity_through_relayout_and_reskinning_until_its_skin_ends() {
    let mut panel = Panel::new();
    let red = [1.0, 0.0, 0.0, 1.0];
    let blue = [0.0, 0.0, 1.0, 1.0];
    let theme = |panel: &mut Panel, color| {
        panel.theme(rows([base_background(GuiPaintPart {
            color: Some(color),
            ..Default::default()
        })]))
    };
    let first_theme = theme(&mut panel, red);
    let second_theme = theme(&mut panel, blue);
    let column = panel.skinned_column(
        first_theme,
        Rows::new(),
        rows([base_background(GuiPaintPart {
            color: Some(red),
            ..Default::default()
        })]),
    );
    panel.frame();
    let first = panel.canvas();
    let identity = part_of(&first, column.container, CanvasPart::Background)
        .style()
        .identity;

    // Relayout keeps the identity and reshapes its geometry.
    panel.insert(column.container, column_layout(150.0));
    panel.frame();
    let wider = panel.canvas();
    let frame = part_of(&wider, column.container, CanvasPart::Background);
    assert_eq!(frame.style().identity, identity);
    assert_eq!(painted_box(frame).size, [150.0, 60.0]);
    assert!(wider.layout_revision > first.layout_revision);
    let (geometry, material) = background_revisions(&wider, column.container);

    // Retargeting the theme keeps the identity and geometry and changes only
    // the material, without reflow.
    panel
        .apply(vec![Command::SetField {
            entity: EntityRef::Handle(column.container),
            component: ComponentValue::GUI_SKIN,
            field: FieldWrite {
                offset: std::mem::offset_of!(GuiSkin, theme) as u32,
                value: FieldValue::Entity(EntityRef::Handle(second_theme)),
            },
        }])
        .result
        .unwrap();
    panel.frame();
    let reskinned = panel.canvas();
    let frame = part_of(&reskinned, column.container, CanvasPart::Background);
    assert_eq!(frame.style().identity, identity);
    assert_eq!(painted_box(frame).fill, CanvasShapeFill::Solid(blue));
    let (next_geometry, next_material) = background_revisions(&reskinned, column.container);
    assert_eq!(next_geometry, geometry);
    assert!(next_material > material);
    assert_eq!(reskinned.layout_revision, wider.layout_revision);

    // Removing the skin removes the paint and leaves the children painting.
    panel
        .apply(vec![Command::RemoveComponent {
            entity: EntityRef::Handle(column.container),
            component: ComponentValue::GUI_SKIN,
        }])
        .result
        .unwrap();
    panel.frame();
    let bare = panel.canvas();
    assert!(parts_of(&bare, column.container).is_empty());
    assert_eq!(parts_of(&bare, column.content), [CanvasPart::Content]);
    assert_eq!(parts_of(&bare, column.separator), [CanvasPart::Background]);

    // A new skin is a new component lifetime and so a new identity.
    panel.skin(column.container, second_theme, Rows::new());
    panel.frame();
    let renewed = part_of(&panel.canvas(), column.container, CanvasPart::Background)
        .style()
        .identity;
    assert_eq!(renewed.target.entity, column.container);
    assert_eq!(renewed.part, CanvasPart::Background);
    assert_ne!(renewed, identity);

    // Deleting a skinned entity removes its paint.
    panel
        .apply(vec![Command::Delete {
            entity: EntityRef::Handle(column.separator),
        }])
        .result
        .unwrap();
    panel.frame();
    assert!(parts_of(&panel.canvas(), column.separator).is_empty());
}

#[test]
fn a_skinned_entity_background_asset_paints_like_a_control_background_asset() {
    let mut panel = Panel::new();
    panel
        .host
        .register_stream_resource_provider("gui-skin")
        .unwrap();
    let column = panel.skinned_column(
        EntityId::from_bits(0),
        rows([base_background(GuiPaintPart {
            asset: Some(asset(ipp_core::TEXTURE_TYPE, "gui-skin:///frame.ippt")),
            color: Some([0.5, 1.0, 1.0, 1.0]),
            ..Default::default()
        })]),
        Rows::new(),
    );
    panel.frame();
    // Nothing paints in its place before the resource is ready.
    assert!(parts_of(&panel.canvas(), column.container).is_empty());
    panel.load(|_| texture_bytes(), 1);
    let canvas = panel.canvas();
    let CanvasPrimitive::Bitmap {
        style,
        bitmap,
        size,
    } = part_of(&canvas, column.container, CanvasPart::Background)
    else {
        panic!("a texture skin asset must paint a bitmap part")
    };
    assert_eq!(*size, [120.0, 60.0]);
    assert_eq!(style.position, [0.0, 0.0]);
    assert_eq!(style.color, [0.5, 1.0, 1.0, 1.0]);
    let publication = panel.host.latest_publication(panel.world).unwrap();
    assert!(
        panel
            .host
            .publication_resource(publication, *bitmap)
            .is_some()
    );
}

// Default looks: what a control paints with no theme, and how themes and
// overrides compose with it. Colours and lengths are the design language's
// (tests/skin-lab/README.md), restated here independently of the runtime's
// tables, at the looks' 16-unit em.

/// Linear RGBA of an sRGB `0xrrggbb` sample.
fn srgb(hex: u32) -> [f32; 4] {
    let linear = |byte: u32| {
        let value = f64::from(byte & 0xff) / 255.0;
        (if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }) as f32
    };
    [linear(hex >> 16), linear(hex >> 8), linear(hex), 1.0]
}

/// The language's colours.
const SURFACE: u32 = 0x00131c;
const ACCENT: u32 = 0x00f4fb;
const NEUTRAL: u32 = 0x90b0c4;
const LINE_COLOUR: u32 = 0x355c70;

/// The one glow of a control frame: full strength (0.04) for focus and half
/// for hover and press, reaching 16 outward and inward.
fn edge_glow(intensity: f32) -> Option<CanvasShapeGlow> {
    part_glow(intensity, 16.0)
}

/// The glow at `reach`: a frame's 16 or a moving part's 8.
fn part_glow(intensity: f32, reach: f32) -> Option<CanvasShapeGlow> {
    Some(CanvasShapeGlow {
        color: srgb(ACCENT),
        intensity,
        radius: reach,
        inner_radius: reach,
        falloff: 2.5,
    })
}

fn cut(corner_cut: [f32; 4]) -> CanvasBoxShape {
    CanvasBoxShape::Rect {
        corner_cut,
        corner_accent: [0.0; 4],
        corner_accent_width: 0.0,
        checker: None,
    }
}

fn label_color(canvas: &CanvasPublication, entity: EntityId) -> [f32; 4] {
    part_of(canvas, entity, CanvasPart::Label).style().color
}

#[test]
fn an_unthemed_button_paints_the_reference_look_in_every_state() {
    let mut panel = Panel::new();
    panel.font(16.0);
    let size = GuiLayout {
        width: 108.0,
        height: 40.0,
        ..Default::default()
    };
    let button = panel.laid_out(
        ComponentValue::GuiButton(GuiButton {
            label: "AA".into(),
            ..Default::default()
        }),
        size,
    );
    let disabled = panel.laid_out(
        ComponentValue::GuiButton(GuiButton {
            label: "AA".into(),
            ..Default::default()
        }),
        size,
    );
    panel.insert(
        disabled,
        ComponentValue::GuiBehavior(GuiBehavior {
            enabled: false,
            ..Default::default()
        }),
    );
    panel.load(|_| support::canvas_font_bytes(), 1);
    panel.present();

    // Primary buttons take the frame cut, 8 on the paired top-left and
    // bottom-right corners, and the idle line round the surface.
    let frame = cut([8.0, 0.0, 8.0, 0.0]);
    let idle = panel.canvas();
    let painted = painted_box(part_of(&idle, button, CanvasPart::Background));
    assert_eq!(painted.size, [108.0, 40.0]);
    assert_eq!(painted.fill, CanvasShapeFill::Solid(srgb(SURFACE)));
    assert_eq!(
        (painted.border_width, painted.border_color),
        (1.25, srgb(NEUTRAL))
    );
    assert_eq!(painted.shape, frame);
    assert_eq!(painted.glow, None);
    assert_eq!(label_color(&idle, button), srgb(ACCENT));

    // Disabled draws whatever would be lit in neutral: the accent label.
    assert_eq!(
        painted_box(part_of(&idle, disabled, CanvasPart::Background)).border_color,
        srgb(NEUTRAL)
    );
    assert_eq!(label_color(&idle, disabled), srgb(NEUTRAL));

    // Hover: the lit line with the half glow, on the same geometry.
    let mut lease = None;
    panel.feedback(button, &mut lease, GuiInteractionUpdate::Hover(true));
    panel.frame();
    let hovered = panel.canvas();
    let painted = painted_box(part_of(&hovered, button, CanvasPart::Background));
    assert_eq!(painted.fill, CanvasShapeFill::Solid(srgb(SURFACE)));
    assert_eq!(
        (painted.border_width, painted.border_color),
        (1.5, srgb(ACCENT))
    );
    assert_eq!(painted.glow, edge_glow(0.02));
    assert_eq!(painted.shape, frame);

    // Press: the accent fill under the hover edge, the label in the surface
    // colour.
    panel.feedback(button, &mut lease, GuiInteractionUpdate::Press);
    panel.frame();
    let pressed = panel.canvas();
    let painted = painted_box(part_of(&pressed, button, CanvasPart::Background));
    assert_eq!(painted.fill, CanvasShapeFill::Solid(srgb(ACCENT)));
    assert_eq!(
        (painted.border_width, painted.border_color),
        (1.5, srgb(ACCENT))
    );
    assert_eq!(painted.glow, edge_glow(0.02));
    assert_eq!(label_color(&pressed, button), srgb(SURFACE));
    panel.feedback(button, &mut lease, GuiInteractionUpdate::Release);
    panel.feedback(button, &mut lease, GuiInteractionUpdate::Hover(false));

    // Focus: the lit line with the full glow on the frame's own contour, over
    // a clear interior.
    panel.semantic(button, GuiLocalAction::Focus(0));
    panel.frame();
    let ring = painted_box(part_of(&panel.canvas(), button, CanvasPart::FocusRing));
    assert_eq!(ring.size, [108.0, 40.0]);
    assert_eq!(ring.fill, CanvasShapeFill::Solid([0.0; 4]));
    assert_eq!((ring.border_width, ring.border_color), (1.5, srgb(ACCENT)));
    assert_eq!(ring.glow, edge_glow(0.04));
    assert_eq!(ring.shape, frame);
}

/// A button's stored `selected` field.
fn selected(panel: &mut Panel, entity: EntityId) -> bool {
    panel
        .host
        .world_mut(panel.world)
        .unwrap()
        .inspect(entity)
        .unwrap()
        .components
        .into_iter()
        .find_map(|value| match value {
            ComponentValue::GuiButton(button) => Some(button.selected),
            _ => None,
        })
        .unwrap()
}

/// A client's write of a button's `selected` field.
fn select_button(panel: &mut Panel, entity: EntityId, value: bool) {
    panel
        .apply(vec![Command::SetField {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::GUI_BUTTON,
            field: FieldWrite {
                offset: std::mem::offset_of!(GuiButton, selected) as u32,
                value: FieldValue::Bool(value),
            },
        }])
        .result
        .unwrap();
}

#[test]
fn a_selected_button_keeps_its_lit_fill_through_hover_press_and_focus() {
    let mut panel = Panel::new();
    panel.font(16.0);
    let size = GuiLayout {
        width: 108.0,
        height: 40.0,
        ..Default::default()
    };
    let selected_button = |label: &str| {
        ComponentValue::GuiButton(GuiButton {
            label: label.into(),
            selected: true,
        })
    };
    let button = panel.laid_out(selected_button("AA"), size);
    let disabled = panel.laid_out(selected_button("AA"), size);
    panel.insert(
        disabled,
        ComponentValue::GuiBehavior(GuiBehavior {
            enabled: false,
            ..Default::default()
        }),
    );
    panel.load(|_| support::canvas_font_bytes(), 1);
    panel.present();
    let frame = cut([8.0, 0.0, 8.0, 0.0]);

    // Selected: the accent fill and line on the frame's own cut, the label in
    // the surface colour.
    let idle = panel.canvas();
    let painted = painted_box(part_of(&idle, button, CanvasPart::Background));
    assert_eq!(painted.fill, CanvasShapeFill::Solid(srgb(ACCENT)));
    assert_eq!(
        (painted.border_width, painted.border_color),
        (1.25, srgb(ACCENT))
    );
    assert_eq!((painted.shape, painted.glow), (frame, None));
    assert_eq!(label_color(&idle, button), srgb(SURFACE));

    // Disabled draws the fill in neutral and keeps the surface label on it.
    let painted = painted_box(part_of(&idle, disabled, CanvasPart::Background));
    assert_eq!(painted.fill, CanvasShapeFill::Solid(srgb(NEUTRAL)));
    assert_eq!(painted.border_color, srgb(NEUTRAL));
    assert_eq!(label_color(&idle, disabled), srgb(SURFACE));

    // Hover and press add the hover edge to the same fill.
    let mut lease = None;
    for update in [
        GuiInteractionUpdate::Hover(true),
        GuiInteractionUpdate::Press,
    ] {
        panel.feedback(button, &mut lease, update);
        panel.frame();
        let canvas = panel.canvas();
        let painted = painted_box(part_of(&canvas, button, CanvasPart::Background));
        assert_eq!(
            painted.fill,
            CanvasShapeFill::Solid(srgb(ACCENT)),
            "{update:?}"
        );
        assert_eq!(
            (painted.border_width, painted.border_color, painted.glow),
            (1.5, srgb(ACCENT), edge_glow(0.02)),
            "{update:?}"
        );
        assert_eq!(label_color(&canvas, button), srgb(SURFACE), "{update:?}");
    }
    panel.feedback(button, &mut lease, GuiInteractionUpdate::Release);
    panel.feedback(button, &mut lease, GuiInteractionUpdate::Hover(false));

    // Pressing a selected button does not unselect it.
    panel.semantic(button, GuiLocalAction::Press);
    panel.frame();
    assert!(selected(&mut panel, button));

    // Focus lights the same contour with the ring and leaves the fill.
    panel.semantic(button, GuiLocalAction::Focus(0));
    panel.frame();
    let focused = panel.canvas();
    let painted = painted_box(part_of(&focused, button, CanvasPart::Background));
    assert_eq!(painted.fill, CanvasShapeFill::Solid(srgb(ACCENT)));
    assert_eq!(label_color(&focused, button), srgb(SURFACE));
    let ring = painted_box(part_of(&focused, button, CanvasPart::FocusRing));
    assert_eq!((ring.border_width, ring.border_color), (1.5, srgb(ACCENT)));
    assert_eq!((ring.glow, ring.shape), (edge_glow(0.04), frame));

    // The field is the selection: a client write unselects the button.
    select_button(&mut panel, button, false);
    panel.frame();
    let unselected = panel.canvas();
    let painted = painted_box(part_of(&unselected, button, CanvasPart::Background));
    assert_eq!(painted.fill, CanvasShapeFill::Solid(srgb(SURFACE)));
    assert_eq!(label_color(&unselected, button), srgb(ACCENT));
}

#[test]
fn theme_rows_of_the_checked_variants_style_selected_and_unselected_buttons() {
    let mut panel = Panel::new();
    let red = [1.0, 0.0, 0.0, 1.0];
    let blue = [0.0, 0.0, 1.0, 1.0];
    let variant = |part, variant| GuiPartId::variant(part, GuiSkinState::Idle, variant);
    let theme = panel.theme(rows([
        GuiPaintPart {
            color: Some(red),
            ..row(variant(
                GuiPrimitivePart::Background,
                GuiPartVariant::Checked,
            ))
        },
        GuiPaintPart {
            color: Some(blue),
            ..row(variant(
                GuiPrimitivePart::Background,
                GuiPartVariant::Unchecked,
            ))
        },
    ]));
    let selected = panel.control(ComponentValue::GuiButton(GuiButton {
        label: "A".into(),
        selected: true,
    }));
    let unselected = panel.control(ComponentValue::GuiButton(GuiButton {
        label: "A".into(),
        selected: false,
    }));
    for button in [selected, unselected] {
        panel.skin(button, theme, Rows::new());
    }
    panel.present();
    let canvas = panel.canvas();
    let background = |entity| painted_box(part_of(&canvas, entity, CanvasPart::Background)).fill;
    assert_eq!(background(selected), CanvasShapeFill::Solid(red));
    assert_eq!(background(unselected), CanvasShapeFill::Solid(blue));

    // A property the theme leaves to the default look keeps the look's
    // selected row: the accent line.
    assert_eq!(
        painted_box(part_of(&canvas, selected, CanvasPart::Background)).border_color,
        srgb(ACCENT)
    );
    assert_eq!(
        painted_box(part_of(&canvas, unselected, CanvasPart::Background)).border_color,
        srgb(NEUTRAL)
    );
}

#[test]
fn unthemed_checkboxes_and_sliders_paint_their_reference_looks() {
    let mut panel = Panel::new();
    let root = panel.root_entity;
    panel.font_size(root, 16.0);
    let unchecked = panel.control(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
    let checked = panel.control(ComponentValue::GuiCheckbox(GuiCheckbox {
        checked: true,
        ..Default::default()
    }));
    let disabled = panel.control(ComponentValue::GuiCheckbox(GuiCheckbox {
        checked: true,
        ..Default::default()
    }));
    panel.insert(
        disabled,
        ComponentValue::GuiBehavior(GuiBehavior {
            enabled: false,
            ..Default::default()
        }),
    );
    let slider = panel.control(ComponentValue::GuiSlider(GuiSlider {
        value: 0.5,
        ..Default::default()
    }));
    panel.frame();
    let canvas = panel.canvas();

    // Small controls and parts take the part cut, 4 on the paired corners.
    let small = cut([4.0, 0.0, 4.0, 0.0]);
    let box_of = |entity| painted_box(part_of(&canvas, entity, CanvasPart::Background));
    let painted = box_of(unchecked);
    assert_eq!(painted.fill, CanvasShapeFill::Solid(srgb(SURFACE)));
    assert_eq!(
        (painted.border_width, painted.border_color),
        (1.25, srgb(NEUTRAL))
    );
    assert_eq!(painted.shape, small);
    assert!(!parts_of(&canvas, unchecked).contains(&CanvasPart::Icon));

    // Checked fills the box with the accent and strokes the check mark in the
    // surface colour, 4 units thick; disabled turns both neutral.
    let painted = box_of(checked);
    assert_eq!(painted.fill, CanvasShapeFill::Solid(srgb(ACCENT)));
    assert_eq!(painted.border_color, srgb(ACCENT));
    let mark = painted_box(part_of(&canvas, checked, CanvasPart::Icon));
    assert_eq!(mark.fill, CanvasShapeFill::Solid(srgb(SURFACE)));
    assert_eq!(mark.border_width, 4.0);
    assert!(matches!(mark.shape, CanvasBoxShape::Stroke { .. }));
    let painted = box_of(disabled);
    assert_eq!(painted.fill, CanvasShapeFill::Solid(srgb(NEUTRAL)));
    assert_eq!(painted.border_color, srgb(NEUTRAL));

    // The slider: a rail in the quiet line at 15% inside the quiet line, a
    // solid accent value without glow and a thumb outlined in the accent
    // round the surface, cut as a part.
    let rail = box_of(slider);
    let mut rail_fill = srgb(LINE_COLOUR);
    rail_fill[3] = 0.15;
    assert_eq!(rail.fill, CanvasShapeFill::Solid(rail_fill));
    assert_eq!(
        (rail.border_width, rail.border_color),
        (1.25, srgb(LINE_COLOUR))
    );
    let fill = painted_box(part_of(&canvas, slider, CanvasPart::Fill));
    assert_eq!(fill.fill, CanvasShapeFill::Solid(srgb(ACCENT)));
    assert_eq!(fill.glow, None);
    let thumb = painted_box(part_of(&canvas, slider, CanvasPart::Icon));
    assert_eq!(thumb.fill, CanvasShapeFill::Solid(srgb(SURFACE)));
    assert_eq!(
        (thumb.border_width, thumb.border_color),
        (1.25, srgb(ACCENT))
    );
    assert_eq!(thumb.shape, small);
}

/// A painted arc's start, sweep, dash cells and dash duty.
fn arc(painted: &PaintedBox) -> [f32; 4] {
    match painted.shape {
        CanvasBoxShape::Arc {
            start,
            sweep,
            dashes,
            dash_duty,
        } => [start, sweep, dashes, dash_duty],
        shape => panic!("expected an arc, found {shape:?}"),
    }
}

/// Whether two lists of numbers agree to a millionth.
fn close(left: &[f32], right: &[f32]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| (left - right).abs() < 1e-5)
}

#[test]
fn an_unthemed_dial_paints_its_rings_inside_a_cut_housing() {
    let mut panel = Panel::new();
    let root = panel.root_entity;
    panel.font_size(root, 16.0);
    // An unsized dial is a square of five ems, at the value's middle; a
    // bipolar one over -1..1 at -0.5 fills from zero.
    let dial = panel.laid_out(
        ComponentValue::GuiSlider(GuiSlider {
            value: 0.5,
            axis: 2,
            ..Default::default()
        }),
        GuiLayout::default(),
    );
    let bipolar = panel.laid_out(
        ComponentValue::GuiSlider(GuiSlider {
            min: -1.0,
            max: 1.0,
            value: -0.5,
            origin: 0.0,
            axis: 2,
            ..Default::default()
        }),
        GuiLayout::default(),
    );
    panel.present();
    let canvas = panel.canvas();
    let part = |entity, part| painted_box(part_of(&canvas, entity, part));

    // The housing is the whole control: a frame-cut surface in the idle line.
    let housing = part(dial, CanvasPart::Background);
    assert_eq!(
        local_rect(&canvas, dial, CanvasPart::Background),
        [0.0, 0.0, 80.0, 80.0]
    );
    assert_eq!(housing.fill, CanvasShapeFill::Solid(srgb(SURFACE)));
    assert_eq!(
        (housing.border_width, housing.border_color),
        (1.25, srgb(NEUTRAL))
    );
    assert_eq!(housing.shape, cut([8.0, 0.0, 8.0, 0.0]));

    // Ticks 4 long in the quiet line, their ring half an em inside the
    // housing, one on each tenth of the 270-degree sweep from half past
    // seven: the ring starts and ends half a cell beyond the sweep.
    let ticks = part(dial, CanvasPart::Ticks);
    assert_eq!(
        local_rect(&canvas, dial, CanvasPart::Ticks),
        [8.0, 8.0, 64.0, 64.0]
    );
    assert_eq!(ticks.fill, CanvasShapeFill::Solid(srgb(LINE_COLOUR)));
    assert_eq!(ticks.border_width, 4.0);
    let cells = 10.0 / 0.75;
    let [start, sweep, dashes, _] = arc(&ticks);
    assert!(
        close(
            &[start, sweep, dashes],
            &[0.625 - 0.5 / cells, 0.75 + 1.0 / cells, cells]
        ),
        "{:?}",
        arc(&ticks)
    );

    // Inside a quarter-em gap, the value ring's centre line is 22 from the
    // centre: the idle-line track over the whole sweep and the 4-unit lit
    // value arc from the minimum to the middle share it.
    let track = part(dial, CanvasPart::Track);
    assert_eq!(
        local_rect(&canvas, dial, CanvasPart::Track),
        [17.375, 17.375, 45.25, 45.25]
    );
    assert_eq!(
        (track.fill, track.border_width),
        (CanvasShapeFill::Solid(srgb(LINE_COLOUR)), 1.25)
    );
    assert!(
        close(&arc(&track)[..2], &[0.625, 0.75]),
        "{:?}",
        arc(&track)
    );
    let value = part(dial, CanvasPart::Fill);
    assert_eq!(
        local_rect(&canvas, dial, CanvasPart::Fill),
        [16.0, 16.0, 48.0, 48.0]
    );
    assert_eq!(
        (value.fill, value.border_width, value.glow),
        (CanvasShapeFill::Solid(srgb(ACCENT)), 4.0, None)
    );
    assert!(
        close(&arc(&value)[..2], &[0.625, 0.375]),
        "{:?}",
        arc(&value)
    );

    // The pointer, a lit stroke 2 thick in the ring's square, points at the
    // value: straight up at the middle of the range, from 0.4 of the ring's
    // radius out to the ring.
    let pointer = part(dial, CanvasPart::Icon);
    assert_eq!(
        local_rect(&canvas, dial, CanvasPart::Icon),
        [18.0, 18.0, 44.0, 44.0]
    );
    assert_eq!(
        (pointer.fill, pointer.border_width),
        (CanvasShapeFill::Solid(srgb(ACCENT)), 2.0)
    );
    let CanvasBoxShape::Stroke {
        segments,
    } = pointer.shape
    else {
        panic!("the pointer is a stroke: {:?}", pointer.shape);
    };
    assert!(close(&segments[0], &[0.5, 0.3, 0.5, 0.0]), "{segments:?}");

    // The bipolar dial at a quarter of its range fills back to zero at
    // twelve, and its pointer turns a quarter of the 270 degrees from half
    // past seven: 67.5 degrees left of twelve.
    let value = part(bipolar, CanvasPart::Fill);
    assert!(
        close(&arc(&value)[..2], &[0.8125, 0.1875]),
        "{:?}",
        arc(&value)
    );
    let CanvasBoxShape::Stroke {
        segments,
    } = part(bipolar, CanvasPart::Icon).shape
    else {
        panic!("the pointer is a stroke");
    };
    let (left, up) = 67.5_f32.to_radians().sin_cos();
    let at = |radius: f32| [0.5 - radius * left, 0.5 - radius * up];
    let ([x0, y0], [x1, y1]) = (at(0.2), at(0.5));
    assert!(close(&segments[0], &[x0, y0, x1, y1]), "{segments:?}");

    // Focus lights the housing's own border; a press, the drag, lights its
    // edge without filling it and glows the pointer as the moving part.
    panel.focus(dial, true);
    panel.frame();
    assert_eq!(
        local_rect(&panel.canvas(), dial, CanvasPart::FocusRing),
        [0.0, 0.0, 80.0, 80.0]
    );
    let mut lease = None;
    panel.feedback(dial, &mut lease, GuiInteractionUpdate::Hover(true));
    panel.feedback(dial, &mut lease, GuiInteractionUpdate::Press);
    panel.frame();
    let canvas = panel.canvas();
    let housing = painted_box(part_of(&canvas, dial, CanvasPart::Background));
    assert_eq!(housing.fill, CanvasShapeFill::Solid(srgb(SURFACE)));
    assert_eq!(
        (housing.border_color, housing.glow),
        (srgb(ACCENT), edge_glow(0.02))
    );
    let pointer = painted_box(part_of(&canvas, dial, CanvasPart::Icon));
    assert_eq!(pointer.glow, part_glow(0.02, 8.0));

    // Disabled keeps the value and its arc, drawn in `neutral`.
    panel.insert(
        bipolar,
        ComponentValue::GuiBehavior(GuiBehavior {
            enabled: false,
            ..Default::default()
        }),
    );
    panel.frame();
    let canvas = panel.canvas();
    let value = painted_box(part_of(&canvas, bipolar, CanvasPart::Fill));
    assert_eq!(value.fill, CanvasShapeFill::Solid(srgb(NEUTRAL)));
    assert!(
        close(&arc(&value)[..2], &[0.8125, 0.1875]),
        "{:?}",
        arc(&value)
    );
    let pointer = painted_box(part_of(&canvas, bipolar, CanvasPart::Icon));
    assert_eq!(pointer.fill, CanvasShapeFill::Solid(srgb(NEUTRAL)));
}

#[test]
fn a_partial_theme_sits_on_the_default_look_property_by_property() {
    let mut panel = Panel::new();
    let root = panel.root_entity;
    panel.font_size(root, 16.0);
    let button = panel.laid_out(
        ComponentValue::GuiButton(GuiButton::default()),
        GuiLayout {
            width: 108.0,
            height: 40.0,
            ..Default::default()
        },
    );
    let red = [1.0, 0.0, 0.0, 1.0];
    let background = GuiPartId::base(GuiPrimitivePart::Background);
    let theme = panel.theme(rows([GuiPaintPart {
        color: Some(red),
        ..row(background)
    }]));
    panel.skin(button, theme, Rows::new());
    panel.present();
    let frame = cut([8.0, 0.0, 8.0, 0.0]);

    // The theme's colour replaces the default interior; the line and the cut
    // stay the default look's.
    let painted = painted_box(part_of(&panel.canvas(), button, CanvasPart::Background));
    assert_eq!(painted.fill, CanvasShapeFill::Solid(red));
    assert_eq!(painted.border_width, 1.25);
    assert_eq!(painted.shape, frame);

    // Every theme row comes before every default row: the theme's base colour
    // also wins over the default pressed fill, while the default pressed rim
    // and glow fill in what the theme leaves absent.
    let mut lease = None;
    panel.feedback(button, &mut lease, GuiInteractionUpdate::Hover(true));
    panel.feedback(button, &mut lease, GuiInteractionUpdate::Press);
    panel.frame();
    let painted = painted_box(part_of(&panel.canvas(), button, CanvasPart::Background));
    assert_eq!(painted.fill, CanvasShapeFill::Solid(red));
    assert_eq!(painted.border_color, srgb(ACCENT));
    assert_eq!(painted.glow, edge_glow(0.02));

    // An override wins over both in every state.
    panel.skin(
        button,
        theme,
        rows([GuiPaintPart {
            corner_cut: Some([0.0; 4]),
            glow_intensity: Some(0.0),
            ..row(background)
        }]),
    );
    panel.frame();
    let painted = painted_box(part_of(&panel.canvas(), button, CanvasPart::Background));
    assert_eq!(painted.shape, cut([0.0; 4]));
    assert_eq!(painted.glow, None);
    assert_eq!(painted.fill, CanvasShapeFill::Solid(red));
}

#[test]
fn the_built_in_switch_look_is_an_ordinary_theme_for_a_checkbox() {
    let mut panel = Panel::new();
    let rail = GuiLayout {
        width: 72.0,
        height: 32.0,
        ..Default::default()
    };
    let off = panel.laid_out(ComponentValue::GuiCheckbox(GuiCheckbox::default()), rail);
    let on = panel.laid_out(
        ComponentValue::GuiCheckbox(GuiCheckbox {
            checked: true,
            ..Default::default()
        }),
        rail,
    );
    let look = ipp_core::systems::gui::gui_skin_looks()
        .iter()
        .find(|look| look.name == "switch")
        .unwrap();
    let theme = panel.theme(rows(look.parts.iter().cloned()));
    panel.skin(off, theme, Rows::new());
    panel.skin(on, theme, Rows::new());
    panel.frame();
    let canvas = panel.canvas();

    // The rail is cut as a frame.
    // It keeps its surface while on, instead of the checkbox's accent fill.
    let painted = painted_box(part_of(&canvas, on, CanvasPart::Background));
    assert_eq!(painted.fill, CanvasShapeFill::Solid(srgb(SURFACE)));
    assert_eq!(painted.shape, cut([8.0, 0.0, 8.0, 0.0]));

    // The block is a part-cut box, not the default check mark: 16 units laid
    // out and scaled by 1.5 to 24, half a rail height from the end it sits at,
    // neutral while off and the accent while on.
    for (entity, x, color) in [(off, 4.0, srgb(NEUTRAL)), (on, 44.0, srgb(ACCENT))] {
        let block = painted_box(part_of(&canvas, entity, CanvasPart::Icon));
        assert_eq!(block.shape, cut([4.0, 0.0, 4.0, 0.0]));
        assert_eq!(block.fill, CanvasShapeFill::Solid(color));
        assert_eq!((block.size, block.scale), ([16.0, 16.0], [1.5, 1.5]));
        assert_eq!(block.border_width, 0.0);
        assert_eq!(local(&canvas, entity, block.position), [x, 4.0]);
    }
}

#[test]
fn default_looks_paint_only_boxes_and_glyphs_and_demand_no_drawing() {
    let mut panel = Panel::new();
    panel.font(10.0);
    let controls = [
        ComponentValue::GuiButton(GuiButton {
            label: "A".into(),
            ..Default::default()
        }),
        ComponentValue::GuiCheckbox(GuiCheckbox {
            label: "A".into(),
            checked: true,
        }),
        ComponentValue::GuiCheckbox(GuiCheckbox::default()),
        ComponentValue::GuiSlider(GuiSlider::default()),
        ComponentValue::GuiTextInput(GuiTextInput {
            text: "A".into(),
            ..Default::default()
        }),
        ComponentValue::GuiScrollView(Default::default()),
    ]
    .map(|control| panel.control(control));
    panel.load(|_| support::canvas_font_bytes(), 1);
    panel.present();

    let only_boxes_and_glyphs = |panel: &Panel, state: &str| {
        let canvas = panel.canvas();
        assert!(!canvas.entries.is_empty());
        for entry in canvas.entries.iter() {
            assert!(
                matches!(
                    primitive(entry),
                    CanvasPrimitive::Box { .. } | CanvasPrimitive::Glyphs { .. }
                ),
                "{state}: {entry:?}"
            );
        }
    };
    only_boxes_and_glyphs(&panel, "idle");
    for &control in &controls {
        let mut lease = None;
        for (update, state) in [
            (GuiInteractionUpdate::Hover(true), "hovered"),
            (GuiInteractionUpdate::Press, "pressed"),
            (GuiInteractionUpdate::Release, "released"),
            (GuiInteractionUpdate::Hover(false), "left"),
        ] {
            panel.feedback(control, &mut lease, update);
            panel.frame();
            only_boxes_and_glyphs(&panel, state);
        }
    }
    for &control in &controls[..5] {
        panel.semantic(control, GuiLocalAction::Focus(0));
        panel.frame();
        only_boxes_and_glyphs(&panel, "focused");
    }
    for &control in &controls {
        panel.insert(
            control,
            ComponentValue::GuiBehavior(GuiBehavior {
                enabled: false,
                ..Default::default()
            }),
        );
    }
    panel.frame();
    only_boxes_and_glyphs(&panel, "disabled");

    // Only the font was ever requested.
    assert!(panel.host.take_resource_requests().is_empty());
}

#[test]
fn default_and_theme_lengths_follow_the_inherited_font_while_overrides_stay_absolute() {
    let mut panel = Panel::new();
    let size = GuiLayout {
        width: 60.0,
        height: 30.0,
        ..Default::default()
    };
    let button = |panel: &mut Panel, font: f32| {
        let button = panel.laid_out(ComponentValue::GuiButton(GuiButton::default()), size);
        panel.font_size(button, font);
        button
    };
    // The button look is drawn at the looks' 16-unit em: twice that doubles
    // every length, and a metre-like 0.16-unit font keeps the same
    // proportions.
    let sheet = button(&mut panel, 16.0);
    let double = button(&mut panel, 32.0);
    let metric = button(&mut panel, 0.16);
    panel.frame();
    let lengths = |canvas: &CanvasPublication, entity| {
        let painted = painted_box(part_of(canvas, entity, CanvasPart::Background));
        let CanvasBoxShape::Rect {
            corner_cut,
            ..
        } = painted.shape
        else {
            panic!("box")
        };
        (painted.border_width, corner_cut)
    };
    let canvas = panel.canvas();
    assert_eq!(lengths(&canvas, sheet), (1.25, [8.0, 0.0, 8.0, 0.0]));
    assert_eq!(lengths(&canvas, double), (2.5, [16.0, 0.0, 16.0, 0.0]));
    let (border, corners) = lengths(&canvas, metric);
    assert!((border - 0.0125).abs() < 1e-6, "{border}");
    assert!((corners[0] - 0.08).abs() < 1e-6, "{corners:?}");

    // A theme without an em keeps its lengths absolute at any font; with one,
    // they follow the font against it. Override rows are the author's own
    // units and are never scaled.
    let background = GuiPartId::base(GuiPrimitivePart::Background);
    let absolute = panel.theme(rows([GuiPaintPart {
        border_width: Some(3.0),
        ..row(background)
    }]));
    let relative = panel.entity(vec![ComponentValue::GuiTheme(GuiTheme {
        parts: rows([GuiPaintPart {
            border_width: Some(3.0),
            corner_cut: Some([2.0, 0.0, 2.0, 0.0]),
            ..row(background)
        }]),
        em: 16.0,
    })]);
    panel.skin(sheet, absolute, Rows::new());
    panel.skin(double, absolute, Rows::new());
    panel.skin(metric, relative, Rows::new());
    panel.frame();
    let canvas = panel.canvas();
    let border =
        |entity| painted_box(part_of(&canvas, entity, CanvasPart::Background)).border_width;
    assert_eq!((border(sheet), border(double)), (3.0, 3.0));
    assert!((border(metric) - 0.03).abs() < 1e-6);
    panel.skin(double, relative, Rows::new());
    panel.frame();
    assert_eq!(
        lengths(&panel.canvas(), double),
        (6.0, [4.0, 0.0, 4.0, 0.0])
    );
    panel.skin(
        double,
        relative,
        rows([GuiPaintPart {
            border_width: Some(2.0),
            ..row(background)
        }]),
    );
    panel.frame();
    let canvas = panel.canvas();
    let painted = painted_box(part_of(&canvas, double, CanvasPart::Background));
    assert_eq!(painted.border_width, 2.0);
    assert_eq!(
        painted.shape,
        cut([4.0, 0.0, 4.0, 0.0]),
        "the override leaves the theme's own lengths scaled"
    );

    // A theme em must be finite and non-negative.
    let outcome = panel.apply(vec![Command::insert_value(
        EntityRef::Handle(relative),
        ComponentValue::GuiTheme(GuiTheme {
            em: -1.0,
            ..Default::default()
        }),
    )]);
    assert!(outcome.result.is_err());
}

#[test]
fn a_canvas_paint_fills_the_background_part_with_its_row_colour_and_nothing_else() {
    let mut panel = Panel::new();
    let amber = [1.0, 0.95, 0.53, 1.0];
    let start = [0.2, 0.4, 0.6, 1.0];
    let paint = || {
        ComponentValue::CanvasPaint(ipp_core::components::CanvasPaint {
            source: "paint:///scanlines".into(),
            ..Default::default()
        })
    };
    // A skinned box whose row names a gradient and a checker: the paint takes the
    // gradient's start colour and replaces both.
    let skinned = panel.entity(vec![
        ComponentValue::CanvasStyle(CanvasStyle {
            x: 200.0,
            y: 50.0,
            ..Default::default()
        }),
        ComponentValue::CanvasBox(CanvasBox {
            width: 30.0,
            height: 12.0,
            ..Default::default()
        }),
        ComponentValue::GuiSkin(GuiSkin {
            parts: rows([base_background(GuiPaintPart {
                color: Some(amber),
                fill_mode: Some(1.0),
                gradient_color0: Some(start),
                checker_size: Some(4.0),
                border_width: Some(2.0),
                border_color: Some([1.0; 4]),
                ..Default::default()
            })]),
            ..Default::default()
        }),
        paint(),
    ]);
    let button = panel.control(ComponentValue::GuiButton(GuiButton::default()));
    panel.insert(button, paint());
    panel.frame();

    let canvas = panel.canvas();
    let painted = painted_box(part_of(&canvas, skinned, CanvasPart::Background));
    let CanvasShapeFill::Paint {
        color,
        paint: target,
    } = painted.fill
    else {
        panic!(
            "a painted background fills with its paint: {:?}",
            painted.fill
        );
    };
    assert_eq!(color, start);
    assert_eq!(target.entity, skinned);
    assert_eq!(target.component, ComponentValue::CANVAS_PAINT);
    assert!(matches!(
        painted.shape,
        CanvasBoxShape::Rect {
            checker: None,
            ..
        }
    ));
    // The border stays the renderer's.
    assert_eq!(painted.border_width, 2.0);

    // Only a control's Background takes its paint.
    let background = painted_box(part_of(&canvas, button, CanvasPart::Background));
    assert!(
        matches!(background.fill, CanvasShapeFill::Paint { paint, .. } if paint.entity == button)
    );
    for entry in canvas.entries.iter() {
        let primitive = primitive(entry);
        let identity = primitive.style().identity;
        if identity.target.entity == button && identity.part != CanvasPart::Background {
            assert!(
                !matches!(
                    primitive,
                    CanvasPrimitive::Box {
                        fill: CanvasShapeFill::Paint { .. },
                        ..
                    }
                ),
                "{:?} must keep its own fill",
                identity.part
            );
        }
    }

    // Each painted part names an instance the publication carries.
    let targets: Vec<_> = canvas
        .paints
        .iter()
        .map(|paint| paint.target.entity)
        .collect();
    assert_eq!(targets.len(), 2);
    assert!(targets.contains(&skinned) && targets.contains(&button));
    assert!(canvas.paints.iter().all(|paint| paint.shader.is_none()));

    // Without its paint the part fills from its row again.
    panel.apply(vec![Command::RemoveComponent {
        entity: EntityRef::Handle(skinned),
        component: ComponentValue::CANVAS_PAINT,
    }]);
    panel.frame();
    let canvas = panel.canvas();
    let restored = painted_box(part_of(&canvas, skinned, CanvasPart::Background));
    assert!(matches!(
        restored.fill,
        CanvasShapeFill::LinearGradient { .. }
    ));
    assert_eq!(canvas.paints.len(), 1);
}

#[test]
fn a_paint_property_write_reaches_the_published_instance_of_a_laid_out_skin() {
    let mut panel = Panel::new();
    let mut paint = ipp_core::components::CanvasPaint {
        source: "paint:///sweep".into(),
        ..Default::default()
    };
    paint
        .properties
        .set("sweep", DynamicValue::F32(0.35))
        .unwrap();
    let skinned = panel.control(ComponentValue::GuiSkin(GuiSkin {
        parts: rows([base_background(GuiPaintPart {
            color: Some([0.1, 0.2, 0.3, 1.0]),
            ..Default::default()
        })]),
        ..Default::default()
    }));
    panel.insert(skinned, ComponentValue::CanvasPaint(paint));
    panel.frame();
    let before = panel.canvas();
    assert_eq!(before.paints[0].properties[0].1, DynamicValue::F32(0.35));

    panel.apply(vec![Command::SetDynamicProperty {
        entity: EntityRef::Handle(skinned),
        component: ComponentValue::CANVAS_PAINT,
        name: "sweep".into(),
        value: DynamicValue::F32(0.75),
    }]);
    panel.frame();
    let after = panel.canvas();
    assert_eq!(after.paints[0].properties[0].1, DynamicValue::F32(0.75));
    assert_ne!(after.paints_revision, before.paints_revision);
}

#[test]
fn a_clip_on_a_paint_property_of_a_laid_out_skin_keeps_reaching_the_instance() {
    use ipp_core::components::schema::FieldValue as SchemaValue;
    use ipp_core::systems::animation::{
        ANIMATION_TYPE, AnimationClip, AnimationControllerDescription, AnimationDriverDescription,
        AnimationInterpolation, AnimationKeyframe, AnimationPlaybackControl, AnimationTrack,
        AnimationTrackTarget, AnimationValue,
    };
    use support::WorldTestDriver;

    let mut panel = Panel::new();
    let mut paint = ipp_core::components::CanvasPaint {
        source: "paint:///sweep".into(),
        ..Default::default()
    };
    paint
        .properties
        .set("sweep", DynamicValue::F32(0.35))
        .unwrap();
    let skinned = panel.control(ComponentValue::GuiSkin(GuiSkin {
        parts: rows([base_background(GuiPaintPart {
            color: Some([0.1, 0.2, 0.3, 1.0]),
            ..Default::default()
        })]),
        ..Default::default()
    }));
    panel.insert(skinned, ComponentValue::CanvasPaint(paint));
    panel.frame();

    let target = AnimationTrackTarget::DynamicProperty {
        component: ComponentValue::CANVAS_PAINT,
        name: "sweep".into(),
    };
    let key = |time: f64, value: f32, interpolation| AnimationKeyframe {
        time,
        value: AnimationValue::Field(SchemaValue::Dynamic(DynamicValue::F32(value))),
        interpolation,
    };
    let clip = AnimationClip::new(
        1.5,
        vec![AnimationTrack {
            target: target.clone(),
            keys: vec![
                key(0.0, 0.1, AnimationInterpolation::Linear),
                key(1.5, 0.85, AnimationInterpolation::Step),
            ],
        }],
    )
    .unwrap();
    let mut context = panel.host.world_mut(panel.world).unwrap();
    context
        .enqueue_asset(ipp_core::services::asset_management::AssetUpload {
            id: 93,
            key: ipp_core::services::asset_management::AssetUploadIdentity {
                kind: ANIMATION_TYPE,
                asset: 93,
                variant: 0,
            },
            bytes: clip.encode(),
        })
        .unwrap();
    assert!(context.await_upload_for_test().assets[0].result.is_ok());
    let controller = context
        .create_animation_controller(AnimationControllerDescription {
            drivers: vec![AnimationDriverDescription {
                source: Arc::<str>::from("asset://10/93"),
                variant: 0,
                track: 0,
                target: skinned,
                property: target,
                entity_bindings: Vec::new(),
                weight: 1.0,
                additive: false,
                reference_time: 0.0,
                repeat: false,
            }],
            ..Default::default()
        })
        .unwrap();
    context
        .enqueue_playback(controller, AnimationPlaybackControl::Play)
        .unwrap();
    drop(context);

    let mut samples = Vec::new();
    for _ in 0..20 {
        panel.frame();
        samples.push(panel.canvas().paints[0].properties[0].1.clone());
    }
    // Every frame publishes the next sample; the clip's change since its first
    // key adds to the authored rest, so the band ends at 0.35 + 0.75.
    assert!(
        samples.windows(2).take(10).all(|pair| pair[0] != pair[1]),
        "{samples:?}"
    );
    let DynamicValue::F32(last) = samples.last().unwrap().clone() else {
        panic!("a float sample");
    };
    assert!((last - 1.1).abs() < 1e-4, "{samples:?}");
}

/// The HSV model on sRGB-encoded values, from the textbook sector formula,
/// decoded to the linear RGB a solid fill or gradient stop carries.
fn hsv_linear(hue: f32, saturation: f32, value: f32) -> [f32; 3] {
    let [h, s, v] = [hue, saturation, value].map(f64::from);
    let sector = h.rem_euclid(1.0) * 6.0;
    let f = sector - sector.floor();
    let [p, q, t] = [1.0 - s, 1.0 - s * f, 1.0 - s * (1.0 - f)].map(|k| v * k);
    let encoded = match sector.floor() as u32 % 6 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    };
    encoded.map(|channel| {
        (if channel <= 0.04045 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }) as f32
    })
}

/// A `120 x 100` colour control at half the language's type, 8 units per em:
/// its surfaces half an em (4) inside, one em (8) apart, rails and swatch 1.5
/// em (12) thick, so the field is `72 x 72`.
fn color_panel(color: GuiColor) -> (Panel, EntityId) {
    let mut panel = Panel::new();
    let control = panel.laid_out(
        ComponentValue::GuiColor(color),
        GuiLayout {
            width: 120.0,
            height: 100.0,
            ..Default::default()
        },
    );
    panel.font_size(control, 8.0);
    panel.frame();
    (panel, control)
}

#[test]
fn a_colour_control_paints_its_surfaces_and_swatch_from_one_value() {
    let (mut panel, color) = color_panel(GuiColor {
        hue: 0.25,
        saturation: 0.5,
        value: 0.75,
        alpha: 0.5,
        alpha_rail: true,
    });
    let canvas = panel.canvas();
    let field = [4.0, 4.0, 72.0, 72.0];
    let hue_rail = [84.0, 4.0, 12.0, 72.0];
    let alpha_rail = [104.0, 4.0, 12.0, 72.0];
    assert_eq!(local_rect(&canvas, color, CanvasPart::Track), field);
    assert_eq!(
        local_rect(&canvas, color, CanvasPart::PartTrack(1)),
        hue_rail
    );
    assert_eq!(
        local_rect(&canvas, color, CanvasPart::PartTrack(2)),
        alpha_rail
    );
    assert_eq!(
        local_rect(&canvas, color, CanvasPart::Fill),
        [4.0, 84.0, 112.0, 12.0]
    );
    // The marker's 6-unit ring centres on saturation 0.5 and value 0.75; each
    // 4-unit thumb bar on its rail's value, rising from the bottom.
    assert_eq!(
        local_rect(&canvas, color, CanvasPart::Marker),
        [37.0, 19.0, 6.0, 6.0]
    );
    assert_eq!(
        local_rect(&canvas, color, CanvasPart::PartIcon(1)),
        [84.0, 56.0, 12.0, 4.0]
    );
    assert_eq!(
        local_rect(&canvas, color, CanvasPart::PartIcon(2)),
        [104.0, 38.0, 12.0, 4.0]
    );

    // The field is the hue's saturation-value field, the hue rail the hue
    // circle from its bottom edge, and the alpha rail the colour from
    // transparent at the bottom to opaque at the top; the swatch is the colour
    // at its alpha. Each surface and the swatch sit in the quiet line, and the
    // alpha rail and swatch over a checker of half an em, which the opaque
    // field and hue rail do not paint.
    let [r, g, b] = hsv_linear(0.25, 0.5, 0.75);
    let painted = |part| painted_box(part_of(&canvas, color, part));
    assert_eq!(
        painted(CanvasPart::Track).fill,
        CanvasShapeFill::SaturationValue {
            hue: 0.25
        }
    );
    assert_eq!(
        painted(CanvasPart::PartTrack(1)).fill,
        CanvasShapeFill::Hue {
            start: [0.0, 72.0],
            end: [0.0, 0.0],
        }
    );
    let CanvasShapeFill::LinearGradient {
        start,
        end,
        start_color,
        end_color,
    } = painted(CanvasPart::PartTrack(2)).fill
    else {
        panic!("the alpha rail is a gradient");
    };
    assert_eq!((start, end), ([0.0, 72.0], [0.0, 0.0]));
    let CanvasShapeFill::Solid(swatch) = painted(CanvasPart::Fill).fill else {
        panic!("the swatch is solid");
    };
    for (actual, expected) in [
        (start_color, [r, g, b, 0.0]),
        (end_color, [r, g, b, 1.0]),
        (swatch, [r, g, b, 0.5]),
    ] {
        for lane in 0..4 {
            assert!((actual[lane] - expected[lane]).abs() < 1e-5, "{actual:?}");
        }
    }
    for (part, checker_size) in [
        (CanvasPart::Track, None),
        (CanvasPart::PartTrack(1), None),
        (CanvasPart::PartTrack(2), Some(4.0)),
        (CanvasPart::Fill, Some(4.0)),
    ] {
        let box_ = painted(part);
        assert_eq!(
            (box_.border_width, box_.border_color),
            (0.625, srgb(LINE_COLOUR)),
            "{part:?}"
        );
        let CanvasBoxShape::Rect {
            corner_cut,
            checker,
            ..
        } = box_.shape
        else {
            panic!("{part:?} is a box");
        };
        assert_eq!(corner_cut, [0.0; 4], "{part:?}");
        assert_eq!(
            checker.map(|checker| checker.size),
            checker_size,
            "{part:?}"
        );
    }
    // The marker is a clear ring, the thumbs clear part-cut bars.
    let marker = painted(CanvasPart::Marker);
    assert_eq!(
        (marker.corner_radius, marker.fill),
        ([3.0; 2], CanvasShapeFill::Solid([0.0; 4]))
    );
    assert!(matches!(
        painted(CanvasPart::PartIcon(1)).shape,
        CanvasBoxShape::Rect {
            corner_cut: [2.0, 0.0, 2.0, 0.0],
            ..
        }
    ));
    assert!(!parts_of(&canvas, color).contains(&CanvasPart::FocusRing));

    // A new hue repaints the field, the rails' thumbs and the swatch in the
    // same publication.
    panel.semantic(color, GuiLocalAction::SetColor([0.6, 0.5, 0.75, 1.0]));
    panel.frame();
    let canvas = panel.canvas();
    let painted = |part| painted_box(part_of(&canvas, color, part));
    assert_eq!(
        painted(CanvasPart::Track).fill,
        CanvasShapeFill::SaturationValue {
            hue: 0.6
        }
    );
    let [r, g, b] = hsv_linear(0.6, 0.5, 0.75);
    let CanvasShapeFill::Solid(swatch) = painted(CanvasPart::Fill).fill else {
        panic!("the swatch is solid");
    };
    for (lane, expected) in [r, g, b, 1.0].into_iter().enumerate() {
        assert!((swatch[lane] - expected).abs() < 1e-5, "{swatch:?}");
    }
    support::gui_panel::assert_near(
        &local_rect(&canvas, color, CanvasPart::PartIcon(1)),
        &[84.0, 4.0 + 0.4 * 72.0 - 2.0, 12.0, 4.0],
    );
    assert_eq!(
        local_rect(&canvas, color, CanvasPart::PartIcon(2)),
        [104.0, 2.0, 12.0, 4.0]
    );
}

#[test]
fn a_colour_control_rings_the_focused_surface_and_lights_the_pressed_one() {
    let (mut panel, color) = color_panel(GuiColor {
        alpha_rail: true,
        ..Default::default()
    });

    // The ring paints on the surface focus names, square and glowing outward.
    for (part, ring) in [
        (0, [4.0, 4.0, 72.0, 72.0]),
        (1, [84.0, 4.0, 12.0, 72.0]),
        (2, [104.0, 4.0, 12.0, 72.0]),
    ] {
        panel.semantic(color, GuiLocalAction::Focus(part));
        panel.frame();
        let canvas = panel.canvas();
        assert_eq!(
            local_rect(&canvas, color, CanvasPart::FocusRing),
            ring,
            "part {part}"
        );
        let glow = painted_box(part_of(&canvas, color, CanvasPart::FocusRing))
            .glow
            .unwrap();
        assert_eq!((glow.radius, glow.inner_radius), (8.0, 0.0));
    }

    // A press on the hue rail lights its edge and nothing of the field; its
    // thumb keeps its outline, legible over any hue.
    panel.present();
    let mut lease: Option<GuiPointerLease> = None;
    for update in [
        GuiInteractionUpdate::Hover(true),
        GuiInteractionUpdate::Press,
    ] {
        let input = panel.routed(color);
        let lease = lease
            .get_or_insert_with(|| panel.input.pointer_lease(&input, 1).unwrap())
            .clone();
        let command = GuiLocalCommand::part_interaction(
            input,
            lease,
            update,
            GuiInteractionPart::FocusPart(1),
        )
        .unwrap();
        panel
            .host
            .world_mut(panel.world)
            .unwrap()
            .enqueue_system_command(GuiSystem::ID, 800, command)
            .unwrap();
    }
    panel.frame();
    let canvas = panel.canvas();
    let border = |part| painted_box(part_of(&canvas, color, part)).border_color;
    assert_eq!(border(CanvasPart::PartTrack(1)), srgb(ACCENT));
    assert_eq!(border(CanvasPart::PartIcon(1)), srgb(0xe5f5f7));
    assert_eq!(border(CanvasPart::Track), srgb(LINE_COLOUR));
    assert_eq!(border(CanvasPart::Marker), srgb(0xe5f5f7));
    // The pressed surface keeps its colours.
    assert!(matches!(
        painted_box(part_of(&canvas, color, CanvasPart::PartTrack(1))).fill,
        CanvasShapeFill::Hue { .. }
    ));
}

#[test]
fn a_disabled_colour_control_dims_its_surfaces_and_keeps_its_swatch() {
    let (mut panel, color) = color_panel(GuiColor {
        hue: 0.5,
        saturation: 1.0,
        value: 1.0,
        alpha: 1.0,
        alpha_rail: false,
    });
    panel.insert(
        color,
        ComponentValue::GuiBehavior(GuiBehavior {
            enabled: false,
            ..Default::default()
        }),
    );
    panel.frame();
    let canvas = panel.canvas();
    let painted = |part| painted_box(part_of(&canvas, color, part));
    // Without the alpha rail the field widens to the hue rail and the swatch
    // is the opaque colour.
    assert_eq!(
        local_rect(&canvas, color, CanvasPart::Track),
        [4.0, 4.0, 92.0, 72.0]
    );
    assert!(!parts_of(&canvas, color).contains(&CanvasPart::PartTrack(2)));
    assert_eq!(painted(CanvasPart::Track).opacity, 0.4);
    assert_eq!(painted(CanvasPart::PartTrack(1)).opacity, 0.4);
    assert_eq!(painted(CanvasPart::Fill).opacity, 1.0);
    let CanvasShapeFill::Solid(swatch) = painted(CanvasPart::Fill).fill else {
        panic!("the swatch is solid");
    };
    support::gui_panel::assert_near(&swatch, &[0.0, 1.0, 1.0, 1.0]);
    for part in [CanvasPart::Marker, CanvasPart::PartIcon(1)] {
        assert_eq!(painted(part).border_color, srgb(NEUTRAL), "{part:?}");
    }
}
