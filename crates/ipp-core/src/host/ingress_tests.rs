use super::*;
use crate::{Batch, Command, ComponentValue, EntityRef, components::Camera, systems::*};
use std::sync::Arc;

/// Camera outputs and the evaluators they require.
const CAMERA_SYSTEMS: &[SystemId] = &[
    animation::AnimationSystem::ID,
    asset_dependencies::AssetDependencySystem::ID,
    hierarchy::HierarchySystem::ID,
    look_at::LookAtSystem::ID,
    hierarchy::FinalPropagationSystem::ID,
    geometry::GeometrySystem::ID,
    camera::CameraSystem::ID,
];

struct LocalFactory;

struct LocalSystem;

impl SystemFactory for LocalFactory {
    fn id(&self) -> SystemId {
        SystemId("fixture.local-read")
    }

    fn create(&self, _: &mut SystemInitContext<'_>) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(LocalSystem))
    }
}

impl System for LocalSystem {
    fn update(&mut self, _: &mut SystemUpdateContext<'_, '_>) {}

    fn command(
        &mut self,
        context: &mut SystemCommandContext<'_>,
        _: u64,
        _: &dyn std::any::Any,
    ) -> Result<(), ErrorReason> {
        let world = context.world.id();
        let view = context.host_ingress().unwrap();
        assert_eq!(view.local.world_ref().id(), world);
        assert!(view.world(view.local.world_ref()).is_some());
        Ok(())
    }
}

#[test]
fn ordinary_local_commands_and_groups_never_scan_foreign_worlds() {
    for count in [1, 128, 512] {
        let mut host = HostRuntime::with_system_factories(vec![Arc::new(LocalFactory)]).unwrap();
        for _ in 0..count {
            let world = host
                .create_world(Default::default(), &[SystemId("fixture.local-read")])
                .unwrap();
            let mut context = host.world_mut(world).unwrap();
            context
                .enqueue_system_command(SystemId("fixture.local-read"), 1, ())
                .unwrap();
            context
                .enqueue_system_command_batch_with_reply(
                    SystemId("fixture.local-read"),
                    1,
                    1,
                    vec![(), ()],
                )
                .unwrap();
        }
        let frame = host.frame(0.0).unwrap();
        assert_eq!(frame.worlds.len(), count);
        assert!(
            frame
                .worlds
                .values()
                .all(|report| report.as_ref().unwrap().system_command_outcomes[0].applied == 2)
        );
        assert_eq!(host.topology.foreign_world_scans, 0);
    }
}

fn camera(host: &mut HostRuntime) -> OutputRef {
    camera_in(host, CAMERA_SYSTEMS)
}

fn camera_in(host: &mut HostRuntime, systems: &[SystemId]) -> OutputRef {
    let world = host.create_world(Default::default(), systems).unwrap();
    host.world_mut(world)
        .unwrap()
        .enqueue(Batch {
            id: 1,
            operations: vec![
                Command::Create {
                    alias: 1,
                    metadata: Default::default(),
                    adopt: false,
                },
                Command::insert_value(
                    EntityRef::Alias(1),
                    ComponentValue::Camera(Camera::default()),
                ),
            ],
        })
        .unwrap();

    let entity = host
        .frame(0.0)
        .unwrap()
        .worlds
        .remove(&world)
        .unwrap()
        .unwrap()
        .outcomes
        .remove(0)
        .result
        .unwrap()[0]
        .1;
    let world = host.world_ref(world).unwrap();
    host.bind_output(world, entity, OutputKind::Camera).unwrap()
}

#[test]
fn host_identity_survives_empty_catalog_and_is_never_shared_or_reused() {
    let mut host = HostRuntime::new();
    let identity = host.identity();
    assert_eq!(host.world_ids().count(), 0);
    let output = camera(&mut host);
    host.set_root_output(
        output,
        WorldViewport {
            width: 320,
            height: 240,
            device_pixel_ratio: 1.0,
        },
    )
    .unwrap();
    assert_eq!(
        host.root_output_binding(output.world())
            .unwrap()
            .unwrap()
            .generation
            .identity()
            .0,
        identity
    );
    host.frame(0.0).unwrap();
    assert_eq!(host.identity(), identity);
    assert!(host.destroy_world(output.world().id()));
    assert_eq!(host.world_ids().count(), 0);
    assert_eq!(host.identity(), identity);
    let replacement = camera(&mut host);
    assert_eq!(host.identity(), identity);
    assert_ne!(replacement.world(), output.world());

    let mut seen = std::collections::BTreeSet::from([identity]);
    for _ in 0..32 {
        let other = HostRuntime::new();
        assert!(seen.insert(other.identity()));
    }
    drop(host);
    assert!(seen.insert(HostRuntime::new().identity()));
}

