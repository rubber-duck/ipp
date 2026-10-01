use super::*;
use crate::components::{GuiButton, GuiLayout, Surface};
use crate::systems::canvas::test_support::CanvasTestHost;
use crate::systems::gui::local::{
    GuiEntityTarget, GuiLocalEffect, GuiLocalEffectKind, GuiLocalEffectSource,
};
use crate::systems::*;
use crate::{
    Batch, Command, ComponentValue, EntityId, EntityPlacementRef, EntityRef, ErrorReason,
    HostRuntime, OutputKind, OutputRef, WorldAttachment, WorldRef, WorldViewport,
};
use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

#[path = "pointer_tests.rs"]
mod pointer_tests;

#[path = "router_tests.rs"]
mod router_tests;

#[path = "scroll_router_tests.rs"]
mod scroll_router_tests;

#[path = "router_test_support.rs"]
mod router_test_support;

#[path = "pointer_routing_tests.rs"]
mod pointer_routing_tests;

#[path = "keyboard_traversal_tests.rs"]
mod keyboard_traversal_tests;

#[path = "camera_routing_tests.rs"]
mod camera_routing_tests;

#[path = "scroll_chain_tests.rs"]
mod scroll_chain_tests;

#[path = "query_bound_tests.rs"]
mod query_bound_tests;

const PROBE: SystemId = SystemId("fixture.gui-input-boundary");

/// Canvas Worlds in the gui_input suites: panels with GUI controls and layout,
/// asset-backed fonts, themes and skins, and canvases presenting them on
/// Surface slots, plus the boundary probe.
const SUITE_SYSTEMS: &[SystemId] = &[
    world_attachment::WorldAttachmentSystem::ID,
    animation::AnimationSystem::ID,
    asset_dependencies::AssetDependencySystem::ID,
    hierarchy::HierarchySystem::ID,
    look_at::LookAtSystem::ID,
    hierarchy::FinalPropagationSystem::ID,
    geometry::GeometrySystem::ID,
    camera::CameraSystem::ID,
    surface::SurfaceSystem::ID,
    canvas::CanvasSystem::ID,
    gui::GuiSystem::ID,
    gui::GuiLayoutSystem::ID,
    PROBE,
];

/// A 3D World presenting panels on spatially placed Surfaces under a camera: it
/// does not select the Canvas System, whose Worlds place every Surface as a
/// canvas slot.
const SCENE_SYSTEMS: &[SystemId] = &[
    world_attachment::WorldAttachmentSystem::ID,
    animation::AnimationSystem::ID,
    asset_dependencies::AssetDependencySystem::ID,
    hierarchy::HierarchySystem::ID,
    look_at::LookAtSystem::ID,
    hierarchy::FinalPropagationSystem::ID,
    geometry::GeometrySystem::ID,
    camera::CameraSystem::ID,
    surface::SurfaceSystem::ID,
    PROBE,
];

#[test]
fn initial_failure_returns_the_existing_response_capacity_to_the_adapter() {
    let (host, _, _, target, ledger) = fixture();
    let service = GuiInputService::new(GuiInputLimits {
        pending: 0,
        ..Default::default()
    });
    let context = bind(&host, &service, target.world);
    let failed = service
        .reserve_routed(&host, &context, target, 1, &[], Permit::boxed(&ledger))
        .err()
        .unwrap();
    assert_eq!(failed.reason, GuiInputError::Capacity);
    assert_eq!(ledger.borrow().reserved, 1);
    let (reason, permit) = failed.into_parts();
    permit.settle(GuiDeliveryTerminal::Rejected(reason));
    assert_eq!(ledger.borrow().reserved, 0);
    assert_eq!(
        terminals(&ledger),
        [GuiDeliveryTerminal::Rejected(GuiInputError::Capacity)]
    );
}

#[test]
fn reentrant_revocation_during_exact_effect_preparation_prevents_commit() {
    let (mut host, service, context, target, ledger) = fixture();
    let service = Rc::new(service);
    let session = session(&context);
    let command = reserve(&host, &service, &context, target, &ledger);
    let revoke_service = service.clone();
    let revoke_session = session.clone();
    ledger.borrow_mut().callback_effect_only = true;
    ledger.borrow_mut().callback = Some(Box::new(move || {
        revoke_service.close_session(&revoke_session);
    }));
    queue(&mut host, command);
    host.frame(0.0).unwrap();
    assert_eq!(ledger.borrow().preparations, [false, true]);
    assert!(terminals(&ledger).is_empty());
    assert_eq!(ledger.borrow().reserved, 0);
    assert_eq!(service.pending_count(), 0);
}

