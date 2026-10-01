//! Routed scroll-bar, scrolling and drag-gesture behavior against completed publications.

use super::*;
use crate::components::{GuiLayout, GuiVirtualList};
use crate::services::gui_input::router::*;
use crate::systems::gui::test_support::{GuiControlRead, GuiTestValue, read_control};

struct Recorder(Rc<RefCell<Delivery>>);

impl GuiRoutingDelivery for Recorder {
    fn command(&mut self) -> Result<Box<dyn GuiDeliveryPermit>, GuiInputError> {
        Ok(Permit::boxed(&self.0))
    }
}

struct ScrollScene {
    host: HostRuntime,
    world: WorldRef,
    list: EntityId,
    output: OutputRef,
    viewport: WorldViewport,
}

impl ScrollScene {
    /// A 100x50 VirtualList of `items` 10-unit estimates.
    fn new(items: u32) -> Self {
        let (mut host, _) = host();
        let world = world(&mut host);
        let list = create(
            &mut host,
            world,
            vec![
                ComponentValue::GuiLayout(GuiLayout {
                    width: 100.0,
                    height: 50.0,
                    ..Default::default()
                }),
                ComponentValue::GuiVirtualList(GuiVirtualList {
                    item_count: items,
                    item_extent: 10.0,
                    axis: 1,
                    overscan: 0,
                    ..Default::default()
                }),
            ],
            None,
        );
        let output = host.canvas_output(world, [100.0, 50.0], 1.0);
        let viewport = WorldViewport {
            width: 100,
            height: 50,
            device_pixel_ratio: 1.0,
        };
        host.set_root_output(output, viewport).unwrap();
        host.frame(0.0).unwrap();
        Self {
            host,
            world,
            list,
            output,
            viewport,
        }
    }

    fn view(&self) -> crate::ViewDescriptor {
        self.host
            .resolve_view(crate::ViewQueryTarget::RootView {
                output: self.output,
                expected_viewport: self.viewport,
            })
            .unwrap()
    }

    fn route(
        &mut self,
        router: &mut GuiInputRouter,
        context: &mut GuiRoutingContext,
        delivery: &mut Recorder,
        input: GuiPhysicalInput,
    ) -> GuiRoutingDisposition {
        let view = self.view();
        router
            .route(&mut self.host, context, view, input, delivery)
            .unwrap()
    }

    fn snapshot(&mut self) -> GuiControlRead {
        read_control(&self.host.world_mut(self.world.id()).unwrap(), self.list).unwrap()
    }
}

fn press(pointer: u64, point: [f32; 2]) -> GuiPhysicalInput {
    GuiPhysicalInput::PointerDown {
        button: GuiPhysicalButton::Primary,
        pointer,
        point,
    }
}

fn release(pointer: u64, point: [f32; 2]) -> GuiPhysicalInput {
    GuiPhysicalInput::PointerUp {
        button: GuiPhysicalButton::Primary,
        pointer,
        point,
    }
}

fn thumb_move(pointer: u64, y: f32) -> GuiPhysicalInput {
    GuiPhysicalInput::PointerMove {
        pointer,
        point: [0.98, y],
    }
}

/// The list's `offset_x` and `offset_y` fields.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ScrollPosition {
    offset: [f32; 2],
}

fn scroll_position(scene: &mut ScrollScene) -> ScrollPosition {
    let GuiTestValue::Scroll(offset) = scene.snapshot().value else {
        panic!("scroll value");
    };
    ScrollPosition {
        offset,
    }
}

fn rejected(ledger: &Rc<RefCell<Delivery>>) -> Vec<GuiInputError> {
    terminals(ledger)
        .into_iter()
        .filter_map(|terminal| match terminal {
            GuiDeliveryTerminal::Rejected(error) => Some(error),
            _ => None,
        })
        .collect()
}