#[test]
fn world_fault_observation_validates_exact_lifetimes_without_progress_or_release() {
    use crate::services::asset_management::AssetSource;

    let mut host = HostRuntime::new();
    let output = camera(&mut host);
    host.set_root_output(
        output,
        WorldViewport {
            width: 320,
            height: 240,
            device_pixel_ratio: 1.0,
        },
    )
    .unwrap();
    host.frame(0.25).unwrap();
    let world = output.world();
    let stale = WorldRef {
        incarnation: world.incarnation + 1,
        ..world
    };
    let mut foreign_host = HostRuntime::new();
    let foreign = camera(&mut foreign_host).world();
    assert_eq!(world.id(), foreign.id());
    let destroyed = host.create_world(Default::default(), &[]).unwrap();
    let destroyed = host.world_ref(destroyed).unwrap();
    host.destroy_world(destroyed.id());
    let idle = host.create_world(Default::default(), &[]).unwrap();
    let idle = host.world_ref(idle).unwrap();
    let mut context = host.world_mut(world.id()).unwrap();
    let clock = (context.tick(), context.time());
    context
        .enqueue(Batch {
            id: 91,
            operations: vec![Command::Create {
                alias: 1,
                metadata: Default::default(),
                adopt: false,
            }],
        })
        .unwrap();
    drop(context);
    let key = host
        .assets
        .get_or_create(AssetSource {
            kind: crate::MESH_TYPE,
            uri: "missing:///fault-observer.ippm".into(),
            variant: 0,
        })
        .unwrap();
    host.assets.revoke_resource(key);
    let releases = host.assets.pending_releases();
    assert_eq!(releases.len(), 1);
    let publication = host.latest_publication(world.id());
    let binding = host.root_output_binding(world).unwrap();
    let revision = host.topology.revision;
    let frame = host.frame;

    for _ in 0..128 {
        assert_eq!(host.world_fault(world), Ok(None));
        assert_eq!(host.world_fault(idle), Ok(None));
        for invalid in [stale, foreign, destroyed] {
            assert_eq!(host.world_fault(invalid), Err(ErrorReason::InvalidEntity));
        }
    }
    assert_eq!(host.assets.pending_releases(), releases);
    assert!(host.assets.get(key).is_some());
    assert_eq!(host.topology.foreign_world_scans, 0);
    assert_eq!(host.topology.revision, revision);
    assert_eq!(host.frame, frame);
    assert_eq!(host.latest_publication(world.id()), publication);
    assert_eq!(host.root_output_binding(world).unwrap(), binding);
    let context = host.world_mut(world.id()).unwrap();
    assert_eq!((context.tick(), context.time()), clock);
    drop(context);
    let report = host.frame(0.0).unwrap();
    let outcomes = &report.worlds[&world.id()].as_ref().unwrap().outcomes;
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].batch_id, 91);
    assert!(outcomes[0].result.is_ok());
}