#[test]
fn cancellation_after_prepare_is_rechecked_before_the_local_commit_closure() {
    let mut scene = scene();
    scene.state.lock().unwrap().reject_prepared = true;
    let command = routed(&scene);
    queue(&mut scene.host, command);
    scene.host.frame(0.0).unwrap();
    assert_eq!(scene.state.lock().unwrap().prepared, 0);
    assert_eq!(
        terminals(&scene.ledger),
        [GuiDeliveryTerminal::Rejected(GuiInputError::Cancelled)]
    );
    assert_eq!(scene.ledger.borrow().reserved, 0);
}

struct Resource;

impl crate::services::asset_management::Asset for Resource {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn decoded(&self) -> &dyn Any {
        self
    }

    fn resident_bytes(&self) -> usize {
        1
    }
}

#[test]
fn revoked_exact_resource_cannot_be_authorized_by_retained_source_identity() {
    use crate::services::asset_management::{
        AssetTypeId, AssetUploadIdentity, BufferedAssetLoader,
    };
    let mut scene = scene();
    let kind = AssetTypeId(65000);
    scene
        .host
        .asset_resources_mut()
        .register_loader(kind, || BufferedAssetLoader::new(|_| Ok(Resource)))
        .unwrap();
    let key = scene
        .host
        .asset_resources_mut()
        .upload(
            AssetUploadIdentity {
                kind,
                asset: 1,
                variant: 0,
            },
            vec![1],
        )
        .unwrap();
    scene.host.progress_assets();
    scene.state.lock().unwrap().retained = Some(key);
    scene.host.frame(0.0).unwrap();
    let command = routed(&scene);
    scene.host.asset_resources_mut().revoke_resource(key);
    queue(&mut scene.host, command);
    scene.host.frame(0.0).unwrap();
    assert_eq!(
        terminals(&scene.ledger),
        [GuiDeliveryTerminal::Rejected(GuiInputError::StalePath)]
    );
}

#[test]
fn pending_payloads_keep_metadata_budget_until_drop_even_after_cancellation() {
    let (host, _, _, target, ledger) = fixture();
    let service = GuiInputService::new(GuiInputLimits {
        pending: 1,
        ..Default::default()
    });
    let context = bind(&host, &service, target.world);
    let command = reserve(&host, &service, &context, target, &ledger);
    command.reject(GuiInputError::Unavailable);
    assert_eq!(service.pending_count(), 0);
    assert!(matches!(
        service.reserve_routed(&host, &context, target, 100, &[], Permit::boxed(&ledger)),
        Err(GuiInputReservationError {
            reason: GuiInputError::Capacity,
            ..
        })
    ));
    drop(command);
    let next = reserve(&host, &service, &context, target, &ledger);
    drop(next);
    assert_eq!(ledger.borrow().reserved, 0);
}

#[test]
fn current_hidden_or_zero_opacity_parent_rejects_old_eligible_paint() {
    for opacity in [false, true] {
        let mut scene = scene();
        let command = routed(&scene);
        let value = if opacity {
            ComponentValue::CanvasStyle(crate::components::CanvasStyle {
                opacity: 0.0,
                ..Default::default()
            })
        } else {
            ComponentValue::GuiBehavior(crate::components::GuiBehavior {
                visible: false,
                ..Default::default()
            })
        };
        apply(
            &mut scene.host,
            scene.root.world(),
            vec![Command::insert_value(
                EntityRef::Handle(scene.root_entity),
                value,
            )],
        );
        queue(&mut scene.host, command);
        scene.host.frame(0.0).unwrap();
        assert_eq!(
            terminals(&scene.ledger),
            [GuiDeliveryTerminal::Rejected(GuiInputError::StalePath)]
        );
    }
}

#[derive(Default)]
struct Delivery {
    terminals: Vec<GuiDeliveryTerminal>,
    preparations: Vec<bool>,
    reserved: usize,
    fail_effect: bool,
    callback_effect_only: bool,
    callback: Option<Box<dyn FnOnce()>>,
}

struct Permit(Rc<RefCell<Delivery>>, bool);

impl Permit {
    fn boxed(ledger: &Rc<RefCell<Delivery>>) -> Box<Self> {
        ledger.borrow_mut().reserved += 1;
        Box::new(Self(ledger.clone(), false))
    }
}

impl GuiDeliveryPermit for Permit {
    fn prepare_native(
        &mut self,
        _: &crate::systems::gui::local::GuiNativeTextState,
    ) -> Result<(), GuiDeliveryError> {
        if self.0.borrow().fail_effect {
            Err(GuiDeliveryError::Capacity)
        } else {
            Ok(())
        }
    }

