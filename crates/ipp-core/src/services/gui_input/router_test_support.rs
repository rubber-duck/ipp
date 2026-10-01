//! Physical-routing harness shared by the ported router suites: one bound
//! context over a presented root, routed against completed publications.

use super::*;
use crate::components::{Camera, Transform};
use crate::services::gui_input::router::*;
pub(super) use crate::systems::gui::test_support::{GuiControlRead, GuiTestValue};

pub(super) struct Ledger(pub(super) Rc<RefCell<Delivery>>);

impl GuiRoutingDelivery for Ledger {
    fn command(&mut self) -> Result<Box<dyn GuiDeliveryPermit>, GuiInputError> {
        Ok(Permit::boxed(&self.0))
    }
}

/// A router bound to one presented root output.
pub(super) struct Rig {
    pub(super) host: HostRuntime,
    pub(super) root: OutputRef,
    pub(super) viewport: WorldViewport,
    pub(super) router: GuiInputRouter,
    pub(super) context: Option<GuiRoutingContext>,
    pub(super) ledger: Rc<RefCell<Delivery>>,
    pub(super) blockers: Vec<super::query::GuiPickingBlocker>,
}

impl Rig {
    /// Present `root` at `viewport`, complete a frame and bind a context.
    pub(super) fn new(host: HostRuntime, root: OutputRef, viewport: WorldViewport) -> Self {
        let mut rig = Self {
            host,
            root,
            viewport,
            router: GuiInputRouter::default(),
            context: None,
            ledger: Rc::default(),
            blockers: Vec::new(),
        };
        rig.present(viewport);
        rig
    }

    /// The first top-level entity of the presented World: the canvas root of
    /// a [`canvas_root`] scene.
    pub(super) fn root_entity(&mut self) -> EntityId {
        self.host
            .world_mut(self.root.world().id())
            .unwrap()
            .entity_children(None)
            .next()
            .unwrap()
    }

    /// Replace the presented viewport, releasing the old context first as a
    /// physical adapter must, then bind a fresh one.
    pub(super) fn present(&mut self, viewport: WorldViewport) {
        if let Some(context) = self.context.take() {
            self.router.release(&mut self.host, context);
        }
        self.viewport = viewport;
        self.host.set_root_output(self.root, viewport).unwrap();
        self.frame();
        self.rebind();
    }

    /// Bind a fresh context with the current explicit blockers.
    pub(super) fn rebind(&mut self) {
        if let Some(context) = self.context.take() {
            self.router.release(&mut self.host, context);
        }
        let (context, _) = self
            .router
            .bind(&self.host, self.root.world(), 900, self.blockers.clone())
            .unwrap();
        self.context = Some(context);
    }

    pub(super) fn query(&self) -> crate::ViewQueryTarget {
        crate::ViewQueryTarget::RootView {
            output: self.root,
            expected_viewport: self.viewport,
        }
    }

    pub(super) fn view(&self) -> crate::ViewDescriptor {
        self.host.resolve_view(self.query()).unwrap()
    }

    pub(super) fn frame(&mut self) {
        let report = self.host.frame(0.0).unwrap();
        assert!(
            report.worlds.values().all(Result::is_ok),
            "{:?}",
            report.worlds
        );
    }

    /// Route against the latest completed publication without advancing it.
    pub(super) fn route(
        &mut self,
        input: GuiPhysicalInput,
    ) -> Result<GuiRoutingDisposition, GuiInputError> {
        let view = self.view();
        let mut delivery = Ledger(self.ledger.clone());
        self.router.route(
            &mut self.host,
            self.context.as_mut().unwrap(),
            view,
            input,
            &mut delivery,
        )
    }

    /// Route one input, then complete the frame that applies it.
    pub(super) fn send(&mut self, input: GuiPhysicalInput) -> GuiRoutingDisposition {
        let disposition = self.route(input).unwrap();
        self.frame();
        disposition
    }

    /// Cancel stale physical ownership at the adapter's routing boundary.
    pub(super) fn synchronize(&mut self) -> GuiRoutingCancellation {
        let view = self.host.resolve_view(self.query()).ok();
        self.router
            .synchronize(&self.host, self.context.as_mut().unwrap(), view)
    }

    pub(super) fn wheel_remainder(&self) -> [f32; 2] {
        self.router
            .scroll_remainder(self.context.as_ref().unwrap())
            .unwrap()
            .remaining()
            .unwrap()
    }

    /// Read a control's fields and GUI System query records.
    pub(super) fn snapshot(&mut self, world: WorldRef, entity: EntityId) -> GuiControlRead {
        self.read(world, entity).unwrap()
    }

    /// Read a control's fields and GUI System query records, if it is a control.
    pub(super) fn read(&mut self, world: WorldRef, entity: EntityId) -> Option<GuiControlRead> {
        crate::systems::gui::test_support::read_control(
            &self.host.world_mut(world.id()).unwrap(),
            entity,
        )
    }

