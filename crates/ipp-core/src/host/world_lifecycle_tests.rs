use super::*;
use crate::host::test_support::{CAMERA_SYSTEMS, camera, camera_in};
use crate::{Batch, Command, ComponentValue, EntityRef, systems::*};
use std::sync::Arc;

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