    fn prepare(&mut self, effect: Option<&GuiLocalEffect>) -> Result<(), GuiDeliveryError> {
        let (callback, fail) = {
            let mut ledger = self.0.borrow_mut();
            ledger.preparations.push(effect.is_some());
            let callback = if !ledger.callback_effect_only || effect.is_some() {
                ledger.callback.take()
            } else {
                None
            };
            // The preparation after validation reserves the terminal, with or
            // without an effect; `fail_effect` refuses that one.
            let fail = ledger.fail_effect && self.1;
            self.1 = true;
            (callback, fail)
        };
        if let Some(callback) = callback {
            callback();
        }
        if fail {
            Err(GuiDeliveryError::Capacity)
        } else {
            Ok(())
        }
    }

    fn settle(self: Box<Self>, terminal: GuiDeliveryTerminal) {
        self.0.borrow_mut().terminals.push(terminal);
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        self.0.borrow_mut().reserved -= 1;
    }
}

#[derive(Default)]
struct ProbeState {
    fail_publication: Option<WorldRef>,
    prepared: usize,
    retained: Option<crate::services::asset_management::AssetKey>,
    reject_prepared: bool,
    effect_source: Option<GuiLocalEffectSource>,
    effect_kind: Option<GuiLocalEffectKind>,
}

struct ProbeFactory(Arc<Mutex<ProbeState>>);
struct Probe(Arc<Mutex<ProbeState>>);

impl SystemFactory for ProbeFactory {
    fn id(&self) -> SystemId {
        PROBE
    }

    fn create(&self, _: &mut SystemInitContext<'_>) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(Probe(self.0.clone())))
    }
}

impl System for Probe {
    fn update(&mut self, _: &mut SystemUpdateContext<'_, '_>) {}

    fn command_world_references(&self, command: &dyn Any, visit: &mut dyn FnMut(WorldRef)) {
        if let Some(command) = command.downcast_ref::<GuiInputCommand>() {
            command.world_references(visit);
        }
    }

    fn command(
        &mut self,
        context: &mut SystemCommandContext<'_>,
        _: u64,
        command: &dyn Any,
    ) -> Result<(), ErrorReason> {
        let command = command
            .downcast_ref::<GuiInputCommand>()
            .ok_or(ErrorReason::InvalidValue)?;
        let view = context.host_ingress().unwrap();
        if command.validate(&view).is_err() {
            return Ok(());
        }
        let tick = view.world(command.target().world).unwrap().next_tick();
        let mut ancestry = Vec::new();
        let world = view.world(command.target().world).unwrap();
        let mut ancestor = Some(command.target().entity);
        while let Some(entity) = ancestor {
            ancestry.push(entity);
            ancestor = world.entity_link(entity).and_then(|link| link.parent);
        }
        ancestry.reverse();
        let (source, kind) = {
            let state = self.0.lock().unwrap();
            (
                state.effect_source.unwrap_or(command.effect_source()),
                state
                    .effect_kind
                    .clone()
                    .unwrap_or(GuiLocalEffectKind::Pressed),
            )
        };
        let effect = GuiLocalEffect {
            id: None,
            target: command.target(),
            source,
            tick,
            ancestry: ancestry.into(),
            kind,
        };
        if let Ok(prepared) = command.prepare_effect(effect) {
            if self.0.lock().unwrap().reject_prepared {
                command.reject(GuiInputError::Cancelled);
            }
            let _ = prepared.commit(|| {
                self.0.lock().unwrap().prepared += 1;
            });
        }
        Ok(())
    }

    fn publish_output(
        &self,
        context: &crate::WorldContext<'_>,
        builder: &mut crate::WorldOutputBuilder<'_>,
    ) -> Result<(), ErrorReason> {
        if let Some(key) = self.0.lock().unwrap().retained {
            builder.retain(key);
        }
        if self.0.lock().unwrap().fail_publication == Some(context.world_ref()) {
            Err(ErrorReason::InvalidValue)
        } else {
            Ok(())
        }
    }
}

fn host() -> (HostRuntime, Arc<Mutex<ProbeState>>) {
    let state = Arc::new(Mutex::new(ProbeState::default()));
    let mut factories = compiled_system_factories();
    factories.push(Arc::new(ProbeFactory(state.clone())));
    (
        HostRuntime::with_system_factories(factories).unwrap(),
        state,
    )
}

fn world(host: &mut HostRuntime) -> WorldRef {
    let world = host
        .create_world(Default::default(), SUITE_SYSTEMS)
        .unwrap();
    host.world_ref(world).unwrap()
}

/// A camera World with spatial Surfaces; see [`SCENE_SYSTEMS`].
fn scene_world(host: &mut HostRuntime) -> WorldRef {
    let world = host
        .create_world(Default::default(), SCENE_SYSTEMS)
        .unwrap();
    host.world_ref(world).unwrap()
}

fn batch(host: &mut HostRuntime, world: WorldRef, operations: Vec<Command>) -> crate::BatchOutcome {
    host.world_mut(world.id())
        .unwrap()
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();

    host.frame(0.0)
        .unwrap()
        .worlds
        .remove(&world.id())
        .unwrap()
        .unwrap()
        .outcomes
        .remove(0)
}

