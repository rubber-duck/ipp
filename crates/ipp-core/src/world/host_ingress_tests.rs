use crate::{
    Batch, Command, ComponentValue, EntityId, EntityRef, ErrorReason, HostRuntime, OutputKind,
    OutputRef, PublishedWorldAttachment, RootOutputBinding, WorldAttachment, WorldPublicationId,
    WorldRef, WorldViewport, components::Camera, services::asset_management::*, systems::*,
};
use std::{
    any::Any,
    cell::Cell,
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

/// Worlds in these tests hold cameras and attachments; the probe observes ingress.
const SYSTEMS: &[SystemId] = &[
    world_attachment::WorldAttachmentSystem::ID,
    animation::AnimationSystem::ID,
    asset_dependencies::AssetDependencySystem::ID,
    hierarchy::HierarchySystem::ID,
    look_at::LookAtSystem::ID,
    hierarchy::FinalPropagationSystem::ID,
    geometry::GeometrySystem::ID,
    camera::CameraSystem::ID,
    PROBE,
];

const PROBE: SystemId = SystemId("fixture.host-ingress");

#[derive(Default)]
struct Observations {
    reads: Vec<Observation>,
    retained: Option<AssetKey>,
    publication_failure: Option<WorldRef>,
}

#[derive(Debug)]
struct Observation {
    root: Option<RootOutputBinding>,
    active: bool,
    output_live: bool,
    publication_available: bool,
    resource_available: bool,
    edge_current: bool,
    edge_available: bool,
    fault: Option<ErrorReason>,
}

struct Read {
    world: WorldRef,
    entity: Option<EntityId>,
    output: Option<OutputRef>,
    publication: Option<WorldPublicationId>,
    resource: Option<AssetKey>,
    edge: Option<PublishedWorldAttachment>,
}

impl Read {
    fn world(world: WorldRef) -> Self {
        Self {
            world,
            entity: None,
            output: None,
            publication: None,
            resource: None,
            edge: None,
        }
    }
}

enum Action {
    Read(Box<Read>),
    Mutate(Vec<Command>),
    Revoke(AssetKey),
    Fail,
}

struct Input {
    declared: Vec<WorldRef>,
    action: Action,
    ticket: Option<Ticket>,
}

impl Input {
    fn new(action: Action) -> Self {
        Self {
            declared: Vec::new(),
            action,
            ticket: None,
        }
    }

    fn read(read: Read, declared: Vec<WorldRef>) -> Self {
        Self {
            declared,
            ..Self::new(Action::Read(Box::new(read)))
        }
    }
}

struct ProbeFactory(Arc<Mutex<Observations>>);

struct ProbeSystem(Arc<Mutex<Observations>>);

impl SystemFactory for ProbeFactory {
    fn id(&self) -> SystemId {
        PROBE
    }

    fn create(&self, _: &mut SystemInitContext<'_>) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(ProbeSystem(self.0.clone())))
    }
}

impl System for ProbeSystem {
    fn update(&mut self, _: &mut SystemUpdateContext<'_, '_>) {}

    fn command_world_references(&self, command: &dyn Any, visit: &mut dyn FnMut(WorldRef)) {
        if let Some(input) = command.downcast_ref::<Input>() {
            for world in &input.declared {
                visit(*world);
            }
        }
    }

    fn command(
        &mut self,
        context: &mut SystemCommandContext<'_>,
        _: u64,
        command: &dyn Any,
    ) -> Result<(), ErrorReason> {
        let input = command
            .downcast_ref::<Input>()
            .ok_or(ErrorReason::InvalidValue)?;
        let result = match &input.action {
            Action::Read(read) => {
                let view = context.host_ingress().ok_or(ErrorReason::InvalidValue)?;
                let world = view.world(read.world).ok_or(ErrorReason::InvalidEntity)?;
                let observation = Observation {
                    root: view.root_binding(read.world),
                    active: read.entity.is_some_and(|entity| {
                        world.component_is_active(entity, ComponentValue::CAMERA)
                    }),
                    output_live: read
                        .output
                        .is_some_and(|output| view.output_is_live(output)),
                    publication_available: read
                        .publication
                        .is_some_and(|id| view.publication(id).is_some()),
                    resource_available: read
                        .publication
                        .zip(read.resource)
                        .is_some_and(|(id, key)| view.publication_resource(id, key).is_some()),
                    edge_current: read
                        .edge
                        .as_ref()
                        .is_some_and(|edge| view.attachment_is_current(&edge.token)),
                    edge_available: read
                        .edge
                        .as_ref()
                        .is_some_and(|edge| view.attached_publication(edge).is_some()),
                    fault: world.fault(),
                };
                self.0.lock().unwrap().reads.push(observation);
                Ok(())
            }
            Action::Mutate(commands) => context.world.apply_authored_commands(Some(self), commands),
            Action::Revoke(key) => {
                context.world.asset_acquisition.revoke_resource(*key);
                Ok(())
            }
            Action::Fail => Err(ErrorReason::InvalidValue),
        };
        if let Some(ticket) = &input.ticket {
            ticket.settle(if result.is_ok() {
                Terminal::Applied
            } else {
                Terminal::Rejected
            });
        }
        result
    }

    fn publish_output(
        &self,
        world: &crate::WorldContext<'_>,
        builder: &mut crate::WorldOutputBuilder<'_>,
    ) -> Result<(), ErrorReason> {
        let state = self.0.lock().unwrap();
        if state.publication_failure == Some(world.world_ref()) {
            return Err(ErrorReason::InvalidValue);
        }
        if let Some(key) = state.retained {
            builder.retain(key);
        }
        Ok(())
    }
}

fn fixture() -> (HostRuntime, Arc<Mutex<Observations>>) {
    let shared = Arc::new(Mutex::new(Observations::default()));
    let mut factories = compiled_system_factories();
    factories.push(Arc::new(ProbeFactory(shared.clone())));
    (
        crate::test_task_scheduler::with_factories(factories).unwrap(),
        shared,
    )
}

fn world(host: &mut HostRuntime) -> WorldRef {
    // Hosts without the probe select the same Systems without it.
    let selected: Vec<_> = SYSTEMS
        .iter()
        .copied()
        .filter(|id| host.system_ids().any(|registered| registered == *id))
        .collect();
    let id = host.create_world(Default::default(), &selected).unwrap();
    host.world_ref(id).unwrap()
}

fn apply(
    host: &mut HostRuntime,
    world: WorldRef,
    operations: Vec<Command>,
) -> Vec<(u32, EntityId)> {
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
        .result
        .unwrap()
}

fn create(host: &mut HostRuntime, world: WorldRef, value: ComponentValue) -> EntityId {
    apply(
        host,
        world,
        vec![
            Command::Create {
                alias: 1,
                metadata: Default::default(),
                adopt: false,
            },
            Command::insert_value(EntityRef::Alias(1), value),
        ],
    )[0]
    .1
}

fn camera(host: &mut HostRuntime, world: WorldRef) -> OutputRef {
    let entity = create(host, world, ComponentValue::Camera(Camera::default()));
    host.bind_output(world, entity, OutputKind::Camera).unwrap()
}

fn queue(host: &mut HostRuntime, world: WorldRef, input: Input) {
    host.world_mut(world.id())
        .unwrap()
        .enqueue_system_command_with_reply(PROBE, 1, 1, input)
        .unwrap();
}

fn group(host: &mut HostRuntime, world: WorldRef, inputs: Vec<Input>) {
    host.world_mut(world.id())
        .unwrap()
        .enqueue_system_command_batch_with_reply(PROBE, 1, 1, inputs)
        .unwrap();
}

fn remove_camera(output: OutputRef) -> Command {
    Command::RemoveComponent {
        entity: EntityRef::Handle(output.camera_entity().unwrap()),
        component: ComponentValue::CAMERA,
    }
}

fn viewport() -> WorldViewport {
    WorldViewport {
        width: 640,
        height: 480,
        device_pixel_ratio: 1.0,
    }
}

#[test]
fn queued_read_observes_current_root_identity_after_an_equal_value_rebind() {
    let (mut host, shared) = fixture();
    let target = world(&mut host);
    let output = camera(&mut host, target);
    host.set_root_output(output, viewport()).unwrap();
    let transported = host
        .root_output_binding(target)
        .unwrap()
        .unwrap()
        .generation
        .identity();
    queue(&mut host, target, Input::read(Read::world(target), vec![]));
    host.set_root_output(output, viewport()).unwrap();
    host.frame(0.0).unwrap();
    let observed = shared.lock().unwrap().reads[0].root.unwrap();
    assert_eq!(observed.output, output);
    assert_eq!(observed.viewport, viewport());
    assert_ne!(observed.generation.identity(), transported);
    assert_eq!(host.root_output_binding(target), Ok(Some(observed)));
}

#[test]
fn same_frame_parent_mutation_is_visible_before_child_validation() {
    let (mut host, shared) = fixture();
    let parent = world(&mut host);
    let child = world(&mut host);
    create(
        &mut host,
        parent,
        ComponentValue::WorldAttachment(WorldAttachment::spatial(child)),
    );
    let output = camera(&mut host, parent);
    host.set_root_output(output, viewport()).unwrap();
    host.frame(0.0).unwrap();
    let prior = host.latest_publication(parent.id()).unwrap();
    let root = host.root_output_binding(parent).unwrap();
    host.world_mut(parent.id())
        .unwrap()
        .enqueue(Batch {
            id: 2,
            operations: vec![remove_camera(output)],
        })
        .unwrap();
    queue(
        &mut host,
        child,
        Input::read(
            Read {
                entity: Some(output.camera_entity().unwrap()),
                output: Some(output),
                publication: Some(prior),
                ..Read::world(parent)
            },
            vec![parent],
        ),
    );
    let frame = host.frame(0.0).unwrap();
    assert!(
        frame.worlds[&parent.id()].as_ref().unwrap().outcomes[0]
            .result
            .is_ok()
    );
    assert!(
        frame.worlds[&child.id()]
            .as_ref()
            .unwrap()
            .system_command_outcomes[0]
            .result
            .is_ok()
    );
    let observations = shared.lock().unwrap();
    let read = &observations.reads[0];
    assert_eq!(read.root, root);
    assert!(!read.active);
    assert!(!read.output_live);
    assert!(read.publication_available);
}

#[test]
fn each_group_command_reborrows_local_mutations_and_its_own_foreign_declarations() {
    let (mut host, shared) = fixture();
    let target = world(&mut host);
    let foreign = world(&mut host);
    let output = camera(&mut host, target);
    group(
        &mut host,
        target,
        vec![
            Input::new(Action::Mutate(vec![remove_camera(output)])),
            Input::read(
                Read {
                    entity: Some(output.camera_entity().unwrap()),
                    output: Some(output),
                    ..Read::world(target)
                },
                vec![],
            ),
            Input::read(Read::world(foreign), vec![foreign]),
            Input::read(Read::world(foreign), vec![]),
            Input::read(Read::world(target), vec![]),
        ],
    );
    let frame = host.frame(0.0).unwrap();
    let outcome = &frame.worlds[&target.id()]
        .as_ref()
        .unwrap()
        .system_command_outcomes[0];
    assert_eq!(outcome.applied, 3);
    assert_eq!(outcome.result, Err(ErrorReason::InvalidEntity));
    let observations = shared.lock().unwrap();
    assert_eq!(observations.reads.len(), 2);
    assert!(!observations.reads[0].active);
    assert!(!observations.reads[0].output_live);
}

#[test]
fn local_replacement_does_not_retarget_old_output_and_unscoped_output_read_fails_closed() {
    let (mut host, shared) = fixture();
    let target = world(&mut host);
    let remote = world(&mut host);
    let local_output = camera(&mut host, target);
    let remote_output = camera(&mut host, remote);
    group(
        &mut host,
        target,
        vec![
            Input::new(Action::Mutate(vec![
                remove_camera(local_output),
                Command::insert_value(
                    EntityRef::Handle(local_output.camera_entity().unwrap()),
                    ComponentValue::Camera(Camera::default()),
                ),
            ])),
            Input::read(
                Read {
                    entity: Some(local_output.camera_entity().unwrap()),
                    output: Some(local_output),
                    ..Read::world(target)
                },
                vec![],
            ),
            Input::read(
                Read {
                    output: Some(remote_output),
                    ..Read::world(target)
                },
                vec![remote],
            ),
            Input::read(
                Read {
                    output: Some(remote_output),
                    ..Read::world(target)
                },
                vec![],
            ),
        ],
    );
    host.frame(0.0).unwrap();
    let observations = shared.lock().unwrap();
    assert_eq!(observations.reads.len(), 3);
    assert!(observations.reads[0].active);
    assert!(!observations.reads[0].output_live);
    assert!(observations.reads[1].output_live);
    assert!(!observations.reads[2].output_live);
}

#[test]
fn declared_deleted_and_foreign_host_borrows_fail_closed() {
    let (mut host, shared) = fixture();
    let target = world(&mut host);
    let stale = world(&mut host);
    assert!(host.destroy_world(stale.id()));
    let mut other = HostRuntime::new();
    crate::test_task_scheduler::install(&mut other);
    let foreign = world(&mut other);
    for reference in [stale, foreign] {
        queue(
            &mut host,
            target,
            Input::read(Read::world(reference), vec![reference]),
        );
    }
    let frame = host.frame(0.0).unwrap();
    for outcome in &frame.worlds[&target.id()]
        .as_ref()
        .unwrap()
        .system_command_outcomes
    {
        assert_eq!(outcome.result, Err(ErrorReason::InvalidEntity));
    }
    assert!(shared.lock().unwrap().reads.is_empty());
}

#[test]
fn queued_input_runs_without_a_presentation_requirement() {
    let (mut host, shared) = fixture();
    let parent = world(&mut host);
    let target = world(&mut host);
    create(
        &mut host,
        parent,
        ComponentValue::WorldAttachment(WorldAttachment::spatial(target)),
    );
    let output = camera(&mut host, parent);
    host.set_root_output(output, viewport()).unwrap();
    apply(&mut host, parent, vec![remove_camera(output)]);
    assert!(host.root_output(parent.id()).is_none());
    queue(&mut host, target, Input::read(Read::world(target), vec![]));
    let frame = host.frame(0.0).unwrap();
    assert!(
        frame.worlds[&target.id()]
            .as_ref()
            .unwrap()
            .system_command_outcomes[0]
            .result
            .is_ok()
    );
    assert_eq!(shared.lock().unwrap().reads[0].fault, None);
}

#[test]
fn superseded_pending_receipt_does_not_authorize_a_current_path() {
    let (mut host, shared) = fixture();
    let parent = world(&mut host);
    let child = world(&mut host);
    let anchor = create(
        &mut host,
        parent,
        ComponentValue::WorldAttachment(WorldAttachment::spatial(child)),
    );
    host.frame(0.0).unwrap();
    let old = host
        .publication(host.latest_publication(parent.id()).unwrap())
        .unwrap()
        .attachments[0]
        .clone();

    // A failed parent publication retains the superseded edge's completed contribution.
    shared.lock().unwrap().publication_failure = Some(parent);
    apply(
        &mut host,
        parent,
        vec![Command::insert_value(
            EntityRef::Handle(anchor),
            ComponentValue::WorldAttachment(WorldAttachment::spatial(child)),
        )],
    );
    assert_eq!(
        host.attachment_retirement(&old.token),
        Ok(crate::WorldAttachmentRetirement::Pending)
    );
    queue(
        &mut host,
        parent,
        Input::read(
            Read {
                edge: Some(old),
                ..Read::world(parent)
            },
            vec![child],
        ),
    );
    host.frame(0.0).unwrap();
    let observations = shared.lock().unwrap();
    assert!(!observations.reads[0].edge_current);
    assert!(!observations.reads[0].edge_available);
}

struct Payload;

impl Asset for Payload {
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
fn retained_source_is_not_latest_and_pending_asset_release_is_not_availability() {
    let (mut host, shared) = fixture();
    let parent = world(&mut host);
    let child = world(&mut host);
    create(
        &mut host,
        parent,
        ComponentValue::WorldAttachment(WorldAttachment::spatial(child)),
    );
    let kind = AssetTypeId(65000);
    host.asset_resources_mut()
        .register_loader(kind, || {
            crate::test_task_scheduler::blob_loader(|_| Ok(Payload))
        })
        .unwrap();
    let key = host
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
    host.progress_assets();
    crate::test_task_scheduler::poll_ready();
    host.progress_assets();
    shared.lock().unwrap().retained = Some(key);
    host.frame(0.0).unwrap();
    let old = host.latest_publication(child.id()).unwrap();
    let edge = host
        .publication(host.latest_publication(parent.id()).unwrap())
        .unwrap()
        .attachments[0]
        .clone();
    shared.lock().unwrap().publication_failure = Some(parent);
    assert!(
        host.frame(0.0)
            .unwrap()
            .publication_errors
            .contains_key(&parent.id())
    );
    assert_ne!(host.latest_publication(child.id()), Some(old));
    assert!(host.publication(old).is_some());
    let input = || {
        Input::read(
            Read {
                publication: Some(old),
                resource: Some(key),
                edge: Some(edge.clone()),
                ..Read::world(child)
            },
            vec![parent],
        )
    };
    group(
        &mut host,
        child,
        vec![input(), Input::new(Action::Revoke(key)), input()],
    );
    host.frame(0.0).unwrap();
    let observations = shared.lock().unwrap();
    assert!(observations.reads[0].publication_available);
    assert!(observations.reads[0].resource_available);
    assert!(observations.reads[0].edge_current);
    assert!(observations.reads[0].edge_available);
    assert!(!observations.reads[1].publication_available);
    assert!(!observations.reads[1].resource_available);
    assert!(!observations.reads[1].edge_available);
    assert!(host.publication_resource(old, key).is_none());
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Terminal {
    Applied,
    Rejected,
    Cancelled,
}

#[derive(Default)]
struct Ledger {
    permits: usize,
    terminal: BTreeMap<u64, Terminal>,
    attempts: BTreeMap<u64, usize>,
}

struct Ticket {
    id: u64,
    terminal: Cell<bool>,
    ledger: Arc<Mutex<Ledger>>,
}

impl Ticket {
    fn settle(&self, terminal: Terminal) {
        if !self.terminal.replace(true) {
            let mut ledger = self.ledger.lock().unwrap();
            assert!(ledger.terminal.insert(self.id, terminal).is_none());
        }
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        self.settle(Terminal::Cancelled);
        let mut ledger = self.ledger.lock().unwrap();
        ledger.permits -= 1;
        *ledger.attempts.entry(self.id).or_default() += 1;
    }
}

fn ticket(ledger: &Arc<Mutex<Ledger>>, id: u64, action: Action) -> Input {
    ledger.lock().unwrap().permits += 1;
    Input {
        ticket: Some(Ticket {
            id,
            terminal: Cell::new(false),
            ledger: ledger.clone(),
        }),
        ..Input::new(action)
    }
}

#[test]
fn owned_tickets_cancel_on_session_filter_and_world_destroy_without_a_frame() {
    let (mut host, _) = fixture();
    let target = world(&mut host);
    let ledger = Arc::new(Mutex::new(Ledger::default()));
    let mut context = host.world_mut(target.id()).unwrap();
    context
        .enqueue_system_command(PROBE, 1, ticket(&ledger, 1, Action::Fail))
        .unwrap();
    context
        .enqueue_system_command_batch_with_reply(
            PROBE,
            1,
            1,
            vec![
                ticket(&ledger, 2, Action::Fail),
                ticket(&ledger, 3, Action::Fail),
            ],
        )
        .unwrap();
    context
        .enqueue_system_command(PROBE, 2, ticket(&ledger, 4, Action::Fail))
        .unwrap();
    context.release_system_session(1);
    context.release_system_session(1);
    assert_eq!(context.tick(), 0);
    assert_eq!(ledger.lock().unwrap().permits, 1);
    assert_eq!(ledger.lock().unwrap().terminal.len(), 3);
    drop(context);
    assert!(host.destroy_world(target.id()));
    let ledger = ledger.lock().unwrap();
    assert_eq!(ledger.permits, 0);
    assert_eq!(ledger.terminal.len(), 4);
    assert!(
        ledger
            .terminal
            .values()
            .all(|value| *value == Terminal::Cancelled)
    );
    assert!(ledger.attempts.values().all(|attempts| *attempts == 1));
}

#[test]
fn group_prefix_failure_and_abandoned_tail_drop_once_and_return_every_permit() {
    let (mut host, shared) = fixture();
    let target = world(&mut host);
    let ledger = Arc::new(Mutex::new(Ledger::default()));
    group(
        &mut host,
        target,
        vec![
            ticket(&ledger, 1, Action::Read(Box::new(Read::world(target)))),
            ticket(&ledger, 2, Action::Fail),
            ticket(&ledger, 3, Action::Read(Box::new(Read::world(target)))),
        ],
    );
    let frame = host.frame(0.0).unwrap();
    let outcome = &frame.worlds[&target.id()]
        .as_ref()
        .unwrap()
        .system_command_outcomes[0];
    assert_eq!(outcome.applied, 1);
    assert_eq!(outcome.result, Err(ErrorReason::InvalidValue));
    assert_eq!(shared.lock().unwrap().reads.len(), 1);
    let ledger = ledger.lock().unwrap();
    assert_eq!(ledger.permits, 0);
    assert_eq!(
        ledger.terminal,
        BTreeMap::from([
            (1, Terminal::Applied),
            (2, Terminal::Rejected),
            (3, Terminal::Cancelled)
        ])
    );
    assert!(ledger.attempts.values().all(|attempts| *attempts == 1));
}
