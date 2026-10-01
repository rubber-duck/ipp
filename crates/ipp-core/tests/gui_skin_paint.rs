//! Ordinary control skin paint: theme and override resolution, shape materials, skin
//! assets and interaction states observed in the real Host's Canvas publication.
//!
//! Expectations are hand-computed from the skin rules in `docs/architecture/gui.md`
//! and the control geometry constants, never read back from the implementation.

mod support;

use ipp_core::components::rows::Rows;
use ipp_core::components::{
    GuiBehavior, GuiButton, GuiCheckbox, GuiLayout, GuiSlider, GuiTextInput,
};
use ipp_core::services::asset_management::AssetSource;
use ipp_core::services::asset_management::drawing::DRAWING_TYPE;
use ipp_core::services::gui_input::{
    GuiDeliveryError, GuiDeliveryPermit, GuiDeliveryTerminal, GuiInputContext, GuiInputService,
    GuiInputSession, GuiPointerLease,
};
use ipp_core::systems::canvas::{
    CanvasPaintEntry, CanvasPart, CanvasPrimitive, CanvasPublication, CanvasShapeFill,
    CanvasShapeGlow,
};
use ipp_core::systems::gui::local::{
    GuiInteractionUpdate, GuiLocalAction, GuiLocalCommand, GuiLocalEffect,
};
use ipp_core::systems::gui::presentation::{GuiFont, GuiPaintPart, GuiSkin, GuiTheme};
use ipp_core::systems::gui::{
    FOCUS_BORDER_WIDTH, GuiPartId, GuiPartVariant, GuiPrimitivePart, GuiSkinState, GuiSystem,
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
        let parent = self.root_entity;
        create(
            &mut self.host,
            self.world,
            vec![
                value,
                ComponentValue::GuiLayout(GuiLayout {
                    width: 50.0,
                    height: 20.0,
                    ..Default::default()
                }),
            ],
            Some(parent),
        )
    }

    fn theme(&mut self, parts: Rows<GuiPaintPart>) -> EntityId {
        self.entity(vec![ComponentValue::GuiTheme(GuiTheme {
            parts,
        })])
    }

    fn skin(&mut self, entity: EntityId, theme: EntityId, parts: Rows<GuiPaintPart>) {
        self.insert(
            entity,
            ComponentValue::GuiSkin(GuiSkin {
                theme,
                parts,
                ..Default::default()
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
        let command = GuiLocalCommand::focus(input, visible).unwrap();
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
    let linear = GuiPaintPart {
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
        falloff: 1.5,
    });
    let canvas = panel.canvas();
    let painted = painted_box(part_of(&canvas, button, CanvasPart::Background));
    // Layout supplies the 50 x 20 shape independently of its corner and border.
    assert_eq!(painted.size, [50.0, 20.0]);
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
        // Checked-variant icons belong to checkboxes; plain controls never
        // paint an icon from variant-only rows.
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
    panel.semantic(button, GuiLocalAction::Focus);
    panel.frame();

    let canvas = panel.canvas();
    assert_eq!(
        parts_of(&canvas, button),
        [
            CanvasPart::Background,
            CanvasPart::Label,
            CanvasPart::FocusRing
        ]
    );
    // An empty text input publishes an empty label run, and neither plain
    // control paints an icon from variant-only rows.
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
    assert_eq!(ring.border_width, FOCUS_BORDER_WIDTH);
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
    assert_eq!(untouched.border_width, 0.0);
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
    // The 50 x 20 rail carries a 15 x 15 thumb whose centre travels from 7.5
    // to 42.5; the 5-high track sits at y = 7.5. The fill starts flush with the
    // rail and ends under the thumb centre.
    let mut identity = None;
    for value in [0.0_f32, 0.5, 1.0] {
        if value > 0.0 {
            panel.replace(slider, ControlValue::Scalar(value));
            panel.frame();
        }
        let canvas = panel.canvas();
        let fill = part_of(&canvas, slider, CanvasPart::Fill);
        let painted = painted_box(fill);
        assert_eq!(painted.position, [0.0, 7.5]);
        assert_eq!(painted.size, [7.5 + value * 35.0, 5.0], "value={value}");
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
    let theme = panel.theme(rows([
        GuiPaintPart {
            asset: Some(asset(ipp_core::TEXTURE_TYPE, "gui-skin:///panel.ippt")),
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
fn control_backgrounds_keep_a_default_fill_and_transparent_colours_stay_transparent() {
    let mut panel = Panel::new();
    let button = panel.control(ComponentValue::GuiButton(GuiButton::default()));
    let background = GuiPartId::base(GuiPrimitivePart::Background);
    let theme = panel.theme(rows([GuiPaintPart {
        opacity: Some(0.5),
        ..row(background)
    }]));
    panel.skin(button, theme, Rows::new());
    panel.frame();
    // Unlike the removed node lane, an ordinary control always paints its
    // background: an opacity-only row fades the default fill.
    let painted = painted_box(part_of(&panel.canvas(), button, CanvasPart::Background));
    assert_eq!(
        painted.fill,
        CanvasShapeFill::Solid([0.16, 0.16, 0.16, 1.0])
    );
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