/// Start a thumb drag on a 100-item list and let one move commit.
fn dragged_list() -> (
    ScrollScene,
    GuiInputRouter,
    GuiRoutingContext,
    Recorder,
    Rc<RefCell<Delivery>>,
) {
    let mut scene = ScrollScene::new(100);
    let mut router = GuiInputRouter::default();
    let (mut context, _) = router
        .bind(&scene.host, scene.world, 900, Vec::new())
        .unwrap();
    let ledger: Rc<RefCell<Delivery>> = Rc::default();
    let mut delivery = Recorder(ledger.clone());
    scene.route(
        &mut router,
        &mut context,
        &mut delivery,
        press(1, [0.98, 0.02]),
    );
    scene.route(&mut router, &mut context, &mut delivery, thumb_move(1, 0.3));
    scene.host.frame(0.0).unwrap();
    assert!(scroll_position(&mut scene).offset[1] > 0.0);
    assert!(rejected(&ledger).is_empty());
    (scene, router, context, delivery, ledger)
}

#[test]
fn thumb_drag_continues_across_layout_anchor_normalization() {
    let (mut scene, mut router, mut context, mut delivery, ledger) = dragged_list();
    let dragged = scroll_position(&mut scene);

    // A realized item above the anchor measures 30 instead of its 10 estimate;
    // layout keeps the visible anchor by writing a normalized offset.
    create(
        &mut scene.host,
        scene.world,
        vec![
            ComponentValue::GuiVirtualItem(crate::components::GuiVirtualItem {
                index: 0,
            }),
            ComponentValue::GuiLayout(GuiLayout {
                width: 100.0,
                height: 30.0,
                ..Default::default()
            }),
        ],
        Some(scene.list),
    );
    scene.host.frame(0.0).unwrap();
    let normalized = scroll_position(&mut scene);
    assert!(
        (normalized.offset[1] - dragged.offset[1] - 20.0).abs() < 0.001,
        "layout normalized the anchored offset"
    );

    scene.route(&mut router, &mut context, &mut delivery, thumb_move(1, 0.5));
    scene.host.frame(0.0).unwrap();
    assert!(rejected(&ledger).is_empty(), "{:?}", rejected(&ledger));
    assert!(scroll_position(&mut scene).offset[1] > normalized.offset[1]);
    scene.route(
        &mut router,
        &mut context,
        &mut delivery,
        release(1, [0.98, 0.5]),
    );
    scene.host.frame(0.0).unwrap();
    assert!(rejected(&ledger).is_empty());
    router.release(&mut scene.host, context);
}

#[test]
fn thumb_drag_after_an_external_scroll_action_applies_from_the_written_offset() {
    let (mut scene, mut router, mut context, mut delivery, ledger) = dragged_list();
    let snapshot = scene.snapshot();
    super::gui_action(
        &mut scene.host,
        snapshot.target,
        crate::systems::gui::local::GuiLocalAction::ScrollTo([0.0; 2]),
    );
    scene.host.frame(0.0).unwrap();
    assert_eq!(scroll_position(&mut scene).offset, [0.0; 2]);

    // Routed input carries no staleness check: the held thumb applies at the
    // mutation boundary after the external write, and the last write wins.
    scene.route(&mut router, &mut context, &mut delivery, thumb_move(1, 0.5));
    scene.host.frame(0.0).unwrap();
    assert!(rejected(&ledger).is_empty(), "{:?}", rejected(&ledger));
    assert!(scroll_position(&mut scene).offset[1] > 0.0);
    router.release(&mut scene.host, context);
}

/// A 20-unit checkbox realized as the list's first item.
fn checkbox_item(scene: &mut ScrollScene) -> EntityId {
    let checkbox = create(
        &mut scene.host,
        scene.world,
        vec![
            ComponentValue::GuiVirtualItem(crate::components::GuiVirtualItem {
                index: 0,
            }),
            ComponentValue::GuiCheckbox(Default::default()),
            ComponentValue::GuiLayout(GuiLayout {
                width: 80.0,
                height: 20.0,
                ..Default::default()
            }),
        ],
        Some(scene.list),
    );
    scene.host.frame(0.0).unwrap();
    checkbox
}

fn checked(scene: &mut ScrollScene, checkbox: EntityId) -> bool {
    read_control(&scene.host.world_mut(scene.world.id()).unwrap(), checkbox)
        .unwrap()
        .value
        == GuiTestValue::Bool(true)
}

