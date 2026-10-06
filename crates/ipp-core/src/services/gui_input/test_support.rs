//! Fixtures shared by the GUI input suites: a probe System that prepares routed
//! effects, a delivery ledger recording terminals, and presented Canvas Worlds.

use super::{
    GuiDeliveryError, GuiDeliveryPermit, GuiDeliveryTerminal, GuiInputCommand, GuiInputContext,
    GuiInputError, GuiInputService, GuiInputSession,
};
use crate::components::{FlatSurface, GuiButton, GuiLayout};
use crate::systems::gui::local::{
    GuiEntityTarget, GuiLocalEffect, GuiLocalEffectKind, GuiLocalEffectSource,
};
use crate::systems::{
    System, SystemCommandContext, SystemFactory, SystemId, SystemInitContext, SystemInitError,
    SystemUpdateContext, animation, asset_dependencies, camera, canvas, compiled_system_factories,
    geometry, gui, hierarchy, look_at, surface, world_attachment,
};
use crate::{
    Batch, Command, ComponentValue, EntityId, EntityPlacementRef, EntityRef, ErrorReason,
    HostRuntime, OutputRef, WorldAttachment, WorldRef, WorldViewport,
};
use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

pub(super) const PROBE: SystemId = SystemId("fixture.gui-input-boundary");

/// Canvas Worlds in the gui_input suites: panels with GUI controls and layout,
/// asset-backed fonts, themes and skins, and canvases presenting them on
/// Surface slots, plus the boundary probe.
pub(super) const SUITE_SYSTEMS: &[SystemId] = &[
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
pub(super) const SCENE_SYSTEMS: &[SystemId] = &[
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

#[derive(Default)]
pub(super) struct Delivery {
    pub(super) terminals: Vec<GuiDeliveryTerminal>,
    pub(super) preparations: Vec<bool>,
    pub(super) reserved: usize,
    pub(super) fail_effect: bool,
    pub(super) callback_effect_only: bool,
    pub(super) callback: Option<Box<dyn FnOnce()>>,
}

pub(super) struct Permit(pub(super) Rc<RefCell<Delivery>>, pub(super) bool);

impl Permit {
    pub(super) fn boxed(ledger: &Rc<RefCell<Delivery>>) -> Box<Self> {
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
pub(super) struct ProbeState {
    pub(super) fail_publication: Option<WorldRef>,
    pub(super) prepared: usize,
    pub(super) retained: Option<crate::services::asset_management::AssetKey>,
    pub(super) reject_prepared: bool,
    pub(super) effect_source: Option<GuiLocalEffectSource>,
    pub(super) effect_kind: Option<GuiLocalEffectKind>,
}

pub(super) struct ProbeFactory(pub(super) Arc<Mutex<ProbeState>>);

pub(super) struct Probe(pub(super) Arc<Mutex<ProbeState>>);

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

pub(super) fn host() -> (HostRuntime, Arc<Mutex<ProbeState>>) {
    let state = Arc::new(Mutex::new(ProbeState::default()));
    let mut factories = compiled_system_factories();
    factories.push(Arc::new(ProbeFactory(state.clone())));
    (
        crate::test_task_scheduler::with_factories(factories).unwrap(),
        state,
    )
}

pub(super) fn world(host: &mut HostRuntime) -> WorldRef {
    let world = host
        .create_world(Default::default(), SUITE_SYSTEMS)
        .unwrap();
    host.world_ref(world).unwrap()
}

/// A camera World with spatial Surfaces; see [`SCENE_SYSTEMS`].
pub(super) fn scene_world(host: &mut HostRuntime) -> WorldRef {
    let world = host
        .create_world(Default::default(), SCENE_SYSTEMS)
        .unwrap();
    host.world_ref(world).unwrap()
}

pub(super) fn batch(
    host: &mut HostRuntime,
    world: WorldRef,
    operations: Vec<Command>,
) -> crate::BatchOutcome {
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

pub(super) fn apply(
    host: &mut HostRuntime,
    world: WorldRef,
    operations: Vec<Command>,
) -> Vec<(u32, EntityId)> {
    batch(host, world, operations).result.unwrap()
}

pub(super) fn create(
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

pub(super) fn button(
    host: &mut HostRuntime,
    world: WorldRef,
    parent: Option<EntityId>,
) -> EntityId {
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

pub(super) fn target(host: &mut HostRuntime, world: WorldRef, entity: EntityId) -> GuiEntityTarget {
    crate::systems::gui::test_support::read_control(&host.world_mut(world.id()).unwrap(), entity)
        .unwrap()
        .target
}

/// Queue a `GuiAction` batch from another client; the next frame applies it.
pub(super) fn gui_action(
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

pub(super) fn terminals(ledger: &Rc<RefCell<Delivery>>) -> Vec<GuiDeliveryTerminal> {
    ledger.borrow().terminals.clone()
}

/// A presented Canvas World with one button and a routing context bound to
/// its root output.
pub(super) fn fixture() -> (
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
pub(super) fn bind(
    host: &HostRuntime,
    service: &GuiInputService,
    world: WorldRef,
) -> GuiInputContext {
    let session = service.open_session().unwrap();
    service.bind_context(host, &session, world).unwrap().context
}

pub(super) struct Scene {
    pub(super) host: HostRuntime,
    pub(super) state: Arc<Mutex<ProbeState>>,
    pub(super) service: GuiInputService,
    pub(super) session: GuiInputSession,
    pub(super) context: GuiInputContext,
    pub(super) root: OutputRef,
    pub(super) root_entity: EntityId,
    pub(super) child: OutputRef,
    pub(super) child_entity: EntityId,
    pub(super) target: GuiEntityTarget,
    pub(super) anchor: EntityId,
    pub(super) token: crate::WorldAttachmentToken,
    pub(super) ledger: Rc<RefCell<Delivery>>,
}

/// The canvas of `world` with an empty top-level root entity.
pub(super) fn canvas(host: &mut HostRuntime, world: WorldRef) -> (OutputRef, EntityId) {
    let entity = create(host, world, vec![], None);
    (OutputRef::canvas(world), entity)
}

pub(super) fn scene() -> Scene {
    let (mut host, state) = host();
    let parent = world(&mut host);
    let child_world = world(&mut host);
    let (root, root_entity) = canvas(&mut host, parent);
    let (child, child_entity) = canvas(&mut host, child_world);
    let button = button(&mut host, child_world, Some(child_entity));
    let surface = FlatSurface {
        width: 100.0,
        height: 100.0,
        ..Default::default()
    };
    let anchor = create(
        &mut host,
        parent,
        vec![
            ComponentValue::FlatSurface(surface),
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

pub(super) fn routed(scene: &Scene) -> GuiInputCommand {
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

pub(super) fn queue_edits(host: &mut HostRuntime, world: WorldRef, operations: Vec<Command>) {
    host.world_mut(world.id())
        .unwrap()
        .enqueue(Batch {
            id: 17,
            operations,
        })
        .unwrap();
}