    /// Evaluated Canvas-local logical bounds of a control, from `CanvasStyle`.
    pub(super) fn bounds(&mut self, world: WorldRef, entity: EntityId) -> [f32; 4] {
        self.snapshot(world, entity).bounds
    }

    /// Normalized root viewport point at a fraction of a root-Canvas control's
    /// published bounds.
    pub(super) fn point_in(&mut self, entity: EntityId, fraction: [f32; 2]) -> [f32; 2] {
        let bounds = self.bounds(self.root.world(), entity);
        self.logical([
            bounds[0] + bounds[2] * fraction[0],
            bounds[1] + bounds[3] * fraction[1],
        ])
    }

    /// Normalized root viewport point of a root-Canvas logical point.
    pub(super) fn logical(&self, point: [f32; 2]) -> [f32; 2] {
        let extent = self.canvas(self.root).logical_extent;
        [point[0] / extent[0], point[1] / extent[1]]
    }

    pub(super) fn value(&mut self, world: WorldRef, entity: EntityId) -> GuiTestValue {
        self.snapshot(world, entity).value
    }

    pub(super) fn scroll(&mut self, world: WorldRef, entity: EntityId) -> [f32; 2] {
        match self.value(world, entity) {
            GuiTestValue::Scroll(offset) => offset,
            other => panic!("expected a scrolling control, got {other:?}"),
        }
    }

    /// The one logically focused control among `controls`, if any.
    pub(super) fn focused(
        &mut self,
        controls: &[(WorldRef, EntityId)],
    ) -> Option<(WorldRef, EntityId)> {
        let mut focused = None;
        for &(world, entity) in controls {
            if self
                .read(world, entity)
                .is_some_and(|snapshot| snapshot.focused)
            {
                assert!(
                    focused.replace((world, entity)).is_none(),
                    "two focused controls"
                );
            }
        }
        focused
    }

    pub(super) fn rejected(&self) -> Vec<GuiInputError> {
        terminals(&self.ledger)
            .into_iter()
            .filter_map(|terminal| match terminal {
                GuiDeliveryTerminal::Rejected(error) => Some(error),
                _ => None,
            })
            .collect()
    }

    /// Control value actions applied by routed input, in order: the control
    /// and whether its value field changed. Scrolling is not a control value
    /// here; the values are in the fields.
    pub(super) fn commits(&self) -> Vec<(EntityId, bool)> {
        terminals(&self.ledger)
            .into_iter()
            .filter_map(|terminal| match terminal {
                GuiDeliveryTerminal::Written {
                    target,
                    changed,
                    ..
                } if !matches!(
                    target.component,
                    ComponentValue::GUI_SCROLL_VIEW | ComponentValue::GUI_VIRTUAL_LIST
                ) =>
                {
                    Some((target.entity, changed))
                }
                _ => None,
            })
            .collect()
    }

    /// Canvas publication of one presented output in the latest publication.
    pub(super) fn canvas(&self, output: OutputRef) -> &crate::systems::canvas::CanvasPublication {
        let publication = self.host.latest_publication(output.world().id()).unwrap();
        self.host
            .output(publication, output)
            .unwrap()
            .data::<crate::systems::canvas::CanvasPublication>()
            .unwrap()
    }

    /// Release the context; every reserved delivery must have settled.
    pub(super) fn finish(mut self) {
        if let Some(context) = self.context.take() {
            self.router.release(&mut self.host, context);
        }
        self.frame();
        assert_eq!(self.ledger.borrow().reserved, 0);
    }
}

pub(super) fn press(pointer: u64, point: [f32; 2]) -> GuiPhysicalInput {
    GuiPhysicalInput::PointerDown {
        button: GuiPhysicalButton::Primary,
        pointer,
        point,
    }
}

pub(super) fn release(pointer: u64, point: [f32; 2]) -> GuiPhysicalInput {
    GuiPhysicalInput::PointerUp {
        button: GuiPhysicalButton::Primary,
        pointer,
        point,
    }
}

pub(super) fn movement(pointer: u64, point: [f32; 2]) -> GuiPhysicalInput {
    GuiPhysicalInput::PointerMove {
        pointer,
        point,
    }
}

pub(super) fn wheel(point: [f32; 2], delta: [f32; 2]) -> GuiPhysicalInput {
    GuiPhysicalInput::Wheel {
        point,
        delta,
    }
}

pub(super) fn key(key: GuiPhysicalKey) -> GuiPhysicalInput {
    GuiPhysicalInput::Key {
        key,
    }
}

/// Explicit logical layout box; every other layout field keeps its default.
pub(super) fn sized(kind: u32, width: f32, height: f32) -> ComponentValue {
    ComponentValue::GuiLayout(GuiLayout {
        kind,
        width,
        height,
        ..Default::default()
    })
}