/// Hold a tap on the checkbox, wheel over the list, then release in place.
fn wheel_during_held_tap(delta: f32) -> bool {
    let mut scene = ScrollScene::new(10);
    let checkbox = checkbox_item(&mut scene);
    let mut router = GuiInputRouter::default();
    let (mut context, _) = router
        .bind(&scene.host, scene.world, 900, Vec::new())
        .unwrap();
    let ledger: Rc<RefCell<Delivery>> = Rc::default();
    let mut delivery = Recorder(ledger.clone());
    scene.route(
        &mut router,
        &mut context,
        &mut delivery,
        press(1, [0.2, 0.2]),
    );
    scene.host.frame(0.0).unwrap();
    scene.route(
        &mut router,
        &mut context,
        &mut delivery,
        GuiPhysicalInput::Wheel {
            point: [0.2, 0.6],
            delta: [0.0, delta],
        },
    );
    scene.host.frame(0.0).unwrap();
    scene.route(
        &mut router,
        &mut context,
        &mut delivery,
        release(1, [0.2, 0.2]),
    );
    scene.host.frame(0.0).unwrap();
    assert!(rejected(&ledger).is_empty(), "{:?}", rejected(&ledger));
    let offset = scroll_position(&mut scene).offset[1];
    assert_eq!(offset, delta.max(0.0));
    let toggled = checked(&mut scene, checkbox);
    router.release(&mut scene.host, context);
    toggled
}

#[test]
fn wheel_that_moves_content_disarms_a_held_tap() {
    assert!(!wheel_during_held_tap(2.0));
}

#[test]
fn wheel_that_cannot_move_content_keeps_a_held_tap() {
    assert!(wheel_during_held_tap(-2.0));
}

impl ScrollScene {
    /// Skin the list with a theme of `(part, color)` rows.
    fn theme(&mut self, rows: &[(crate::systems::gui::GuiPartId, [f32; 4])]) {
        use crate::components::rows::Rows;
        use crate::systems::gui::presentation::{GuiPaintPart, GuiSkin, GuiTheme};
        let mut parts = Rows::new();
        for (identity, color) in rows {
            parts
                .push(GuiPaintPart {
                    color: Some(*color),
                    ..GuiPaintPart::keyed(*identity).unwrap()
                })
                .unwrap();
        }
        let theme = create(
            &mut self.host,
            self.world,
            vec![ComponentValue::GuiTheme(GuiTheme {
                parts,
            })],
            None,
        );
        apply(
            &mut self.host,
            self.world,
            vec![Command::insert_value(
                EntityRef::Handle(self.list),
                ComponentValue::GuiSkin(GuiSkin {
                    theme,
                    ..Default::default()
                }),
            )],
        );
        self.host.frame(0.0).unwrap();
    }

    /// Solid colour of one painted part of the list.
    fn part_color(&self, part: crate::systems::canvas::CanvasPart) -> [f32; 4] {
        use crate::systems::canvas::{
            CanvasPaintEntry, CanvasPrimitive, CanvasPublication, CanvasShapeFill,
        };
        let view = self.view();
        let canvas = self
            .host
            .output(view.publication, self.output)
            .unwrap()
            .data::<CanvasPublication>()
            .unwrap();
        canvas
            .entries
            .iter()
            .find_map(|entry| match entry.as_ref() {
                CanvasPaintEntry::Primitive {
                    primitive:
                        CanvasPrimitive::Box {
                            style,
                            fill: CanvasShapeFill::Solid(color),
                            ..
                        },
                    ..
                } if style.identity.part == part => Some(*color),
                _ => None,
            })
            .expect("painted part")
    }
}

