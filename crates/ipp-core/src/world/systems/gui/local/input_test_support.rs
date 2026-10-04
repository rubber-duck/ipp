use super::*;
use crate::services::gui_input::*;
use crate::{HostRuntime, WorldId};
use std::cell::{Cell, RefCell};
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

pub(super) type Deliveries = Rc<RefCell<Vec<(WorldId, u64, GuiDeliveryTerminal)>>>;

pub(super) struct GuiTestHost {
    pub host: HostRuntime,
    pub service: Rc<GuiInputService>,
    pub session: GuiInputSession,
    pub deliveries: Deliveries,
    next: Cell<u64>,
    /// Submitted `GuiAction` batches by World, request, target and action.
    pub(super) actions: Vec<(WorldId, u64, GuiEntityTarget, GuiLocalAction)>,
    /// Outcomes of the action batches that frames applied.
    pub(super) action_outcomes: Vec<(WorldId, crate::BatchOutcome)>,
    /// One effect subscription per World that received an action.
    observers: std::collections::BTreeMap<WorldId, GuiTestObserver>,
    /// Published effects not yet matched to an action.
    pub(super) effects: Vec<GuiLocalEffect>,
}

/// Batch identities of `GuiAction` test batches start here, above the ones
/// ordinary test batches use.
pub(super) const ACTION_BATCHES: u64 = 1 << 32;

struct GuiTestObserver {
    output: crate::systems::gui::observations::GuiObservationOutput,
    _subscription: crate::systems::gui::observations::GuiObservationSubscription,
}

impl Default for GuiTestHost {
    fn default() -> Self {
        let service = Rc::new(GuiInputService::default());
        let session = service.open_session().unwrap();
        Self {
            host: crate::test_task_scheduler::host(),
            service,
            session,
            deliveries: Rc::default(),
            next: Cell::new(0),
            actions: Vec::new(),
            action_outcomes: Vec::new(),
            observers: Default::default(),
            effects: Vec::new(),
        }
    }
}

impl Deref for GuiTestHost {
    type Target = HostRuntime;

    fn deref(&self) -> &Self::Target {
        &self.host
    }
}

impl DerefMut for GuiTestHost {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.host
    }
}

impl GuiTestHost {
    /// Run one Host frame, keeping the outcomes of `GuiAction` batches and the
    /// effects published to this host's observers.
    pub fn frame_report(&mut self, dt: f64) -> crate::HostFrameReport {
        self.host.progress_assets();
        crate::test_task_scheduler::poll_ready();
        self.host.progress_assets();
        let mut report = self.host.frame(dt).unwrap();
        for (&world, result) in &mut report.worlds {
            if let Ok(world_report) = result {
                world_report.outcomes.retain(|outcome| {
                    if outcome.batch_id < ACTION_BATCHES {
                        return true;
                    }
                    self.action_outcomes.push((world, outcome.clone()));
                    false
                });
            }
        }
        for observer in self.observers.values() {
            while let Some(delivery) = observer.output.pop_front() {
                let (record, lease) = delivery.into_parts();
                if let crate::systems::gui::observations::GuiObservationRecord::Effect {
                    effect,
                    ..
                } = record
                {
                    self.effects.push(GuiLocalEffect::clone(&effect));
                }
                drop(lease);
            }
        }
        report
    }

    /// Queue one `GuiAction` batch; the next frame applies it.
    pub fn submit_action(
        &mut self,
        world: WorldId,
        target: GuiEntityTarget,
        action: GuiLocalAction,
    ) {
        self.observe(world);
        let request = self.next_request();
        self.actions.push((world, request, target, action.clone()));
        self.host
            .world_mut(world)
            .unwrap()
            .enqueue(crate::Batch {
                id: ACTION_BATCHES + request,
                operations: vec![crate::Command::GuiAction {
                    target: crate::GuiActionTarget {
                        entity: crate::EntityRef::Handle(target.entity),
                        component: target.component,
                        incarnation: target.incarnation,
                    },
                    action,
                }],
            })
            .unwrap();
    }

