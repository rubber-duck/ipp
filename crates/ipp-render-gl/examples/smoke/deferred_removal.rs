//! End-of-update removal on a real Host, decoded mesh and completed GLES frames.
//!
//! The fixture arms a custom System through host-local configuration, without a
//! client command or a production clock control. EGL setup and capture stay in
//! the runner, so the scenario can use another real RenderDevice unchanged.

use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use ipp_core::{
    Command, ComponentValue, EntityId, EntityRef, ErrorReason, HostRuntime, WorldId,
    components::{MeshInstance, Transform, UnlitMaterial},
    systems::{
        System, SystemCommitContext, SystemDependency, SystemFactory, SystemId, SystemInitContext,
        SystemInitError, SystemLifecycleContext, SystemUpdateContext, compiled_system_factories,
        lifecycle_publisher::{
            ComponentLifecycleKind, EntityLifecycleKind, LifecycleFilter, LifecycleObservation,
            LifecyclePublisherCommand, LifecyclePublisherOutput, LifecyclePublisherSystem,
        },
    },
};
use ipp_render_gl::{RenderDevice, RenderService};

use super::world::{HEIGHT, WIDTH, coverage, save};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const SOURCE: &str = "fixture:///deferred-removal.mesh";
const REMOVER: SystemId = SystemId("fixture.deferred-remover");
const OBSERVER: SystemId = SystemId("fixture.deferred-observer");
const SESSION: u64 = 91;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Removal {
    Entity,
    Mesh,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Trace {
    Queued,
    LaterUpdate,
    Before(SystemId),
    Applied(SystemId),
    Finished(SystemId),
}

#[derive(Default)]
struct FixtureControl {
    target: Mutex<Option<(WorldId, EntityId)>>,
    armed: AtomicBool,
    trace: Mutex<Vec<Trace>>,
}

impl FixtureControl {
    fn target(&self, world: WorldId) -> Option<EntityId> {
        self.armed
            .load(Ordering::Relaxed)
            .then(|| *self.target.lock().unwrap())
            .flatten()
            .filter(|(id, _)| *id == world)
            .map(|(_, entity)| entity)
    }

    fn record(&self, event: Trace) {
        self.trace.lock().unwrap().push(event);
    }
}

struct RemovalFactory {
    id: SystemId,
    removal: Removal,
    control: Arc<FixtureControl>,
}

struct RemovalSystem {
    id: SystemId,
    removal: Removal,
    control: Arc<FixtureControl>,
    updated: bool,
}

impl SystemFactory for RemovalFactory {
    fn id(&self) -> SystemId {
        self.id
    }

    fn dependencies(&self) -> &[SystemDependency] {
        if self.id == REMOVER {
            &[SystemDependency::Required(SystemId("ipp.render"))]
        } else {
            &[SystemDependency::Required(REMOVER)]
        }
    }

    fn create(
        &self,
        _: &mut SystemInitContext<'_>,
    ) -> std::result::Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(RemovalSystem {
            id: self.id,
            removal: self.removal,
            control: Arc::clone(&self.control),
            updated: false,
        }))
    }
}

impl System for RemovalSystem {
    fn update(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
        let Some(entity) = self.control.target(context.world.id()) else {
            return;
        };
        if self.updated {
            return;
        }
        self.updated = true;
        assert_mesh(
            context
                .world
                .inspect_effective(entity)
                .unwrap()
                .components
                .iter(),
        );
        if self.id == REMOVER {
            for _ in 0..2 {
                match self.removal {
                    Removal::Entity => context.world.defer_remove_entity(entity),
                    Removal::Mesh => context
                        .world
                        .defer_remove_component(entity, ComponentValue::MESH_INSTANCE),
                }
                .unwrap();
            }
            if self.removal == Removal::Entity {
                // This token is valid now and stale after the queued entity deletion.
                context
                    .world
                    .defer_remove_component(entity, ComponentValue::MESH_INSTANCE)
                    .unwrap();
            }
            self.control.record(Trace::Queued);
        } else {
            self.control.record(Trace::LaterUpdate);
        }
    }

    fn before_commit(&mut self, context: &mut SystemCommitContext<'_>) {
        let Some(entity) = self.control.target(context.world().id()) else {
            return;
        };
        if context
            .changed_components()
            .any(|changed| changed == (entity, ComponentValue::MESH_INSTANCE))
            && !context.retains_component(entity, ComponentValue::MESH_INSTANCE)
        {
            let old = context
                .world()
                .effective_component(entity, ComponentValue::MESH_INSTANCE)
                .expect("every handler sees occupied storage until the barrier completes");
            assert_mesh(std::iter::once(&old));
            self.control.record(Trace::Before(self.id));
        }
    }

