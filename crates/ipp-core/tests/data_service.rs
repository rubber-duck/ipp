//! Local Host/World composition evidence; these tests do not exercise transports or bindings.

use ipp_core::{
    EntityId, HostRuntime, WorldLimits,
    components::{DynamicPropertyKind, DynamicValue},
    services::data::*,
    systems::*,
};
use std::sync::{Arc, Mutex};

type Observations = Arc<Mutex<Vec<(f64, Vec<u64>, bool)>>>;

struct DataReaderFactory(Observations);

impl SystemFactory for DataReaderFactory {
    fn id(&self) -> SystemId {
        SystemId("test.data-reader")
    }

    fn dependencies(&self) -> &[SystemDependency] {
        DataReader::dependencies()
    }

    fn create(
        &self,
        context: &mut SystemInitContext<'_>,
    ) -> Result<Box<dyn System>, SystemInitError> {
        let bindings = SystemBindings::resolve(context)?;
        let consumer = context
            .data
            .register_consumer(
                DataConsumerIdentity {
                    world: context.world.reference(),
                    entity: EntityId::default(),
                    binding_incarnation: 1,
                },
                DataConsumerRequest {
                    name: "dataset:local".into(),
                    kind: DataSourceKind::Streaming,
                    windows: vec![DataWindow::Count(2)],
                },
            )
            .map_err(|error| SystemInitError::Message(error.to_string()))?;
        Ok(Box::new(DataReader {
            consumer,
            bindings,
            observations: self.0.clone(),
            notified: false,
        }))
    }
}

struct DataReader {
    consumer: DataConsumerHandle,
    bindings: SystemBindings<Self>,
    observations: Observations,
    notified: bool,
}

#[ipp_core::systems::system_update]
impl DataReader {
    fn update(&mut self, _ecs: SystemEcsAccess<'_>, data: &DataService, _dt: f64) {
        let rows = data
            .read_consumer(self.consumer)
            .unwrap()
            .rows()
            .map(|row| row.id.0)
            .collect();
        self.observations.lock().unwrap().push((
            data.time(),
            rows,
            std::mem::take(&mut self.notified),
        ));
    }
}

impl System for DataReader {
    fn prepare_evaluation(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
        self.notified = context
            .world
            .take_data_notification(self.consumer)
            .unwrap()
            .is_some();
    }

    ipp_core::system_update!(bindings);
}

fn producer(host: &mut HostRuntime) -> DataProducerHandle {
    host.data_sources_mut()
        .create_source(
            "dataset:local".into(),
            DataSourceKind::Streaming,
            DataSchema {
                columns: vec![DataColumn::new("value", DynamicPropertyKind::U32)],
            },
        )
        .unwrap()
}

fn append(host: &mut HostRuntime, producer: DataProducerHandle, count: u32) {
    host.data_sources_mut()
        .apply_batch(
            producer,
            [DataDelta::Append {
                rows: (0..count)
                    .map(|value| vec![DynamicValue::U32(value)])
                    .collect(),
            }],
        )
        .unwrap();
}

#[test]
fn generated_service_parameter_and_mutation_notification_use_host_owned_data() {
    let observations = Observations::default();
    let mut host =
        HostRuntime::with_system_factories(vec![Arc::new(DataReaderFactory(observations.clone()))])
            .unwrap();
    let world = host
        .create_world(WorldLimits::default(), &[SystemId("test.data-reader")])
        .unwrap();
    let producer = producer(&mut host);
    append(&mut host, producer, 3);
    host.frame(0.5).unwrap();
    host.frame(0.5).unwrap();
    assert_eq!(
        *observations.lock().unwrap(),
        [(0.5, vec![2, 3], true), (1.0, vec![2, 3], false)]
    );
    assert!(host.destroy_world(world));
    assert!(
        host.data_sources()
            .read_source(producer.source())
            .unwrap()
            .is_empty()
    );
    append(&mut host, producer, 1);
    assert!(
        host.data_sources()
            .read_source(producer.source())
            .unwrap()
            .is_empty()
    );
}