fn apply(
    host: &mut HostRuntime,
    world: WorldRef,
    operations: Vec<Command>,
) -> Vec<(u32, EntityId)> {
    batch(host, world, operations).result.unwrap()
}

fn create(
    host: &mut HostRuntime,
    world: WorldRef,
    values: Vec<ComponentValue>,
    parent: Option<EntityId>,
) -> EntityId {
    let mut commands = vec![Command::Create {
        alias: 1,
        metadata: Default::default(),
        adopt: false,
    }];
    commands.extend(
        values
            .into_iter()
            .map(|value| Command::insert_value(EntityRef::Alias(1), value)),
    );
    if let Some(parent) = parent {
        commands.push(Command::PlaceEntity {
            entity: EntityRef::Alias(1),
            placement: EntityPlacementRef {
                parent: Some(EntityRef::Handle(parent)),
                before: None,
            },
        });
    }
    apply(host, world, commands)[0].1
}

fn button(host: &mut HostRuntime, world: WorldRef, parent: Option<EntityId>) -> EntityId {
    create(
        host,
        world,
        vec![
            ComponentValue::GuiButton(GuiButton::default()),
            ComponentValue::GuiLayout(GuiLayout {
                width: 20.0,
                height: 20.0,
                ..Default::default()
            }),
        ],
        parent,
    )
}

fn target(host: &mut HostRuntime, world: WorldRef, entity: EntityId) -> GuiEntityTarget {
    crate::systems::gui::test_support::read_control(&host.world_mut(world.id()).unwrap(), entity)
        .unwrap()
        .target
}

fn queue(host: &mut HostRuntime, command: GuiInputCommand) {
    host.world_mut(command.target().world.id())
        .unwrap()
        .enqueue_system_command(PROBE, 7, command)
        .unwrap();
}

/// Queue a `GuiAction` batch from another client; the next frame applies it.
fn gui_action(
    host: &mut HostRuntime,
    target: GuiEntityTarget,
    action: crate::systems::gui::local::GuiLocalAction,
) {
    host.world_mut(target.world.id())
        .unwrap()
        .enqueue(Batch {
            id: 500,
            operations: vec![Command::GuiAction {
                target: crate::GuiActionTarget {
                    entity: EntityRef::Handle(target.entity),
                    component: target.component,
                    incarnation: target.incarnation,
                },
                action,
            }],
        })
        .unwrap();
}

fn terminals(ledger: &Rc<RefCell<Delivery>>) -> Vec<GuiDeliveryTerminal> {
    ledger.borrow().terminals.clone()
}

/// A presented Canvas World with one button and a routing context bound to
/// its root output.
fn fixture() -> (
    HostRuntime,
    GuiInputService,
    GuiInputContext,
    GuiEntityTarget,
    Rc<RefCell<Delivery>>,
) {
    let (mut host, _) = host();
    let world = world(&mut host);
    let (root, root_entity) = canvas(&mut host, world);
    let entity = button(&mut host, world, Some(root_entity));
    host.set_root_output(
        root,
        WorldViewport {
            width: 128,
            height: 128,
            device_pixel_ratio: 1.0,
        },
    )
    .unwrap();
    host.frame(0.0).unwrap();
    let target = target(&mut host, world, entity);
    let service = GuiInputService::default();
    let context = bind(&host, &service, world);
    (host, service, context, target, Rc::default())
}

/// A context of a fresh session of `service` on the presented `world`.
fn bind(host: &HostRuntime, service: &GuiInputService, world: WorldRef) -> GuiInputContext {
    let session = service.open_session().unwrap();
    service.bind_context(host, &session, world).unwrap().context
}

/// The session owning `context`.
fn session(context: &GuiInputContext) -> GuiInputSession {
    GuiInputSession(context.0.session.clone())
}

fn reserve(
    host: &HostRuntime,
    service: &GuiInputService,
    context: &GuiInputContext,
    target: GuiEntityTarget,
    ledger: &Rc<RefCell<Delivery>>,
) -> GuiInputCommand {
    service
        .reserve_routed(host, context, target, 99, &[], Permit::boxed(ledger))
        .unwrap()
}

#[test]
fn routed_reservation_source_cannot_be_relabelled() {
    let mut scene = scene();
    let command = routed(&scene);
    scene.state.lock().unwrap().effect_source = Some(GuiLocalEffectSource::Semantic);
    queue(&mut scene.host, command);
    scene.host.frame(0.0).unwrap();
    assert_eq!(
        terminals(&scene.ledger),
        [GuiDeliveryTerminal::Rejected(GuiInputError::Unavailable)]
    );
    assert_eq!(scene.ledger.borrow().preparations, [false]);
    assert_eq!(scene.state.lock().unwrap().prepared, 0);
    assert_eq!(scene.ledger.borrow().reserved, 0);
}