    /// Subscribe to every published effect of `world`, as a client does.
    fn observe(&mut self, world: WorldId) {
        use crate::services::reliable_output::{OutputCharge, OutputLimits, ReliableOutputAccount};
        use crate::systems::gui::observations::*;

        if self.observers.contains_key(&world) {
            return;
        }
        let output = GuiObservationOutput::new(
            ReliableOutputAccount::new(OutputLimits {
                bytes: 1 << 24,
                reply_reserve: 0,
            }),
            GuiObservationEncoding {
                control_bytes: 64,
                effect_bytes: 128,
                ancestry_entry_bytes: 16,
                text_byte_bytes: 1,
            },
        )
        .unwrap();
        let reference = self.host.world_ref(world).unwrap();
        let subscription = output
            .new_subscription(reference, GuiObservationClasses::All)
            .unwrap();
        let lease = output
            .account()
            .reserve(OutputCharge {
                entries: 1,
                bytes: 0,
            })
            .unwrap();
        let command =
            GuiObservationCommand::prepare_subscribe(&output, reference, &subscription, 1, lease)
                .unwrap();
        self.host
            .world_mut(world)
            .unwrap()
            .enqueue_system_command(crate::systems::gui::GuiSystem::ID, 0, command)
            .unwrap();
        self.observers.insert(
            world,
            GuiTestObserver {
                output,
                _subscription: subscription,
            },
        );
    }

    pub fn next_request(&self) -> u64 {
        self.next.set(self.next.get() + 1);
        self.next.get()
    }

    pub fn permit(&self, world: WorldId, request: u64) -> Box<dyn GuiDeliveryPermit> {
        Box::new(ObservedPermit {
            deliveries: self.deliveries.clone(),
            world,
            request,
        })
    }
}

struct ObservedPermit {
    deliveries: Deliveries,
    world: WorldId,
    request: u64,
}

impl GuiDeliveryPermit for ObservedPermit {
    fn prepare(&mut self, _: Option<&GuiLocalEffect>) -> Result<(), GuiDeliveryError> {
        Ok(())
    }

    fn settle(self: Box<Self>, terminal: GuiDeliveryTerminal) {
        self.deliveries
            .borrow_mut()
            .push((self.world, self.request, terminal));
    }
}

/// One settled request: the momentary effect it published, if any, or the
/// reason its batch or ticket was refused.
pub(super) struct GuiTestOutcome {
    pub result: Result<Option<GuiLocalEffect>, crate::ErrorReason>,
}

impl GuiTestOutcome {
    /// The published momentary effect of an applied request.
    pub fn effect(&self) -> &GuiLocalEffect {
        self.result
            .as_ref()
            .unwrap()
            .as_ref()
            .expect("momentary effect")
    }

    /// The published momentary effect of an applied request, by value.
    pub fn into_effect(self) -> GuiLocalEffect {
        self.result.unwrap().expect("momentary effect")
    }
}

/// Logical width and height of the routed fixture's root Canvas, which is
/// presented on a viewport of the same number of pixels.
pub(super) const ROUTED_EXTENT: f32 = 100.0;

/// Logical units per em of the routed fixture's font.
pub(super) const ROUTED_FONT_SIZE: f32 = 10.0;

/// One presented Canvas World with a ready fixture font and a composed
/// physical routing context, so pointer, keyboard and native text input reach
/// ordinary controls through the production router and local mutation boundary.
pub(super) struct GuiRoutedHost {
    pub gui: GuiTestHost,
    pub world: crate::WorldRef,
    pub root: crate::OutputRef,
    /// The top-level column root of the canvas.
    pub root_entity: crate::EntityId,
    pub router: router::GuiInputRouter,
    pub context: router::GuiRoutingContext,
}