/// The canvas of `world` at `width` x `height` logical units and one unit per
/// metre, with a top-level column root; returns the canvas output and the root.
pub(super) fn canvas_root(
    host: &mut HostRuntime,
    world: WorldRef,
    width: f32,
    height: f32,
) -> (OutputRef, EntityId) {
    let output = canvas_extent(host, world, width, height);
    let entity = create(host, world, vec![sized(2, width, height)], None);
    (output, entity)
}

/// Set the stored extent of `world`'s canvas at one unit per metre, at the
/// next mutation boundary, and return the canvas output.
pub(super) fn canvas_extent(
    host: &mut HostRuntime,
    world: WorldRef,
    width: f32,
    height: f32,
) -> OutputRef {
    host.world_mut(world.id())
        .unwrap()
        .enqueue_canvas_state_update(crate::CanvasStateUpdate {
            extent: Some([width, height]),
            units_per_metre: Some(1.0),
        })
        .unwrap();
    OutputRef::canvas(world)
}

/// A presented viewport of `width` x `height` pixels at unit density.
pub(super) fn viewport(width: u32, height: u32) -> WorldViewport {
    WorldViewport {
        width,
        height,
        device_pixel_ratio: 1.0,
    }
}

/// A root Camera World looking down -Z from `z = 10`.
pub(super) fn camera_root(host: &mut HostRuntime, world: WorldRef, lens: Camera) -> OutputRef {
    let entity = create(
        host,
        world,
        vec![
            ComponentValue::Camera(lens),
            ComponentValue::Transform(Transform {
                z: 10.0,
                ..Default::default()
            }),
        ],
        None,
    );
    host.bind_output(world, entity, OutputKind::Camera).unwrap()
}

/// A child Canvas World presented on a `4 x 3` metre Surface in its parent,
/// at one logical unit per metre.
pub(super) struct Panel {
    pub(super) output: OutputRef,
    pub(super) anchor: EntityId,
    /// Controls in tree order; the Canvas root itself when it is the control.
    pub(super) controls: Vec<EntityId>,
}

impl Panel {
    pub(super) fn world(&self) -> WorldRef {
        self.output.world()
    }

    /// Every control of this panel with its World, for focus lookups.
    pub(super) fn targets(&self) -> Vec<(WorldRef, EntityId)> {
        self.controls
            .iter()
            .map(|&entity| (self.world(), entity))
            .collect()
    }
}

/// A panel whose Canvas root is `control`, filling the whole `4 x 3` panel.
pub(super) fn panel(
    host: &mut HostRuntime,
    parent: WorldRef,
    transform: Transform,
    control: ComponentValue,
) -> Panel {
    let child = world(host);
    let output = canvas_extent(host, child, 4.0, 3.0);
    let root = create(host, child, vec![sized(2, 4.0, 3.0), control], None);
    let anchor = attach(host, parent, output, [4.0, 3.0], transform);
    Panel {
        output,
        anchor,
        controls: vec![root],
    }
}

/// A panel whose Canvas root is a column of `count` checkboxes, each `4 x 1`.
pub(super) fn column_panel(
    host: &mut HostRuntime,
    parent: WorldRef,
    transform: Transform,
    count: usize,
) -> Panel {
    let child = world(host);
    let (output, root) = canvas_root(host, child, 4.0, 3.0);
    let controls = (0..count)
        .map(|_| {
            create(
                host,
                child,
                vec![
                    ComponentValue::GuiCheckbox(Default::default()),
                    sized(0, 4.0, 1.0),
                ],
                Some(root),
            )
        })
        .collect();
    let anchor = attach(host, parent, output, [4.0, 3.0], transform);
    Panel {
        output,
        anchor,
        controls,
    }
}

/// Present `output` on a Surface of `extent` metres at `transform` in `parent`.
pub(super) fn attach(
    host: &mut HostRuntime,
    parent: WorldRef,
    output: OutputRef,
    extent: [f32; 2],
    transform: Transform,
) -> EntityId {
    let surface = Surface {
        width: extent[0],
        height: extent[1],
    };
    create(
        host,
        parent,
        vec![
            ComponentValue::Transform(transform),
            ComponentValue::Surface(surface),
            ComponentValue::WorldAttachment(WorldAttachment::surface(output)),
        ],
        None,
    )
}

/// Placement facing +Z at `(x, y, z)`.
pub(super) fn at(x: f32, y: f32, z: f32) -> Transform {
    Transform {
        x,
        y,
        z,
        ..Default::default()
    }
}

/// Placement turned half a revolution about +Y, so its front faces -Z.
pub(super) fn turned(x: f32, y: f32, z: f32) -> Transform {
    Transform {
        qy: 1.0,
        qw: 0.0,
        ..at(x, y, z)
    }
}

/// Replace an entity's Transform.
pub(super) fn place_at(host: &mut HostRuntime, world: WorldRef, entity: EntityId, t: Transform) {
    apply(
        host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(entity),
            ComponentValue::Transform(t),
        )],
    );
}