#[test]
fn rejection_drop_and_duplicate_request_release_exact_capacity() {
    let (host, service, context, target, ledger) = fixture();
    let command = reserve(&host, &service, &context, target, &ledger);
    let duplicate =
        service.reserve_routed(&host, &context, target, 99, &[], Permit::boxed(&ledger));
    assert!(matches!(
        duplicate,
        Err(GuiInputReservationError {
            reason: GuiInputError::DuplicateRequest,
            ..
        })
    ));
    assert_eq!(ledger.borrow().reserved, 2);
    drop(duplicate);
    assert_eq!(ledger.borrow().reserved, 1);
    command.reject(GuiInputError::Unavailable);
    command.reject(GuiInputError::Cancelled);
    drop(command);
    assert_eq!(
        terminals(&ledger),
        [GuiDeliveryTerminal::Rejected(GuiInputError::Unavailable)]
    );
    assert_eq!(service.pending_count(), 0);
    assert_eq!(ledger.borrow().reserved, 0);
}

#[test]
fn world_destruction_drops_real_queued_ticket_without_a_frame() {
    let (mut host, service, context, target, ledger) = fixture();
    let command = reserve(&host, &service, &context, target, &ledger);
    queue(&mut host, command);
    host.destroy_world(target.world.id());
    assert_eq!(terminals(&ledger), [GuiDeliveryTerminal::Cancelled]);
    assert_eq!(service.pending_count(), 0);
    assert_eq!(ledger.borrow().reserved, 0);
}

#[test]
fn exact_effect_capacity_failure_precedes_any_prepared_effect() {
    let mut scene = scene();
    let ledger = Rc::new(RefCell::new(Delivery {
        fail_effect: true,
        ..Default::default()
    }));
    let command = scene
        .service
        .reserve_routed(
            &scene.host,
            &scene.context,
            scene.target,
            99,
            std::slice::from_ref(&scene.token),
            Permit::boxed(&ledger),
        )
        .unwrap();
    queue(&mut scene.host, command);
    scene.host.frame(0.0).unwrap();
    assert_eq!(scene.state.lock().unwrap().prepared, 0);
    assert_eq!(
        terminals(&ledger),
        [GuiDeliveryTerminal::Rejected(GuiInputError::Delivery(
            GuiDeliveryError::Capacity
        ))]
    );
    assert_eq!(ledger.borrow().reserved, 0);
    assert_eq!(scene.service.pending_count(), 0);
}

#[test]
fn reentrant_session_close_during_prepare_never_borrows_maps_across_callbacks() {
    let (mut host, service, context, target, ledger) = fixture();
    let service = Rc::new(service);
    let session = session(&context);
    let command = reserve(&host, &service, &context, target, &ledger);
    let revoke_service = service.clone();
    let revoke_session = session.clone();
    ledger.borrow_mut().callback = Some(Box::new(move || {
        revoke_service.close_session(&revoke_session);
    }));
    queue(&mut host, command);
    host.frame(0.0).unwrap();
    assert!(terminals(&ledger).is_empty());
    assert_eq!(ledger.borrow().preparations, [false]);
    assert_eq!(ledger.borrow().reserved, 0);
    assert_eq!(service.pending_count(), 0);
    assert!(!session.is_live());
}

struct Scene {
    host: HostRuntime,
    state: Arc<Mutex<ProbeState>>,
    service: GuiInputService,
    session: GuiInputSession,
    context: GuiInputContext,
    root: OutputRef,
    root_entity: EntityId,
    child: OutputRef,
    child_entity: EntityId,
    target: GuiEntityTarget,
    anchor: EntityId,
    token: crate::WorldAttachmentToken,
    ledger: Rc<RefCell<Delivery>>,
}

/// The canvas of `world` with an empty top-level root entity.
fn canvas(host: &mut HostRuntime, world: WorldRef) -> (OutputRef, EntityId) {
    let entity = create(host, world, vec![], None);
    (OutputRef::canvas(world), entity)
}

fn scene() -> Scene {
    let (mut host, state) = host();
    let parent = world(&mut host);
    let child_world = world(&mut host);
    let (root, root_entity) = canvas(&mut host, parent);
    let (child, child_entity) = canvas(&mut host, child_world);
    let button = button(&mut host, child_world, Some(child_entity));
    let surface = Surface {
        width: 100.0,
        height: 100.0,
    };
    let anchor = create(
        &mut host,
        parent,
        vec![
            ComponentValue::Surface(surface),
            ComponentValue::WorldAttachment(WorldAttachment::surface(child)),
        ],
        Some(root_entity),
    );
    host.set_root_output(
        root,
        WorldViewport {
            width: 128,
            height: 128,
            device_pixel_ratio: 1.0,
        },
    )
    .unwrap();
    host.frame(0.0).unwrap();
    let target = target(&mut host, child_world, button);
    let token = host
        .publication(host.latest_publication(parent.id()).unwrap())
        .unwrap()
        .attachments[0]
        .token
        .clone();
    let service = GuiInputService::default();
    let session = service.open_session().unwrap();
    let context = service
        .bind_context(&host, &session, parent)
        .unwrap()
        .context;
    Scene {
        host,
        state,
        service,
        session,
        context,
        root,
        root_entity,
        child,
        child_entity,
        target,
        anchor,
        token,
        ledger: Rc::default(),
    }
}