#[test]
fn scroll_bar_parts_resolve_their_own_hover_and_press() {
    use crate::systems::canvas::CanvasPart;
    use crate::systems::gui::GuiPartId;
    use crate::systems::gui::GuiPrimitivePart::{ScrollThumbY, ScrollTrackY};
    use crate::systems::gui::GuiSkinState::{Hovered, Pressed};
    const THUMB: [f32; 4] = [0.8, 0.1, 0.8, 1.0];
    const THUMB_HOVERED: [f32; 4] = [0.1, 0.8, 0.1, 1.0];
    const TRACK: [f32; 4] = [0.1, 0.8, 0.8, 1.0];
    const TRACK_HOVERED: [f32; 4] = [0.1, 0.1, 0.8, 1.0];
    const TRACK_PRESSED: [f32; 4] = [0.8, 0.8, 0.1, 1.0];
    let mut scene = ScrollScene::new(10);
    scene.theme(&[
        (GuiPartId::base(ScrollThumbY), THUMB),
        (GuiPartId::state(ScrollThumbY, Hovered), THUMB_HOVERED),
        (
            GuiPartId::state(ScrollThumbY, Pressed),
            [0.9, 0.9, 0.9, 1.0],
        ),
        (GuiPartId::base(ScrollTrackY), TRACK),
        (GuiPartId::state(ScrollTrackY, Hovered), TRACK_HOVERED),
        (GuiPartId::state(ScrollTrackY, Pressed), TRACK_PRESSED),
    ]);
    let mut router = GuiInputRouter::default();
    let (mut context, _) = router
        .bind(&scene.host, scene.world, 900, Vec::new())
        .unwrap();
    let ledger: Rc<RefCell<Delivery>> = Rc::default();
    let mut delivery = Recorder(ledger.clone());
    let mut expect = |scene: &mut ScrollScene, input, thumb, track| {
        scene.route(&mut router, &mut context, &mut delivery, input);
        scene.host.frame(0.0).unwrap();
        assert_eq!(
            [
                scene.part_color(CanvasPart::ScrollThumbY),
                scene.part_color(CanvasPart::ScrollTrackY),
            ],
            [thumb, track]
        );
    };
    // Hovering the thumb, then the track below it, names each part alone.
    expect(&mut scene, thumb_move(1, 0.1), THUMB_HOVERED, TRACK);
    expect(&mut scene, thumb_move(1, 0.9), THUMB, TRACK_HOVERED);
    // Pressing the track pages without pressing the thumb.
    expect(&mut scene, press(1, [0.98, 0.9]), THUMB, TRACK_PRESSED);
    assert!(scroll_position(&mut scene).offset[1] > 0.0);
    expect(&mut scene, release(1, [0.98, 0.9]), THUMB, TRACK);
    assert!(rejected(&ledger).is_empty(), "{:?}", rejected(&ledger));
    router.release(&mut scene.host, context);
}

#[test]
fn released_thumb_repaints_idle_in_the_next_frame_after_a_captured_drag() {
    use crate::systems::canvas::CanvasPart;
    use crate::systems::gui::GuiPartId;
    use crate::systems::gui::GuiPrimitivePart::ScrollThumbY;
    use crate::systems::gui::GuiSkinState::Pressed;
    const THUMB: [f32; 4] = [0.8, 0.1, 0.8, 1.0];
    const PRESSED: [f32; 4] = [0.9, 0.9, 0.9, 1.0];
    let mut scene = ScrollScene::new(10);
    scene.theme(&[
        (GuiPartId::base(ScrollThumbY), THUMB),
        (GuiPartId::state(ScrollThumbY, Pressed), PRESSED),
    ]);
    let mut router = GuiInputRouter::default();
    let (mut context, _) = router
        .bind(&scene.host, scene.world, 900, Vec::new())
        .unwrap();
    let ledger: Rc<RefCell<Delivery>> = Rc::default();
    let mut delivery = Recorder(ledger.clone());
    scene.route(&mut router, &mut context, &mut delivery, thumb_move(1, 0.1));
    scene.route(
        &mut router,
        &mut context,
        &mut delivery,
        press(1, [0.98, 0.1]),
    );
    // The captured drag leaves the bar for the content before release.
    for y in [0.2, 0.3, 0.4] {
        scene.route(
            &mut router,
            &mut context,
            &mut delivery,
            GuiPhysicalInput::PointerMove {
                pointer: 1,
                point: [0.5, y],
            },
        );
        scene.host.frame(0.0).unwrap();
    }
    assert_eq!(scene.part_color(CanvasPart::ScrollThumbY), PRESSED);
    scene.route(
        &mut router,
        &mut context,
        &mut delivery,
        release(1, [0.5, 0.4]),
    );
    scene.host.frame(0.0).unwrap();
    let snapshot = scene.snapshot();
    assert!(!snapshot.interaction.pressed && !snapshot.interaction.hovered);
    assert_eq!(scene.part_color(CanvasPart::ScrollThumbY), THUMB);
    assert!(scroll_position(&mut scene).offset[1] > 0.0);
    assert!(rejected(&ledger).is_empty(), "{:?}", rejected(&ledger));
    router.release(&mut scene.host, context);
}