    fn lifecycle(
        &mut self,
        context: &SystemLifecycleContext<'_>,
        observation: &LifecycleObservation,
    ) {
        let Some(entity) = self.control.target(context.world.id()) else {
            return;
        };
        if matches!(observation, LifecycleObservation::Component {
            entity: observed,
            component: ComponentValue::MESH_INSTANCE,
            kind: ComponentLifecycleKind::Removed,
            ..
        } if *observed == entity)
        {
            assert!(
                context
                    .world
                    .effective_component(entity, ComponentValue::MESH_INSTANCE)
                    .is_none()
            );
            self.control.record(Trace::Applied(self.id));
        }
    }

    fn finish_update(
        &mut self,
        context: &mut SystemUpdateContext<'_, '_>,
        _: &mut ipp_core::WorldUpdateReport,
    ) {
        let Some(entity) = self.control.target(context.world.id()) else {
            return;
        };
        let remaining = context.world.inspect_effective(entity);
        assert_eq!(remaining.is_none(), self.removal == Removal::Entity);
        assert!(remaining.is_none_or(|snapshot| {
            snapshot
                .components
                .iter()
                .all(|value| !matches!(value, ComponentValue::MeshInstance(_)))
        }));
        assert_eq!(
            context.world.defer_remove_entity(entity),
            Err(ErrorReason::InvalidValue)
        );
        assert_eq!(
            context
                .world
                .defer_remove_component(entity, ComponentValue::MESH_INSTANCE),
            Err(ErrorReason::InvalidValue)
        );
        self.control.record(Trace::Finished(self.id));
    }
}

fn assert_mesh<'a>(values: impl Iterator<Item = &'a ComponentValue>) {
    assert!(
        values.into_iter().any(
            |value| matches!(value, ComponentValue::MeshInstance(mesh) if mesh.source == SOURCE)
        )
    );
}

/// Keep the trace even if an assertion aborts the scenario before its last capture.
struct TraceEvidence<'a> {
    control: &'a FixtureControl,
    path: std::path::PathBuf,
}

impl Drop for TraceEvidence<'_> {
    fn drop(&mut self) {
        let trace = self
            .control
            .trace
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let _ = std::fs::write(&self.path, format!("{trace:#?}\n"));
    }
}

