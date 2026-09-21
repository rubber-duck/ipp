//! Generated parameter adapters execute against real Host-owned Systems and services.

use ipp_core::{HostRuntime, systems::*};
use std::{
    marker::PhantomData,
    sync::{Arc, Mutex},
};

mod support;

struct ParameterFactory<T: System + SystemBoundUpdate> {
    id: SystemId,
    create: Box<dyn Fn(SystemBindings<T>) -> T + Send + Sync>,
    marker: PhantomData<fn() -> T>,
}

impl<T: System + SystemBoundUpdate> SystemFactory for ParameterFactory<T> {
    fn id(&self) -> SystemId {
        self.id
    }

    fn dependencies(&self) -> &[SystemDependency] {
        T::dependencies()
    }

    fn create(
        &self,
        context: &mut SystemInitContext<'_>,
    ) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new((self.create)(SystemBindings::resolve(context)?)))
    }
}

fn factory<T: System + SystemBoundUpdate>(
    id: SystemId,
    create: impl Fn(SystemBindings<T>) -> T + Send + Sync + 'static,
) -> Arc<dyn SystemFactory> {
    Arc::new(ParameterFactory {
        id,
        create: Box::new(create),
        marker: PhantomData,
    })
}

struct Source {
    bindings: SystemBindings<Self>,
    count: u64,
}

impl Source {
    const ID: SystemId = SystemId("test.parameter-source");
}

#[ipp_core::systems::system_update]
impl Source {
    fn update(&mut self, _ecs: SystemEcsAccess<'_>, _dt: f64) {
        self.count += 1;
    }
}

impl System for Source {
    ipp_core::system_update!(bindings);

    fn save_persistent_state(
        &self,
        context: &mut SystemSaveContext<'_>,
    ) -> Result<Option<SystemPersistentState>, String> {
        *context.bytes = context.bytes.checked_add(8).ok_or("size overflow")?;
        if *context.bytes > context.max_bytes {
            return Err("counter payload exceeds capture limit".into());
        }
        Ok(Some(self.count.to_le_bytes().to_vec()))
    }

    fn load_persistent_state(
        &mut self,
        _context: &mut SystemLoadContext<'_, '_>,
        state: Option<&SystemPersistentState>,
    ) -> Result<(), String> {
        let state = state.ok_or("counter state missing")?;
        self.count = u64::from_le_bytes(
            state
                .as_slice()
                .try_into()
                .map_err(|_| "counter payload length")?,
        );
        Ok(())
    }
}

ipp_core::system_parameter!(Source);

type Seen = Arc<Mutex<Vec<(ipp_core::WorldId, u64, f64)>>>;

struct Dependent {
    bindings: SystemBindings<Self>,
    seen: Seen,
}

#[ipp_core::systems::system_update]
impl Dependent {
    fn update(
        &mut self,
        ecs: SystemEcsAccess<'_>,
        source: &Source,
        io: &mut ipp_core::services::data_source::DataSourceManagementService,
        assets: &mut ipp_core::services::asset_management::AssetManagementService,
        dt: f64,
    ) {
        let _ = io;
        let _ = assets;
        self.seen.lock().unwrap().push((ecs.id(), source.count, dt));
    }
}

impl System for Dependent {
    ipp_core::system_update!(bindings);
}

struct Optional {
    bindings: SystemBindings<Self>,
    absent: Arc<Mutex<bool>>,
}

#[ipp_core::systems::system_update]
impl Optional {
    fn update(&mut self, _ecs: SystemEcsAccess<'_>, source: Option<&Source>, _dt: f64) {
        *self.absent.lock().unwrap() = source.is_none();
    }
}

impl System for Optional {
    ipp_core::system_update!(bindings);
}

struct DuplicateService {
    bindings: SystemBindings<Self>,
}

#[ipp_core::systems::system_update]
impl DuplicateService {
    fn update(
        &mut self,
        _ecs: SystemEcsAccess<'_>,
        _one: &ipp_core::services::data_source::DataSourceManagementService,
        _two: &mut ipp_core::services::data_source::DataSourceManagementService,
        _dt: f64,
    ) {
    }
}