/// A 100x50 slider over 0..=100 in whole steps, bound as the root view.
fn slider_scene() -> ScrollScene {
    let (mut host, _) = host();
    let world = world(&mut host);
    let list = create(
        &mut host,
        world,
        vec![
            ComponentValue::GuiLayout(GuiLayout {
                width: 100.0,
                height: 50.0,
                ..Default::default()
            }),
            ComponentValue::GuiSlider(crate::components::GuiSlider {
                min: 0.0,
                max: 100.0,
                step: 1.0,
                value: 0.0,
            }),
        ],
        None,
    );
    let output = host.canvas_output(world, [100.0, 50.0], 1.0);
    let viewport = WorldViewport {
        width: 100,
        height: 50,
        device_pixel_ratio: 1.0,
    };
    host.set_root_output(output, viewport).unwrap();
    host.frame(0.0).unwrap();
    ScrollScene {
        host,
        world,
        list,
        output,
        viewport,
    }
}

fn slider_value(scene: &mut ScrollScene) -> f32 {
    let GuiTestValue::Scalar(value) = scene.snapshot().value else {
        panic!("slider value");
    };
    value
}

fn slide(x: f32) -> GuiPhysicalInput {
    GuiPhysicalInput::PointerMove {
        pointer: 1,
        point: [x, 0.5],
    }
}

#[test]
fn sustained_slider_drag_chains_pipelined_moves_across_frames() {
    let mut scene = slider_scene();
    let mut router = GuiInputRouter::default();
    let (mut context, _) = router
        .bind(&scene.host, scene.world, 900, Vec::new())
        .unwrap();
    let ledger: Rc<RefCell<Delivery>> = Rc::default();
    let mut delivery = Recorder(ledger.clone());
    scene.route(&mut router, &mut context, &mut delivery, slide(0.1));
    scene.route(
        &mut router,
        &mut context,
        &mut delivery,
        press(1, [0.1, 0.5]),
    );
    // Several moves route against one completed frame before it advances.
    for frame in 0..8 {
        for step in 0..3 {
            let x = 0.1 + 0.03 * (frame * 3 + step) as f32;
            scene.route(&mut router, &mut context, &mut delivery, slide(x));
        }
        scene.host.frame(0.0).unwrap();
    }
    let dragged = slider_value(&mut scene);
    scene.route(
        &mut router,
        &mut context,
        &mut delivery,
        release(1, [0.82, 0.5]),
    );
    scene.host.frame(0.0).unwrap();
    assert!(rejected(&ledger).is_empty(), "{:?}", rejected(&ledger));
    assert!(dragged > 60.0, "{dragged}");
    assert_eq!(slider_value(&mut scene), dragged);
    router.release(&mut scene.host, context);
}

#[test]
fn slider_drag_after_an_external_value_action_applies_in_order() {
    let mut scene = slider_scene();
    let mut router = GuiInputRouter::default();
    let (mut context, _) = router
        .bind(&scene.host, scene.world, 900, Vec::new())
        .unwrap();
    let ledger: Rc<RefCell<Delivery>> = Rc::default();
    let mut delivery = Recorder(ledger.clone());
    scene.route(
        &mut router,
        &mut context,
        &mut delivery,
        press(1, [0.3, 0.5]),
    );
    scene.route(&mut router, &mut context, &mut delivery, slide(0.4));
    scene.host.frame(0.0).unwrap();
    assert!(rejected(&ledger).is_empty(), "{:?}", rejected(&ledger));
    let snapshot = scene.snapshot();
    super::gui_action(
        &mut scene.host,
        snapshot.target,
        crate::systems::gui::local::GuiLocalAction::SetScalar(5.0),
    );
    scene.host.frame(0.0).unwrap();
    assert_eq!(slider_value(&mut scene), 5.0);

    // The held drag has no staleness check: its next move is written after
    // the external value, and the last write wins.
    scene.route(&mut router, &mut context, &mut delivery, slide(0.6));
    scene.host.frame(0.0).unwrap();
    assert!(rejected(&ledger).is_empty(), "{:?}", rejected(&ledger));
    let dragged = slider_value(&mut scene);
    assert!(dragged > 40.0, "{dragged}");
    router.release(&mut scene.host, context);
}
