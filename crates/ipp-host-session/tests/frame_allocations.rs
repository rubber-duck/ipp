//! Allocation accounting against production core (without checked-invariants).
#![cfg(feature = "instrumentation")]

use ipp_core::components::{
    BoundingGeometry, Camera, FlatSurface, Light, PickingGeometry, Transform,
};
use ipp_core::systems::canvas::{CanvasBox, CanvasStyle};
use ipp_core::{
    Batch, Command, ComponentValue as C, EntityRef, HostRuntime, WorldAttachment, profiling,
};

#[global_allocator]
static ALLOCATOR: profiling::CountingAllocator = profiling::CountingAllocator;

static CAPTURE: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn unchanged_world_frames_reuse_storage() {
    let _capture = CAPTURE.lock().unwrap();
    let mut host = HostRuntime::new();
    let systems: Vec<_> = host.system_ids().collect();
    let parent = host.create_world(Default::default(), &systems).unwrap();
    let child = host.create_world(Default::default(), &systems).unwrap();
    let attachment = WorldAttachment::spatial(host.world_ref(child).unwrap());
    for world in [parent, child] {
        let mut operations = vec![Command::Create {
            alias: 0,
            metadata: Default::default(),
            adopt: false,
        }];
        for component in [
            C::Transform(Transform::default()),
            C::Camera(Camera::default()),
            C::BoundingGeometry(BoundingGeometry::default()),
            C::PickingGeometry(PickingGeometry::default()),
            C::CanvasStyle(CanvasStyle::default()),
            C::CanvasBox(CanvasBox::default()),
            C::FlatSurface(FlatSurface::default()),
            C::Light(Light::default()),
        ] {
            operations.push(Command::insert_value(EntityRef::Alias(0), component));
        }
        if world == parent {
            operations.push(Command::insert_value(
                EntityRef::Alias(0),
                C::WorldAttachment(attachment.clone()),
            ));
        }
        host.world_mut(world)
            .unwrap()
            .enqueue(Batch {
                id: 1,
                operations,
            })
            .unwrap();
    }
    let mut report = ipp_core::HostFrameReport::default();
    host.frame_into(0.0, &mut report).unwrap();
    for result in report.worlds.values() {
        result.as_ref().unwrap().outcomes[0]
            .result
            .as_ref()
            .unwrap();
    }
    assert!(report.publication_errors.is_empty());
    for _ in 0..10 {
        host.frame_into(1.0 / 60.0, &mut report).unwrap();
    }

    let mut previous = host.latest_publication(parent).unwrap();
    profiling::reset_for_host(true, host.identity());
    for _ in 0..20 {
        host.frame_into(1.0 / 60.0, &mut report).unwrap();
        assert!(report.publication_errors.is_empty());
        let current = host.latest_publication(parent).unwrap();
        assert_ne!(current, previous);
        previous = current;
    }
    profiling::pause();
    let allocations = profiling::allocations();
    profiling::visit_capture(
        |stage| {
            if stage.values[2] > 0 {
                eprintln!("{stage:?}");
            }
            true
        },
        |name, _, values| {
            if values[0] > 0 {
                eprintln!("{name}: {values:?}");
            }
            true
        },
    );
    profiling::release_capture();
    assert_eq!(allocations, (0, 0));
}

struct Platform;

impl ipp_host_session::HostServices for Platform {
    const NAME: &'static str = "steady-frame-test";

    fn initialize(
        _: &mut HostRuntime,
        _: &ipp_host_session::services::task_scheduler::TaskSchedulers,
    ) -> Result<Self, String> {
        Ok(Self)
    }

    fn service_resources(&mut self, _: &mut HostRuntime) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn steady_session_progress_uses_inline_storage() {
    let _capture = CAPTURE.lock().unwrap();
    let mut host = ipp_host_session::Host::<Platform>::new().unwrap();
    host.open_session(1, &[]).unwrap();
    host.session_mut(1)
        .unwrap()
        .receive(&ipp_protocol::contract::HELLO)
        .unwrap();
    drop(host.session_mut(1).unwrap().take_response().unwrap());
    for _ in 0..10 {
        host.tick(1.0 / 60.0).unwrap();
        drop(host.session_mut(1).unwrap().take_response().unwrap());
    }
    profiling::reset_for_host(true, host.runtime().identity());
    for _ in 0..20 {
        host.tick(1.0 / 60.0).unwrap();
        let (bytes, completion) = host
            .session_mut(1)
            .unwrap()
            .take_response()
            .unwrap()
            .into_parts();
        assert!(matches!(bytes, ipp_host_session::ResponseBytes::Frame(_)));
        drop(completion);
    }
    profiling::pause();
    let allocations = profiling::allocations();
    profiling::release_capture();
    assert_eq!(allocations, (0, 0));
}