pub fn run<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    fixture: &[u8],
    mut capture: impl FnMut() -> Result<Vec<u8>>,
    output: &Path,
) -> Result<()> {
    for (name, removal) in [("entity", Removal::Entity), ("component", Removal::Mesh)] {
        let control = Arc::new(FixtureControl::default());
        let _evidence = TraceEvidence {
            control: &control,
            path: output.join(format!("deferred-{name}-trace.txt")),
        };
        let mut factories = compiled_system_factories();
        for id in [REMOVER, OBSERVER] {
            factories.push(Arc::new(RemovalFactory {
                id,
                removal,
                control: Arc::clone(&control),
            }));
        }
        let mut host = HostRuntime::with_system_factories(factories)?;
        renderer.install(&mut host)?;
        let (world, target) = scene(&mut host)?;
        let (peer, _) = scene(&mut host)?;
        load(&mut host, [world, peer], fixture)?;
        renderer.begin_frame();
        assert_eq!(
            super::world::render_frame(
                renderer,
                &mut host.world_mut(world).unwrap(),
                WIDTH,
                HEIGHT
            )?
            .draw_calls,
            1
        );
        let visible = capture()?;
        save(output, &format!("deferred-{name}-visible"), &visible)?;
        assert!(coverage(&visible).0 > 1000);
        assert_eq!(
            renderer
                .render(&mut host.world_mut(peer).unwrap(), WIDTH, HEIGHT)?
                .draw_calls,
            1
        );
        let peer_before = capture()?;
        save(
            output,
            &format!("deferred-{name}-peer-before"),
            &peer_before,
        )?;

        host.world_mut(world).unwrap().enqueue_system_command(
            LifecyclePublisherSystem::ID,
            SESSION,
            LifecyclePublisherCommand::Subscribe {
                subscription: 1,
                filter: LifecycleFilter {
                    entity: Some(target),
                    assets: false,
                    ..Default::default()
                },
            },
        )?;
        update(&mut host, &[world])?;
        assert!(
            host.world_mut(world)
                .unwrap()
                .drain_system_events::<LifecyclePublisherOutput>(
                    LifecyclePublisherSystem::ID,
                    SESSION
                )
                .is_empty()
        );
        *control.target.lock().unwrap() = Some((world, target));
        control.armed.store(true, Ordering::Relaxed);
        update(&mut host, &[world])?;

        let trace = control.trace.lock().unwrap().clone();
        assert_eq!(
            trace,
            vec![
                Trace::Queued,
                Trace::LaterUpdate,
                Trace::Before(REMOVER),
                Trace::Before(OBSERVER),
                Trace::Applied(REMOVER),
                Trace::Applied(OBSERVER),
                Trace::Finished(REMOVER),
                Trace::Finished(OBSERVER)
            ]
        );
        let outputs = host
            .world_mut(world)
            .unwrap()
            .drain_system_events::<LifecyclePublisherOutput>(LifecyclePublisherSystem::ID, SESSION);
        std::fs::write(
            output.join(format!("deferred-{name}-events.txt")),
            format!("{outputs:#?}\n"),
        )?;
        let events = outputs
            .into_iter()
            .flat_map(|output| match output {
                LifecyclePublisherOutput::Events(events) => events,
                LifecyclePublisherOutput::Overflow {
                    ..
                } => panic!("bounded fixture must not overflow"),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            events.len(),
            if removal == Removal::Entity {
                5
            } else {
                2
            }
        );
        let tick = host.world_mut(world).unwrap().tick();
        assert!(
            events
                .iter()
                .all(|event| event.tick == tick && event.subscription == 1)
        );
        assert_eq!(events.iter().filter(|event| matches!(event.observation, LifecycleObservation::Component { entity, component: ComponentValue::MESH_INSTANCE, kind: ComponentLifecycleKind::Removed, previous_incarnation: Some(_), incarnation: None } if entity == target)).count(), 1);
        assert_eq!(events.iter().filter(|event| matches!(event.observation, LifecycleObservation::Component { entity, component: ComponentValue::BOUNDING_GEOMETRY, kind: ComponentLifecycleKind::Removed, previous_incarnation: Some(_), incarnation: None } if entity == target)).count(), 1);
        assert_eq!(events.iter().filter(|event| matches!(event.observation, LifecycleObservation::Entity { entity, kind: EntityLifecycleKind::Deleted } if entity == target)).count(), usize::from(removal == Removal::Entity));
        assert!(
            events
                .windows(2)
                .all(|pair| pair[0].sequence < pair[1].sequence)
        );

        assert!(
            host.world_mut(world).unwrap().render_items().is_empty(),
            "prepared rows must be rebuilt after deferred cleanup"
        );
        renderer.begin_frame();
        assert_eq!(
            renderer
                .render(&mut host.world_mut(world).unwrap(), WIDTH, HEIGHT)?
                .draw_calls,
            0
        );
        let removed = capture()?;
        save(output, &format!("deferred-{name}-removed"), &removed)?;
        assert_eq!(
            coverage(&removed).0,
            0,
            "the very next presentation must show the removal"
        );
        assert_eq!(
            renderer
                .render(&mut host.world_mut(peer).unwrap(), WIDTH, HEIGHT)?
                .draw_calls,
            1
        );
        let peer_after = capture()?;
        save(output, &format!("deferred-{name}-peer-after"), &peer_after)?;
        assert_eq!(
            peer_before, peer_after,
            "shared mesh and peer World survive cleanup"
        );
        assert!(host.destroy_world(world));
        assert!(host.destroy_world(peer));
        host.flush_resource_lifecycle();
    }
    Ok(())
}

fn scene(host: &mut HostRuntime) -> Result<(WorldId, EntityId)> {
    let mut world = super::world::fixture_world(host)?;
    super::world::apply(
        &mut world,
        vec![
            Command::Create {
                alias: 1,
                metadata: Default::default(),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(1),
                value: ComponentValue::Transform(Transform::default()),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(1),
                value: ComponentValue::MeshInstance(MeshInstance {
                    source: SOURCE.into(),
                    variant: 0,
                }),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(1),
                value: ComponentValue::UnlitMaterial(UnlitMaterial::default()),
            },
        ],
    )?;
    let target = world
        .entities()
        .into_iter()
        .find(|entity| {
            entity
                .effective
                .iter()
                .any(|value| matches!(value, ComponentValue::MeshInstance(_)))
        })
        .unwrap()
        .id;
    Ok((world.id(), target))
}

fn update(host: &mut HostRuntime, worlds: &[WorldId]) -> Result<()> {
    for &world in worlds {
        host.world_mut(world).unwrap().prepare_update(0.0)?;
    }
    host.progress_assets();
    for &world in worlds {
        host.world_mut(world).unwrap().step(0.0)?;
    }
    host.flush_resource_lifecycle();
    Ok(())
}

fn load(host: &mut HostRuntime, worlds: [WorldId; 2], fixture: &[u8]) -> Result<()> {
    for _ in 0..128 {
        update(host, &worlds)?;
        for request in host.take_resource_requests() {
            assert_eq!(request.kind, ipp_core::AssetResourceKind::Mesh);
            host.complete_resource(request.id, Ok(fixture.to_vec()))?;
        }
        if worlds
            .iter()
            .all(|world| host.world_mut(*world).unwrap().render_items().len() == 1)
        {
            return Ok(());
        }
    }
    Err("deferred-removal cube did not become ready".into())
}