#[test]
fn world_fault_observation_reports_real_commit_failure_without_hiding_publication() {
    use crate::components::Scalar;

    struct Factory;

    struct NonConvergent(usize);

    impl SystemFactory for Factory {
        fn id(&self) -> SystemId {
            SystemId("fixture.fault-observation")
        }

        fn create(
            &self,
            _: &mut SystemInitContext<'_>,
        ) -> Result<Box<dyn System>, SystemInitError> {
            Ok(Box::new(NonConvergent(0)))
        }
    }

    impl System for NonConvergent {
        fn update(&mut self, _: &mut SystemUpdateContext<'_, '_>) {}

        fn before_commit(&mut self, context: &mut SystemCommitContext<'_>) {
            let targets: Vec<_> = context
                .changed_components()
                .filter(|(_, component)| *component == ComponentValue::SCALAR)
                .map(|(entity, _)| entity)
                .collect();
            for entity in targets {
                self.0 += 1;
                context.restore_evaluated_component(
                    entity,
                    ComponentValue::Scalar(Scalar {
                        value: self.0 as f32,
                    }),
                );
            }
        }
    }

    let mut factories = compiled_system_factories();
    factories.push(Arc::new(Factory));
    let mut host = HostRuntime::with_system_factories(factories).unwrap();
    let fault = Factory.id();
    let output = camera_in(
        &mut host,
        &[CAMERA_SYSTEMS, &[constraints::ConstraintSystem::ID, fault]].concat(),
    );
    host.frame(0.1).unwrap();
    let world = output.world();
    let publication = host.latest_publication(world.id()).unwrap();
    assert_eq!(host.world_fault(world), Ok(None));
    host.world_mut(world.id())
        .unwrap()
        .enqueue(Batch {
            id: 92,
            operations: vec![Command::insert_value(
                EntityRef::Handle(output.camera_entity().unwrap()),
                ComponentValue::Scalar(Scalar::default()),
            )],
        })
        .unwrap();
    let report = host.frame(0.1).unwrap();
    assert_eq!(
        report.worlds[&world.id()].as_ref().unwrap().outcomes[0]
            .result
            .as_ref()
            .unwrap_err()
            .reason,
        ErrorReason::NonConvergentCommit
    );
    let frame = host.frame;
    for _ in 0..128 {
        assert_eq!(
            host.world_fault(world),
            Ok(Some(ErrorReason::NonConvergentCommit))
        );
    }
    assert_eq!(host.frame, frame);
    assert_eq!(host.latest_publication(world.id()), Some(publication));
    assert!(host.output(publication, output).is_some());
    let observed = host.world_fault(world).unwrap();
    assert_eq!(host.world_mut(world.id()).unwrap().fault(), observed);
}

#[test]
fn explicit_root_rebind_is_host_fenced_never_aba_and_publication_refresh_is_not_rebind() {
    let mut host = HostRuntime::new();
    let output = camera(&mut host);
    let viewport = WorldViewport {
        width: 640,
        height: 480,
        device_pixel_ratio: 2.0,
    };
    assert_eq!(host.root_output_binding(output.world()), Ok(None));
    host.set_root_output(output, viewport).unwrap();
    let first = host.root_output_binding(output.world()).unwrap().unwrap();
    host.frame(0.0).unwrap();
    host.frame(0.1).unwrap();
    assert_eq!(host.root_output_binding(output.world()), Ok(Some(first)));
    host.set_root_output(output, viewport).unwrap();
    let second = host.root_output_binding(output.world()).unwrap().unwrap();
    assert_ne!(first.generation, second.generation);
    assert_eq!(first.generation.identity(), (host.topology.identity, 1));
    assert_eq!(second.generation.identity(), (host.topology.identity, 2));
    assert_eq!(first.output, second.output);
    assert_eq!(first.viewport, second.viewport);
    host.clear_root_output(output.world().id());
    assert_eq!(host.root_output_binding(output.world()), Ok(None));
    host.set_root_output(
        output,
        WorldViewport {
            width: 800,
            ..viewport
        },
    )
    .unwrap();
    let third = host.root_output_binding(output.world()).unwrap().unwrap();
    assert_ne!(second.generation, third.generation);
    assert_eq!(third.viewport.width, 800);
    assert_eq!(
        host.set_root_output(
            output,
            WorldViewport {
                width: 0,
                ..viewport
            }
        ),
        Err(ErrorReason::InvalidValue)
    );
    assert_eq!(host.root_output_binding(output.world()), Ok(Some(third)));
    host.topology.next_root_binding = u64::MAX;
    assert_eq!(
        host.set_root_output(output, viewport),
        Err(ErrorReason::Capacity)
    );
    assert_eq!(host.root_output_binding(output.world()), Ok(Some(third)));
    assert!(host.destroy_world(output.world().id()));
    assert_eq!(
        host.root_output_binding(output.world()),
        Err(ErrorReason::InvalidEntity)
    );
    let replacement = camera(&mut host);
    assert_eq!(host.root_output_binding(replacement.world()), Ok(None));
    assert_eq!(
        host.set_root_output(replacement, viewport),
        Err(ErrorReason::Capacity)
    );

    let mut other = HostRuntime::new();
    let other_output = camera(&mut other);
    other.set_root_output(other_output, viewport).unwrap();
    let other_root = other
        .root_output_binding(other_output.world())
        .unwrap()
        .unwrap();
    assert_ne!(first.generation, other_root.generation);
    assert_ne!(
        first.generation.identity().0,
        other_root.generation.identity().0
    );
    assert_eq!(
        first.generation.identity().1,
        other_root.generation.identity().1
    );
    assert_eq!(
        host.root_output_binding(other_output.world()),
        Err(ErrorReason::InvalidEntity)
    );
}