impl GuiRoutedHost {
    /// A canvas with a column-laid-out top-level root carrying the fixture font.
    pub fn new() -> Self {
        use crate::systems::canvas::test_support::CanvasTestHost;
        use crate::systems::gui::layout::GuiLayout;
        use crate::systems::gui::presentation::GuiFont;
        use crate::{ComponentValue, WorldViewport};

        let mut gui = GuiTestHost::default();
        gui.register_stream_resource_provider("gui-font").unwrap();
        let world = gui
            .create_world(
                Default::default(),
                &[
                    crate::systems::animation::AnimationSystem::ID,
                    crate::systems::asset_dependencies::AssetDependencySystem::ID,
                    crate::systems::canvas::CanvasSystem::ID,
                    crate::systems::gui::GuiSystem::ID,
                    crate::systems::gui::GuiLayoutSystem::ID,
                ],
            )
            .unwrap();
        let world = gui.world_ref(world).unwrap();
        let root_entity = routed_create(
            &mut gui,
            world,
            None,
            vec![
                ComponentValue::GuiLayout(GuiLayout {
                    kind: 2,
                    width: ROUTED_EXTENT,
                    height: ROUTED_EXTENT,
                    ..Default::default()
                }),
                ComponentValue::GuiFont(GuiFont {
                    source: "gui-font:///body.ippf".into(),
                    variant: 0,
                    font_size: ROUTED_FONT_SIZE,
                }),
            ],
        );
        let root = gui.canvas_output(world, [ROUTED_EXTENT, ROUTED_EXTENT], 1.0);
        gui.set_root_output(
            root,
            WorldViewport {
                width: ROUTED_EXTENT as u32,
                height: ROUTED_EXTENT as u32,
                device_pixel_ratio: 1.0,
            },
        )
        .unwrap();
        routed_frame(&mut gui);
        let mut requests = Vec::new();
        for _ in 0..8 {
            requests.extend(gui.take_resource_requests());
            if !requests.is_empty() {
                break;
            }
            routed_frame(&mut gui);
        }
        assert_eq!(requests.len(), 1, "the root font is requested once");
        gui.complete_resource(
            requests[0].id,
            Ok(crate::world::systems::gui::test_support::font_fixture_bytes()),
        )
        .unwrap();
        for _ in 0..4 {
            routed_frame(&mut gui);
        }
        let router = router::GuiInputRouter::default();
        let (context, _) = router.bind(&gui, world, 900, Vec::new()).unwrap();
        Self {
            gui,
            world,
            root,
            root_entity,
            router,
            context,
        }
    }

    /// Create one entity under the root Canvas and settle its first frame.
    pub fn create(&mut self, values: Vec<crate::ComponentValue>) -> crate::EntityId {
        let entity = routed_create(&mut self.gui, self.world, Some(self.root_entity), values);
        self.frame();
        entity
    }

    /// Apply ordinary authoring operations to the fixture World.
    pub fn apply(&mut self, operations: Vec<crate::Command>) {
        routed_apply(&mut self.gui, self.world, operations);
    }

    pub fn frame(&mut self) {
        routed_frame(&mut self.gui);
    }

    /// Normalized viewport position of a root-Canvas logical point.
    pub fn point(&self, logical: [f32; 2]) -> [f32; 2] {
        logical.map(|value| value / ROUTED_EXTENT)
    }

    /// The presented root view.
    fn view(&self) -> crate::ViewDescriptor {
        self.gui
            .resolve_view(crate::ViewQueryTarget::RootView {
                output: self.root,
                expected_viewport: crate::WorldViewport {
                    width: ROUTED_EXTENT as u32,
                    height: ROUTED_EXTENT as u32,
                    device_pixel_ratio: 1.0,
                },
            })
            .unwrap()
    }

    /// The adapter's routing boundary without input: it may adopt focus a
    /// command set, applied by the next frame.
    pub fn synchronize(&mut self) -> router::GuiRoutingCancellation {
        let view = self.view();
        self.router
            .synchronize(&mut self.gui.host, &mut self.context, Some(view))
    }

    /// Replace the input context, closing its session as an adapter does.
    pub fn rebind(&mut self) {
        let (fresh, _) = self
            .router
            .bind(&self.gui, self.world, 900, Vec::new())
            .unwrap();
        let old = std::mem::replace(&mut self.context, fresh);
        self.router.release(&mut self.gui.host, old);
    }

    /// Route one input against the current completed view without a frame,
    /// so several inputs can reach the same local mutation boundary.
    pub fn send(
        &mut self,
        input: router::GuiPhysicalInput,
    ) -> Result<router::GuiRoutingDisposition, GuiInputError> {
        let view = self.view();
        let mut delivery = RoutedDelivery {
            deliveries: self.gui.deliveries.clone(),
            world: self.world.id(),
        };
        self.router.route(
            &mut self.gui.host,
            &mut self.context,
            view,
            input,
            &mut delivery,
        )
    }

