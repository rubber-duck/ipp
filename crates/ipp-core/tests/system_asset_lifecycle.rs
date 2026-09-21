//! Actual Host-owned providers and Worlds establish the shared release barrier.

use ipp_core::{HostRuntime, WorldLimits, services::asset_management::*, systems::*};
use std::{
    any::Any,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

type Trace = Arc<Mutex<Vec<String>>>;

struct Payload(Trace);

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

impl Drop for Payload {
    fn drop(&mut self) {
        self.0.lock().unwrap().push("drop".into());
    }
}

struct Loader(Trace);

impl AssetLoader for Loader {
    type Data = Payload;

    fn poll_load(
        &mut self,
        reader: &mut dyn ipp_core::services::data_source::DataReader,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Data, String>> {
        let mut byte = [0];
        match reader.poll_read(cx, &mut byte) {
            Poll::Ready(Ok(1)) => Poll::Ready(Ok(Payload(Arc::clone(&self.0)))),
            Poll::Ready(Ok(_)) => Poll::Ready(Err("fixture input missing".into())),
            Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
            Poll::Pending => Poll::Pending,
        }
    }
}

struct ObserverFactory {
    id: SystemId,
    trace: Trace,
}

struct Observer {
    id: SystemId,
    trace: Trace,
}

impl SystemFactory for ObserverFactory {
    fn id(&self) -> SystemId {
        self.id
    }

    fn create(&self, _: &mut SystemInitContext<'_>) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(Observer {
            id: self.id,
            trace: Arc::clone(&self.trace),
        }))
    }
}

impl System for Observer {
    fn update(&mut self, _: &mut SystemUpdateContext<'_, '_>) {}

    fn before_asset_release(
        &mut self,
        context: &mut SystemAssetContext<'_>,
        event: &AssetLifecycleEvent,
    ) {
        assert!(
            context
                .world
                .asset_resources()
                .get_typed::<Payload>(event.key)
                .is_some()
        );
        self.trace
            .lock()
            .unwrap()
            .push(format!("before:{}:{}", context.world.id().0, self.id.0));
    }

    fn asset_lifecycle(
        &mut self,
        context: &mut SystemAssetContext<'_>,
        event: &AssetLifecycleEvent,
    ) {
        if event.kind == AssetLifecycleKind::Removed {
            assert!(context.world.asset_resources().get(event.key).is_none());
            self.trace.lock().unwrap().push(format!(
                "removed:{}:{}",
                context.world.id().0,
                self.id.0
            ));
        }
    }
}

#[test]
fn every_world_finishes_handlers_before_payload_drop_and_identity_reuse() {
    let trace: Trace = Default::default();
    let mut factories = compiled_system_factories();
    for id in [SystemId("fixture.first"), SystemId("fixture.second")] {
        factories.push(Arc::new(ObserverFactory {
            id,
            trace: Arc::clone(&trace),
        }));
    }
    let mut host = HostRuntime::with_system_factories(factories).unwrap();
    let first = host.create_world(WorldLimits::default()).unwrap();
    let second = host.create_world(WorldLimits::default()).unwrap();
    let loader_trace = Arc::clone(&trace);
    let kind = AssetTypeId(65000);
    host.asset_resources_mut()
        .register_loader(kind, move || Loader(Arc::clone(&loader_trace)))
        .unwrap();
    let identity = AssetUploadIdentity {
        kind,
        asset: 1,
        variant: 0,
    };
    let key = host
        .asset_resources_mut()
        .upload(identity, vec![1])
        .unwrap();
    host.progress_assets();
    assert!(host.asset_resources().get_typed::<Payload>(key).is_some());
    host.asset_resources_mut().release(key);
    assert!(
        host.asset_resources().get_typed::<Payload>(key).is_some(),
        "a release request preserves storage before the Host barrier"
    );
    assert!(trace.lock().unwrap().is_empty());
    host.flush_resource_lifecycle();
    assert_eq!(
        *trace.lock().unwrap(),
        vec![
            format!("before:{}:fixture.first", first.0),
            format!("before:{}:fixture.second", first.0),
            format!("before:{}:fixture.first", second.0),
            format!("before:{}:fixture.second", second.0),
            "drop".into(),
            format!("removed:{}:fixture.first", first.0),
            format!("removed:{}:fixture.second", first.0),
            format!("removed:{}:fixture.first", second.0),
            format!("removed:{}:fixture.second", second.0),
        ]
    );
    let reused = host
        .asset_resources_mut()
        .upload(identity, vec![2])
        .unwrap();
    assert_ne!(key, reused);
    assert!(host.asset_resources().get(key).is_none());
}

struct UpdateReleaseFactory {
    request: Arc<Mutex<Option<AssetKey>>>,
    trace: Trace,
}

struct UpdateRelease {
    request: Arc<Mutex<Option<AssetKey>>>,
    trace: Trace,
}

impl SystemFactory for UpdateReleaseFactory {
    fn id(&self) -> SystemId {
        SystemId("fixture.update-release")
    }

    fn create(&self, _: &mut SystemInitContext<'_>) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(UpdateRelease {
            request: self.request.clone(),
            trace: self.trace.clone(),
        }))
    }
}

impl System for UpdateRelease {
    fn update(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
        let (ecs, mut inputs, _) = context.inputs();
        let assets = inputs.assets.take().unwrap();
        if let Some(key) = self.request.lock().unwrap().take() {
            assert!(assets.get_typed::<Payload>(key).is_some());
            assets.release(key);
        }
        self.trace
            .lock()
            .unwrap()
            .push(format!("update:{}", ecs.id().0));
    }
}

#[test]
fn update_requested_release_waits_for_every_prepared_world() {
    let trace = Trace::default();
    let request = Arc::new(Mutex::new(None));
    let mut factories = compiled_system_factories();
    factories.push(Arc::new(UpdateReleaseFactory {
        request: request.clone(),
        trace: trace.clone(),
    }));
    factories.push(Arc::new(ObserverFactory {
        id: SystemId("fixture.observer"),
        trace: trace.clone(),
    }));
    let mut host = HostRuntime::with_system_factories(factories).unwrap();
    let first = host.create_world(Default::default()).unwrap();
    let second = host.create_world(Default::default()).unwrap();
    let kind = AssetTypeId(65000);
    let output = trace.clone();
    host.asset_resources_mut()
        .register_loader(kind, move || Loader(output.clone()))
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
    *request.lock().unwrap() = Some(key);
    host.world_mut(first).unwrap().prepare_update(0.0).unwrap();
    host.world_mut(second).unwrap().prepare_update(0.0).unwrap();
    host.world_mut(first).unwrap().step(0.0).unwrap();
    host.flush_resource_lifecycle();
    assert!(host.asset_resources().get_typed::<Payload>(key).is_some());
    assert_eq!(*trace.lock().unwrap(), [format!("update:{}", first.0)]);
    host.world_mut(second).unwrap().step(0.0).unwrap();
    host.flush_resource_lifecycle();
    assert_eq!(
        *trace.lock().unwrap(),
        [
            format!("update:{}", first.0),
            format!("update:{}", second.0),
            format!("before:{}:fixture.observer", first.0),
            format!("before:{}:fixture.observer", second.0),
            "drop".into(),
            format!("removed:{}:fixture.observer", first.0),
            format!("removed:{}:fixture.observer", second.0),
        ]
    );
    assert!(host.asset_resources().get(key).is_none());
}