fn routed(scene: &Scene) -> GuiInputCommand {
    scene
        .service
        .reserve_routed(
            &scene.host,
            &scene.context,
            scene.target,
            99,
            std::slice::from_ref(&scene.token),
            Permit::boxed(&scene.ledger),
        )
        .unwrap()
}

#[test]
fn root_session_close_cancels_child_tickets_capture_and_native_focus_synchronously() {
    let mut scene = scene();
    let command = routed(&scene);
    let mut declarations = Vec::new();
    command.world_references(&mut |world| declarations.push(world));
    assert!(declarations.contains(&scene.root.world()));
    assert!(declarations.contains(&scene.child.world()));
    scene
        .service
        .set_native_focus(&scene.context, Some(scene.target))
        .unwrap();
    scene
        .service
        .set_pointer_capture(&scene.context, 3, Some(scene.target))
        .unwrap();
    queue(&mut scene.host, command);
    let cancellation = scene.service.close_session(&scene.session);
    assert_eq!(
        cancellation,
        [GuiNativeCancellation {
            focus: Some(scene.target),
            captures: vec![(3, scene.target)]
        }]
    );
    assert_eq!(scene.service.pending_count(), 0);
    assert_eq!(scene.ledger.borrow().reserved, 0);
    assert!(terminals(&scene.ledger).is_empty());
    scene.host.frame(0.0).unwrap();
    assert!(scene.ledger.borrow().preparations.is_empty());
    assert!(terminals(&scene.ledger).is_empty());
    let reopened = scene.service.open_session().unwrap();
    assert_ne!(reopened.id(), scene.session.id());
}

#[test]
fn equal_root_rebind_and_equal_context_rebind_fence_old_routed_work() {
    let mut scene = scene();
    let command = routed(&scene);
    scene
        .host
        .set_root_output(scene.root, scene.context.root().viewport)
        .unwrap();
    queue(&mut scene.host, command);
    scene.host.frame(0.0).unwrap();
    assert_eq!(
        terminals(&scene.ledger),
        [GuiDeliveryTerminal::Rejected(GuiInputError::StaleContext)]
    );
    let rebound = scene
        .service
        .bind_context(&scene.host, &scene.session, scene.root.world())
        .unwrap()
        .context;
    assert_ne!(rebound.identity(), scene.context.identity());
    scene.context = rebound;
    let command = routed(&scene);
    let fresh = scene
        .service
        .bind_context(&scene.host, &scene.session, scene.root.world())
        .unwrap()
        .context;
    assert_ne!(fresh.identity(), scene.context.identity());
    assert_eq!(
        terminals(&scene.ledger).last(),
        Some(&GuiDeliveryTerminal::Cancelled)
    );
    drop(command);
    assert_eq!(terminals(&scene.ledger).len(), 2);
}

#[test]
fn presentation_revocation_cycle_cannot_revive_context_with_unchanged_root_binding() {
    let mut scene = scene();
    let root_binding = scene.context.root();
    let original = scene.context.clone();
    let command = routed(&scene);
    queue(&mut scene.host, command);
    scene
        .service
        .set_native_focus(&original, Some(scene.target))
        .unwrap();
    scene
        .service
        .set_pointer_capture(&original, 7, Some(scene.target))
        .unwrap();

    let cancellation = scene.service.release_context(&original);
    assert_eq!(
        cancellation,
        [GuiNativeCancellation {
            focus: Some(scene.target),
            captures: vec![(7, scene.target)]
        }]
    );
    assert_eq!(terminals(&scene.ledger), [GuiDeliveryTerminal::Cancelled]);
    assert_eq!(scene.ledger.borrow().reserved, 0);

    let alternate = scene
        .service
        .bind_context(&scene.host, &scene.session, scene.root.world())
        .unwrap()
        .context;
    scene.service.release_context(&alternate);
    let returned = scene
        .service
        .bind_context(&scene.host, &scene.session, scene.root.world())
        .unwrap()
        .context;
    assert_eq!(
        scene.host.root_output_binding(scene.root.world()).unwrap(),
        Some(root_binding)
    );
    assert_eq!(returned.root(), original.root());
    assert_ne!(returned.identity(), original.identity());
    assert_ne!(returned.identity(), alternate.identity());
    assert!(scene.service.release_context(&original).is_empty());
    assert_eq!(
        scene
            .service
            .set_native_focus(&original, Some(scene.target)),
        Err(GuiInputError::StaleContext)
    );
    assert!(matches!(
        scene.service.reserve_routed(
            &scene.host,
            &original,
            scene.target,
            100,
            &[scene.token.clone()],
            Permit::boxed(&scene.ledger)
        ),
        Err(GuiInputReservationError {
            reason: GuiInputError::StaleContext,
            ..
        })
    ));

    let next_ledger = Rc::default();
    let next = scene
        .service
        .reserve_routed(
            &scene.host,
            &returned,
            scene.target,
            101,
            &[scene.token.clone()],
            Permit::boxed(&next_ledger),
        )
        .unwrap();
    queue(&mut scene.host, next);
    scene.host.frame(0.0).unwrap();
    assert_eq!(terminals(&scene.ledger), [GuiDeliveryTerminal::Cancelled]);
    assert!(scene.ledger.borrow().preparations.is_empty());
    assert!(matches!(
        terminals(&next_ledger).as_slice(),
        [GuiDeliveryTerminal::Applied(_)]
    ));
    assert_eq!(scene.state.lock().unwrap().prepared, 1);
}