impl System for DuplicateService {
    ipp_core::system_update!(bindings);
}

#[test]
fn typed_parameters_generate_order_and_borrow_independent_mutable_services() {
    let seen = Seen::default();
    let output = seen.clone();
    let mut factories = compiled_system_factories();
    factories.push(factory(SystemId("test.dependent"), move |bindings| {
        Dependent {
            bindings,
            seen: output.clone(),
        }
    }));
    factories.push(factory(Source::ID, |bindings| Source {
        bindings,
        count: 0,
    }));
    let mut host = HostRuntime::with_system_factories(factories).unwrap();
    let one = host.create_world(Default::default()).unwrap();
    let two = host.create_world(Default::default()).unwrap();
    host.world_mut(one).unwrap().step(0.25).unwrap();
    host.world_mut(one).unwrap().step(0.5).unwrap();
    host.world_mut(two).unwrap().step(0.75).unwrap();
    assert_eq!(
        *seen.lock().unwrap(),
        [(one, 1, 0.25), (one, 2, 0.5), (two, 1, 0.75)]
    );
    assert_eq!(
        Dependent::dependencies(),
        &[SystemDependency::Required(Source::ID)]
    );
}

#[test]
fn optional_parameter_absence_is_valid_but_required_metadata_rejects_it() {
    let absent = Arc::new(Mutex::new(false));
    let output = absent.clone();
    let mut factories = compiled_system_factories();
    factories.push(factory(SystemId("test.optional"), move |bindings| {
        Optional {
            bindings,
            absent: output.clone(),
        }
    }));
    let mut host = HostRuntime::with_system_factories(factories).unwrap();
    let world = host.create_world(Default::default()).unwrap();
    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert!(*absent.lock().unwrap());

    let mut factories = compiled_system_factories();
    factories.push(factory(SystemId("test.dependent"), |bindings| Dependent {
        bindings,
        seen: Seen::default(),
    }));
    assert!(matches!(
        HostRuntime::with_system_factories(factories),
        Err(SystemScheduleError::MissingRequired {
            required: Source::ID,
            ..
        })
    ));
}

#[test]
fn duplicate_service_parameters_reject_during_initialization() {
    let mut factories = compiled_system_factories();
    factories.push(factory(SystemId("test.duplicate"), |bindings| {
        DuplicateService {
            bindings,
        }
    }));
    let mut host = HostRuntime::with_system_factories(factories).unwrap();
    assert!(host.create_world(Default::default()).is_err());
}