    /// Route one input and apply it in the next frame.
    pub fn route(
        &mut self,
        input: router::GuiPhysicalInput,
    ) -> Result<router::GuiRoutingDisposition, GuiInputError> {
        let routed = self.send(input);
        self.frame();
        routed
    }

    /// Route a native edit stamped with `fence` and apply it.
    pub fn edit(
        &mut self,
        fence: GuiTextFence,
        edit: GuiTextEdit,
    ) -> Result<router::GuiRoutingDisposition, GuiInputError> {
        self.route(router::GuiPhysicalInput::Text {
            fence,
            edit,
        })
    }

    /// Route a primary tap at a root-Canvas logical point.
    pub fn tap(&mut self, logical: [f32; 2]) {
        let point = self.point(logical);
        for input in [
            router::GuiPhysicalInput::PointerDown {
                pointer: 1,
                point,
                button: router::GuiPhysicalButton::Primary,
            },
            router::GuiPhysicalInput::PointerUp {
                pointer: 1,
                point,
                button: router::GuiPhysicalButton::Primary,
            },
        ] {
            self.route(input).unwrap();
        }
    }

    /// Current native state of this session's physical focus owner.
    pub fn native(&mut self) -> Option<GuiNativeTextState> {
        self.router
            .with_native_text(&mut self.gui.host, &self.context, |state| state.cloned())
    }

    /// Read a control's fields and GUI System query records.
    pub fn snapshot(
        &mut self,
        entity: crate::EntityId,
    ) -> crate::systems::gui::test_support::GuiControlRead {
        crate::systems::gui::test_support::read_control(
            &self.gui.world_mut(self.world.id()).unwrap(),
            entity,
        )
        .unwrap()
    }

    /// Completed root Canvas output of the latest World publication.
    pub fn canvas(&self) -> crate::systems::canvas::CanvasPublication {
        self.gui
            .publication(self.gui.latest_publication(self.world.id()).unwrap())
            .unwrap()
            .output(self.root)
            .unwrap()
            .data::<crate::systems::canvas::CanvasPublication>()
            .unwrap()
            .clone()
    }

    /// Settled terminals of routed, semantic and replacement requests, in order.
    pub fn terminals(&mut self) -> Vec<GuiDeliveryTerminal> {
        self.gui
            .deliveries
            .borrow_mut()
            .drain(..)
            .map(|(_, _, terminal)| terminal)
            .collect()
    }
}

fn routed_apply(
    host: &mut HostRuntime,
    world: crate::WorldRef,
    operations: Vec<crate::Command>,
) -> Vec<(u32, crate::EntityId)> {
    super::local_tests::submit(host, world.id(), operations)
        .result
        .unwrap()
}

fn routed_create(
    host: &mut HostRuntime,
    world: crate::WorldRef,
    parent: Option<crate::EntityId>,
    values: Vec<crate::ComponentValue>,
) -> crate::EntityId {
    use crate::{Command, EntityPlacementRef, EntityRef};

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
    routed_apply(host, world, operations)[0].1
}

fn routed_frame(host: &mut GuiTestHost) {
    let report = host.frame_report(0.0);
    assert!(report.worlds.values().all(Result::is_ok), "{report:?}");
    assert!(report.publication_errors.is_empty());
}

struct RoutedDelivery {
    deliveries: Deliveries,
    world: WorldId,
}

impl router::GuiRoutingDelivery for RoutedDelivery {
    fn command(&mut self) -> Result<Box<dyn GuiDeliveryPermit>, GuiInputError> {
        Ok(Box::new(RoutedPermit(ObservedPermit {
            deliveries: self.deliveries.clone(),
            world: self.world,
            request: 0,
        })))
    }
}

/// A physical adapter's permit: it also reserves native-buffer responses.
struct RoutedPermit(ObservedPermit);

impl GuiDeliveryPermit for RoutedPermit {
    fn prepare_native(&mut self, _: &GuiNativeTextState) -> Result<(), GuiDeliveryError> {
        Ok(())
    }

    fn prepare(&mut self, effect: Option<&GuiLocalEffect>) -> Result<(), GuiDeliveryError> {
        self.0.prepare(effect)
    }

    fn settle(self: Box<Self>, terminal: GuiDeliveryTerminal) {
        Box::new(self.0).settle(terminal);
    }
}