fn queue_edits(host: &mut HostRuntime, world: WorldRef, operations: Vec<Command>) {
    host.world_mut(world.id())
        .unwrap()
        .enqueue(Batch {
            id: 17,
            operations,
        })
        .unwrap();
}

fn place(entity: EntityId, parent: EntityId) -> Command {
    Command::PlaceEntity {
        entity: EntityRef::Handle(entity),
        placement: EntityPlacementRef {
            parent: Some(EntityRef::Handle(parent)),
            before: None,
        },
    }
}

fn field(entity: EntityId, component: u16, offset: usize, value: crate::FieldValue) -> Command {
    Command::SetField {
        entity: EntityRef::Handle(entity),
        component,
        field: crate::FieldWrite {
            offset: offset as u32,
            value,
        },
    }
}

#[test]
fn same_canvas_reparent_keeps_routed_authority_and_pins_current_effect_ancestry() {
    let mut scene = scene();
    let parent = create(
        &mut scene.host,
        scene.child.world(),
        vec![],
        Some(scene.child_entity),
    );
    let command = routed(&scene);
    let source = command.routed_proof().source();
    queue_edits(
        &mut scene.host,
        scene.child.world(),
        vec![place(scene.target.entity, parent)],
    );
    queue(&mut scene.host, command);
    scene.host.frame(0.0).unwrap();
    let outcomes = terminals(&scene.ledger);
    let [GuiDeliveryTerminal::Applied(effect)] = outcomes.as_slice() else {
        panic!("expected routed delivery");
    };
    assert_eq!(
        effect.source,
        GuiLocalEffectSource::Routed {
            publication: source
        }
    );
    assert_eq!(
        &*effect.ancestry,
        &[scene.child_entity, parent, scene.target.entity]
    );
    apply(
        &mut scene.host,
        scene.child.world(),
        vec![place(scene.target.entity, scene.child_entity)],
    );
    assert_eq!(
        &*effect.ancestry,
        &[scene.child_entity, parent, scene.target.entity]
    );
}

#[test]
fn current_target_ancestry_hidden_or_zero_opacity_rejects_old_eligible_hit() {
    for at_target in [false, true] {
        for opacity in [false, true] {
            let mut scene = scene();
            let parent = create(
                &mut scene.host,
                scene.child.world(),
                vec![],
                Some(scene.child_entity),
            );
            let command = routed(&scene);
            let value = if opacity {
                ComponentValue::CanvasStyle(crate::components::CanvasStyle {
                    opacity: 0.0,
                    ..Default::default()
                })
            } else {
                ComponentValue::GuiBehavior(crate::components::GuiBehavior {
                    visible: false,
                    ..Default::default()
                })
            };
            let policy_entity = if at_target {
                scene.target.entity
            } else {
                parent
            };
            queue_edits(
                &mut scene.host,
                scene.child.world(),
                vec![
                    place(scene.target.entity, parent),
                    Command::insert_value(EntityRef::Handle(policy_entity), value),
                ],
            );
            queue(&mut scene.host, command);
            scene.host.frame(0.0).unwrap();
            assert_eq!(
                terminals(&scene.ledger),
                [GuiDeliveryTerminal::Rejected(GuiInputError::StalePath)]
            );
            assert_eq!(scene.state.lock().unwrap().prepared, 0);
        }
    }
}