struct FailingFactory;

impl SystemFactory for FailingFactory {
    fn id(&self) -> SystemId {
        SystemId("test.fail-after-data-reader")
    }

    fn dependencies(&self) -> &[SystemDependency] {
        &[SystemDependency::Required(SystemId("test.data-reader"))]
    }

    fn create(
        &self,
        _context: &mut SystemInitContext<'_>,
    ) -> Result<Box<dyn System>, SystemInitError> {
        Err(SystemInitError::Message(
            "intentional initialization failure".into(),
        ))
    }
}

#[test]
fn partial_world_initialization_releases_its_data_demand() {
    let mut host = HostRuntime::with_system_factories(vec![
        Arc::new(DataReaderFactory(Observations::default())),
        Arc::new(FailingFactory),
    ])
    .unwrap();
    let producer = producer(&mut host);
    assert!(
        host.create_world(
            WorldLimits::default(),
            &[
                SystemId("test.data-reader"),
                SystemId("test.fail-after-data-reader")
            ]
        )
        .is_err()
    );
    append(&mut host, producer, 3);
    assert!(
        host.data_sources()
            .read_source(producer.source())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn preparing_before_producer_update_does_not_miss_final_invalidation() {
    let observations = Observations::default();
    let mut host =
        HostRuntime::with_system_factories(vec![Arc::new(DataReaderFactory(observations.clone()))])
            .unwrap();
    let world = host
        .create_world(WorldLimits::default(), &[SystemId("test.data-reader")])
        .unwrap();
    let producer = producer(&mut host);
    append(&mut host, producer, 1);
    host.frame(0.0).unwrap();
    host.world_mut(world).unwrap().prepare_update(0.5).unwrap();
    assert!(host.has_pending_world_updates());
    // Pre-ingress admission imposes no producer lock. The cut is still ahead.
    append(&mut host, producer, 2);
    host.frame(0.5).unwrap();
    host.frame(0.0).unwrap();
    assert_eq!(
        *observations.lock().unwrap(),
        [
            (0.0, vec![1], true),
            (0.5, vec![2, 3], true),
            (0.5, vec![2, 3], false),
        ]
    );
}

type PhaseObservations = Arc<Mutex<Vec<(&'static str, &'static str, f64, Vec<u64>, bool)>>>;

struct PhaseReaderFactory {
    id: &'static str,
    windows: Vec<DataWindow>,
    observations: PhaseObservations,
}

impl SystemFactory for PhaseReaderFactory {
    fn id(&self) -> SystemId {
        SystemId(self.id)
    }

    fn dependencies(&self) -> &[SystemDependency] {
        if self.id == "test.data-second" {
            &[SystemDependency::Required(SystemId("test.data-first"))]
        } else {
            &[]
        }
    }

    fn create(
        &self,
        context: &mut SystemInitContext<'_>,
    ) -> Result<Box<dyn System>, SystemInitError> {
        let consumer = context
            .data
            .register_consumer(
                DataConsumerIdentity {
                    world: context.world.reference(),
                    entity: EntityId::default(),
                    binding_incarnation: if self.id == "test.data-first" {
                        1
                    } else {
                        2
                    },
                },
                DataConsumerRequest {
                    name: "dataset:local".into(),
                    kind: DataSourceKind::Streaming,
                    windows: self.windows.clone(),
                },
            )
            .unwrap();
        Ok(Box::new(PhaseReader {
            id: self.id,
            consumer,
            notified: false,
            observations: self.observations.clone(),
        }))
    }
}

struct PhaseReader {
    id: &'static str,
    consumer: DataConsumerHandle,
    notified: bool,
    observations: PhaseObservations,
}

impl PhaseReader {
    fn observe(&self, context: &SystemUpdateContext<'_, '_>, phase: &'static str) {
        let data = context.world.data_sources();
        let rows = data
            .read_consumer(self.consumer)
            .unwrap()
            .rows()
            .map(|row| row.id.0)
            .collect();
        self.observations
            .lock()
            .unwrap()
            .push((self.id, phase, data.time(), rows, self.notified));
    }
}

impl System for PhaseReader {
    fn accept_ingress(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
        // Deliberately observe early as the root timing probe does. This cannot
        // replace taking the final notification after service progress/ingress.
        self.notified = context
            .world
            .take_data_notification(self.consumer)
            .unwrap()
            .is_some();
    }

    fn prepare_evaluation(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
        self.notified |= context
            .world
            .take_data_notification(self.consumer)
            .unwrap()
            .is_some();
        self.observe(context, "prepare");
    }

    fn update(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
        self.observe(context, "evaluate");
    }

    fn finish_update(
        &mut self,
        context: &mut SystemUpdateContext<'_, '_>,
        _report: &mut ipp_core::WorldUpdateReport,
    ) {
        self.observe(context, "finish");
        self.notified = false;
    }
}

fn phase_host(
    windows: Vec<DataWindow>,
    observations: &PhaseObservations,
) -> (HostRuntime, ipp_core::WorldId) {
    let mut host = HostRuntime::with_system_factories(vec![
        Arc::new(PhaseReaderFactory {
            id: "test.data-first",
            windows: windows.clone(),
            observations: observations.clone(),
        }),
        Arc::new(PhaseReaderFactory {
            id: "test.data-second",
            windows,
            observations: observations.clone(),
        }),
    ])
    .unwrap();
    let world = host
        .create_world(
            WorldLimits::default(),
            &[SystemId("test.data-first"), SystemId("test.data-second")],
        )
        .unwrap();
    (host, world)
}

fn assert_phase_cut(observations: &PhaseObservations, time: f64, rows: &[u64], notified: bool) {
    let observations = observations.lock().unwrap();
    assert_eq!(observations.len(), 6);
    for ((id, phase, observed_time, observed_rows, changed), (expected_id, expected_phase)) in
        observations.iter().zip([
            ("test.data-first", "prepare"),
            ("test.data-second", "prepare"),
            ("test.data-first", "evaluate"),
            ("test.data-second", "evaluate"),
            ("test.data-first", "finish"),
            ("test.data-second", "finish"),
        ])
    {
        assert_eq!((*id, *phase), (expected_id, expected_phase));
        assert_eq!(*observed_time, time);
        assert_eq!(observed_rows, rows);
        assert_eq!(*changed, notified);
    }
}

#[test]
fn two_readers_across_phases_see_one_cut_after_pre_ingress_admission() {
    let observations = PhaseObservations::default();
    let (mut host, world) = phase_host(vec![DataWindow::Count(2)], &observations);
    let producer = producer(&mut host);
    append(&mut host, producer, 2);
    host.frame(0.0).unwrap();
    observations.lock().unwrap().clear();
    host.world_mut(world).unwrap().prepare_update(0.5).unwrap();
    append(&mut host, producer, 2);
    host.frame(0.5).unwrap();
    assert_phase_cut(&observations, 0.5, &[3, 4], true);
}

#[test]
fn prepared_before_host_time_expiry_notifies_both_readers_at_final_boundary() {
    let observations = PhaseObservations::default();
    let (mut host, world) = phase_host(
        vec![DataWindow::Range {
            column: "value".into(),
            width: 0.5,
            anchor: DataWindowAnchor::HostTime {
                units_per_second: 1.0,
            },
        }],
        &observations,
    );
    let producer = producer(&mut host);
    append(&mut host, producer, 1);
    host.frame(0.0).unwrap();
    assert_phase_cut(&observations, 0.0, &[1], true);
    observations.lock().unwrap().clear();
    host.world_mut(world).unwrap().prepare_update(1.0).unwrap();
    host.frame(1.0).unwrap();
    assert_phase_cut(&observations, 1.0, &[], true);
    observations.lock().unwrap().clear();
    host.frame(0.0).unwrap();
    assert_phase_cut(&observations, 1.0, &[], false);
}