#[test]
fn extension_system_state_round_trips_through_generic_world_file_sections() {
    use ipp_core::services::world_serialization::{WorldLoadOptions, WorldSnapshot};
    let mut factories = compiled_system_factories();
    factories.push(factory(Source::ID, |bindings| Source {
        bindings,
        count: 0,
    }));
    let mut host = HostRuntime::with_system_factories(factories).unwrap();
    let original = host.create_world(Default::default()).unwrap();
    for _ in 0..3 {
        host.world_mut(original).unwrap().step(0.0).unwrap();
    }
    let bytes = host.save_world(original, 77, Default::default()).unwrap();
    let mut snapshot = WorldSnapshot::decode(&bytes, 77, Default::default()).unwrap();
    assert_eq!(snapshot.systems[Source::ID.0], 3u64.to_le_bytes());
    let restored = host
        .load_world(
            &bytes,
            77,
            WorldLoadOptions {
                symbolic_id: Some("restored-counter".into()),
                ..Default::default()
            },
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert_eq!(
        host.world_mut(restored)
            .unwrap()
            .system::<Source>(Source::ID)
            .unwrap()
            .count,
        3
    );
    host.world_mut(restored).unwrap().step(0.0).unwrap();
    assert_eq!(
        host.world_mut(restored)
            .unwrap()
            .system::<Source>(Source::ID)
            .unwrap()
            .count,
        4
    );
    assert_eq!(
        host.world_mut(original)
            .unwrap()
            .system::<Source>(Source::ID)
            .unwrap()
            .count,
        3
    );
    snapshot.systems.insert("test.unselected".into(), vec![1]);
    let invalid = snapshot.encode(77, Default::default()).unwrap();
    assert!(
        host.load_world(
            &invalid,
            77,
            WorldLoadOptions {
                symbolic_id: Some("invalid-counter".into()),
                ..Default::default()
            },
            Default::default(),
            Default::default()
        )
        .is_err()
    );
}

mod effective_observations {
    use super::*;
    use crate::support::HostWorldTestDriver;
    use ipp_core::{
        Batch, Command, ComponentValue, EntityId, EntityRef,
        components::schema::FieldValue,
        components::{LinearDriver, Scalar},
        services::asset_management::{AssetUpload, AssetUploadIdentity},
        systems::animation::*,
    };
    use std::mem::offset_of;

    type Targets = Arc<Mutex<Option<(EntityId, EntityId)>>>;
    type Observations = Arc<Mutex<Vec<(&'static str, f32)>>>;

    struct EffectiveObserver {
        bindings: SystemBindings<Self>,
        targets: Targets,
        seen: Observations,
    }

    fn scalar(snapshot: SystemEffectiveEntitySnapshot) -> f32 {
        // Exhaustive destructuring keeps this callback API explicitly effective-only.
        let SystemEffectiveEntitySnapshot {
            id: _,
            metadata: _,
            components,
        } = snapshot;
        components
            .iter()
            .find_map(|value| match value {
                ComponentValue::Scalar(value) => Some(value.value),
                _ => None,
            })
            .unwrap()
    }

    #[ipp_core::systems::system_update(SystemDependency::Required(SystemId("ipp.constraints")))]
    impl EffectiveObserver {
        fn update(&mut self, ecs: SystemEcsAccess<'_>, _dt: f64) {
            if let Some((_, target)) = *self.targets.lock().unwrap() {
                self.seen.lock().unwrap().push((
                    "typed-update",
                    scalar(ecs.inspect_effective(target).unwrap()),
                ));
            }
        }
    }

    impl System for EffectiveObserver {
        ipp_core::system_update!(bindings);

        fn after_commit(&mut self, context: &mut SystemCommitContext<'_>) {
            if context.is_evaluated()
                && let Some((source, _)) = *self.targets.lock().unwrap()
            {
                self.seen.lock().unwrap().push((
                    "committed-sample",
                    scalar(context.world().inspect_effective(source).unwrap()),
                ));
            }
        }

        fn before_numeric_update(&mut self, context: &mut SystemNumericContext<'_>) {
            if let Some((source, _)) = *self.targets.lock().unwrap()
                && context
                    .changed_components()
                    .contains(&(source, ComponentValue::SCALAR))
            {
                self.seen.lock().unwrap().push((
                    "numeric-before",
                    scalar(context.world().inspect_effective(source).unwrap()),
                ));
            }
        }

        fn finish_update(
            &mut self,
            context: &mut SystemUpdateContext<'_, '_>,
            _: &mut ipp_core::WorldUpdateReport,
        ) {
            if let Some((_, target)) = *self.targets.lock().unwrap() {
                self.seen.lock().unwrap().push((
                    "finished",
                    scalar(context.world.inspect_effective(target).unwrap()),
                ));
            }
        }

        fn teardown(&mut self, context: &mut SystemTeardownContext<'_>) {
            if let Some((_, target)) = *self.targets.lock().unwrap() {
                self.seen.lock().unwrap().push((
                    "teardown",
                    scalar(context.world.inspect_effective(target).unwrap()),
                ));
            }
        }
    }

    #[test]
    fn scoped_effective_observations_follow_animation_and_constraints_without_authored_claims() {
        let targets = Targets::default();
        let seen = Observations::default();
        let observed_targets = targets.clone();
        let observed_values = seen.clone();
        let mut factories = compiled_system_factories();
        factories.push(factory(
            SystemId("test.effective-observer"),
            move |bindings| EffectiveObserver {
                bindings,
                targets: observed_targets.clone(),
                seen: observed_values.clone(),
            },
        ));
        let mut host = HostRuntime::with_system_factories(factories).unwrap();
        let id = host.create_world(Default::default()).unwrap();
        let mut world = host.world_mut(id).unwrap();
        world
            .enqueue(Batch {
                id: 1,
                operations: vec![
                    Command::Create {
                        alias: 1,
                        metadata: Default::default(),
                    },
                    Command::InsertComponentValue {
                        entity: EntityRef::Alias(1),
                        value: ComponentValue::Scalar(Scalar {
                            value: 3.0,
                        }),
                    },
                    Command::Create {
                        alias: 2,
                        metadata: Default::default(),
                    },
                    Command::InsertComponentValue {
                        entity: EntityRef::Alias(2),
                        value: ComponentValue::Scalar(Scalar {
                            value: 20.0,
                        }),
                    },
                ],
            })
            .unwrap();
        let created = world.step(0.0).unwrap().outcomes.remove(0).result.unwrap();
        let (source, target) = (created[0].1, created[1].1);
        world
            .enqueue(Batch {
                id: 2,
                operations: vec![Command::InsertComponentValue {
                    entity: EntityRef::Handle(target),
                    value: ComponentValue::LinearDriver(LinearDriver {
                        source,
                        scale: 2.0,
                        bias: 1.0,
                    }),
                }],
            })
            .unwrap();
        world.step(0.0).unwrap().outcomes[0]
            .result
            .as_ref()
            .unwrap();
        let property = AnimationTrackTarget::AnimationProperty(AnimationProperty {
            component: ComponentValue::SCALAR,
            offsets: vec![offset_of!(Scalar, value) as u32],
        });
        let clip = AnimationClip::new(
            1.0,
            vec![AnimationTrack {
                target: property.clone(),
                keys: vec![AnimationKeyframe {
                    time: 0.0,
                    value: AnimationValue::Field(FieldValue::F32(5.0)),
                    interpolation: AnimationInterpolation::Step,
                }],
            }],
        )
        .unwrap();
        world
            .enqueue_asset(AssetUpload {
                id: 1,
                key: AssetUploadIdentity {
                    kind: ANIMATION_TYPE,
                    asset: 1,
                    variant: 0,
                },
                bytes: clip.encode(),
            })
            .unwrap();
        drop(world);
        host.await_world_upload_for_test(id).assets[0]
            .result
            .as_ref()
            .unwrap();
        let mut world = host.world_mut(id).unwrap();
        let controller = world
            .create_animation_controller(AnimationControllerDescription {
                drivers: vec![AnimationDriverDescription {
                    source: "asset://10/1".into(),
                    variant: 0,
                    track: 0,
                    target: source,
                    property,
                    weight: 1.0,
                    additive: false,
                    reference_time: 0.0,
                    repeat: false,
                }],
                ..Default::default()
            })
            .unwrap();
        world
            .enqueue_playback(controller, AnimationPlaybackControl::Play)
            .unwrap();
        *targets.lock().unwrap() = Some((source, target));
        seen.lock().unwrap().clear();
        world.step(0.0).unwrap();
        let source_snapshot = world.inspect(source).unwrap();
        let target_snapshot = world.inspect(target).unwrap();
        assert!(
            source_snapshot
                .base
                .contains(&ComponentValue::Scalar(Scalar {
                    value: 3.0
                }))
        );
        assert!(
            source_snapshot
                .effective
                .contains(&ComponentValue::Scalar(Scalar {
                    value: 5.0
                }))
        );
        assert!(
            target_snapshot
                .base
                .contains(&ComponentValue::Scalar(Scalar {
                    value: 20.0
                }))
        );
        assert!(
            target_snapshot
                .effective
                .contains(&ComponentValue::Scalar(Scalar {
                    value: 11.0
                }))
        );
        drop(world);
        host.destroy_world(id);
        let values = seen.lock().unwrap();
        assert!(values.contains(&("numeric-before", 3.0)));
        assert!(values.contains(&("typed-update", 11.0)));
        assert!(values.contains(&("finished", 11.0)));
        assert!(values.contains(&("teardown", 11.0)));
    }
}