#[test]
fn transparent_tint_preserves_the_published_canvas_hit_policy() {
    let mut scene = scene();
    apply(
        &mut scene.host,
        scene.child.world(),
        vec![Command::insert_value(
            EntityRef::Handle(scene.target.entity),
            ComponentValue::CanvasStyle(crate::components::CanvasStyle {
                alpha: 0.0,
                ..Default::default()
            }),
        )],
    );
    scene.host.frame(0.0).unwrap();
    let command = routed(&scene);
    queue(&mut scene.host, command);
    scene.host.frame(0.0).unwrap();
    assert!(matches!(
        terminals(&scene.ledger).as_slice(),
        [GuiDeliveryTerminal::Applied(_)]
    ));
}

#[test]
fn refused_layout_write_keeps_the_retained_hit_usable() {
    let mut scene = scene();
    let command = routed(&scene);
    queue_edits(
        &mut scene.host,
        scene.child.world(),
        vec![field(
            scene.target.entity,
            ComponentValue::GUI_LAYOUT,
            std::mem::offset_of!(GuiLayout, min_width),
            crate::FieldValue::F32(-1.0),
        )],
    );
    queue(&mut scene.host, command);
    let report = scene.host.frame(0.0).unwrap();
    let outcome = &report.worlds[&scene.child.world().id()]
        .as_ref()
        .unwrap()
        .outcomes[0];
    assert_eq!(
        outcome.result.as_ref().unwrap_err().reason,
        ErrorReason::InvalidValue
    );

    // The invalid write has no effect, so the published hit stays usable.
    assert!(
        scene
            .host
            .world_mut(scene.child.world().id())
            .unwrap()
            .inspect(scene.target.entity)
            .unwrap()
            .components
            .iter()
            .any(|value| matches!(value, ComponentValue::GuiLayout(layout) if layout.min_width >= 0.0))
    );
    assert!(matches!(
        terminals(&scene.ledger).as_slice(),
        [GuiDeliveryTerminal::Applied(_)]
    ));
    assert_eq!(scene.state.lock().unwrap().prepared, 1);
}

#[test]
fn still_available_nonlatest_child_source_remains_valid() {
    let mut scene = scene();
    let command = routed(&scene);
    let old_child = scene
        .host
        .latest_publication(scene.child.world().id())
        .unwrap();
    scene.state.lock().unwrap().fail_publication = Some(scene.root.world());
    scene.host.frame(0.0).unwrap();
    assert_ne!(
        scene.host.latest_publication(scene.child.world().id()),
        Some(old_child)
    );
    assert!(scene.host.publication(old_child).is_some());
    queue(&mut scene.host, command);
    scene.host.frame(0.0).unwrap();
    assert!(matches!(
        terminals(&scene.ledger).as_slice(),
        [GuiDeliveryTerminal::Applied(_)]
    ));
}

#[test]
fn current_surface_removal_and_superseded_pending_token_reject_retained_path() {
    for remove_surface in [true, false] {
        let mut scene = scene();
        let command = routed(&scene);
        let operation = if remove_surface {
            Command::RemoveComponent {
                entity: EntityRef::Handle(scene.anchor),
                component: ComponentValue::SURFACE,
            }
        } else {
            Command::insert_value(
                EntityRef::Handle(scene.anchor),
                ComponentValue::WorldAttachment(WorldAttachment::surface(scene.child)),
            )
        };

        // A failed root publication retains the superseded token's completed path.
        scene.state.lock().unwrap().fail_publication = Some(scene.root.world());
        apply(&mut scene.host, scene.root.world(), vec![operation]);
        assert_eq!(
            scene.host.attachment_retirement(&scene.token).unwrap(),
            crate::WorldAttachmentRetirement::Pending
        );
        queue(&mut scene.host, command);
        scene.host.frame(0.0).unwrap();
        assert_eq!(
            terminals(&scene.ledger),
            [GuiDeliveryTerminal::Rejected(GuiInputError::StalePath)]
        );
    }
}

#[test]
fn metadata_capacity_is_reusable() {
    let (host, _, _, target, ledger) = fixture();
    let service = GuiInputService::new(GuiInputLimits {
        pending: 1,
        ..Default::default()
    });
    let context = bind(&host, &service, target.world);
    let command = reserve(&host, &service, &context, target, &ledger);
    assert!(matches!(
        service.reserve_routed(&host, &context, target, 100, &[], Permit::boxed(&ledger)),
        Err(GuiInputReservationError {
            reason: GuiInputError::Capacity,
            ..
        })
    ));
    drop(command);
    let next = reserve(&host, &service, &context, target, &ledger);
    assert_eq!(service.pending_count(), 1);
    drop(next);
    assert_eq!(
        terminals(&ledger),
        [
            GuiDeliveryTerminal::Cancelled,
            GuiDeliveryTerminal::Cancelled
        ]
    );
    assert_eq!(ledger.borrow().reserved, 0);
}
