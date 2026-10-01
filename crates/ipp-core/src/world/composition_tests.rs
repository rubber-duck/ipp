use crate::{
    Batch, Command, ComponentValue, EntityRef, ErrorReason, HostRuntime, WorldAttachment, WorldId,
    systems::*,
};
use std::sync::{Arc, Mutex};

#[test]
fn attachment_producer_edits_cannot_change_the_traversal_inside_evaluation() {
    #[derive(Default)]
    struct Probe {
        attachment: Option<(crate::EntityId, WorldAttachment)>,
        results: Vec<Result<(), ErrorReason>>,
        world: Option<WorldId>,
    }

    struct Factory(Arc<Mutex<Probe>>);
    struct Mutator(Arc<Mutex<Probe>>);

    impl SystemFactory for Factory {
        fn id(&self) -> SystemId {
            SystemId("fixture.attachment-phase")
        }

        fn create(
            &self,
            _: &mut SystemInitContext<'_>,
        ) -> Result<Box<dyn System>, SystemInitError> {
            Ok(Box::new(Mutator(self.0.clone())))
        }
    }

    impl System for Mutator {
        fn update(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
            let command = {
                let probe = self.0.lock().unwrap();
                if probe.world != Some(context.world.id()) {
                    return;
                }
                let (entity, value) = probe.attachment.clone().unwrap();
                Command::insert_value(
                    EntityRef::Handle(entity),
                    ComponentValue::WorldAttachment(value),
                )
            };
            let result = context
                .world
                .apply_authored_commands(Some(self), &[command]);
            self.0.lock().unwrap().results.push(result);
        }
    }

    let probe = Arc::new(Mutex::new(Probe::default()));
    let mut factories = compiled_system_factories();
    factories.push(Arc::new(Factory(probe.clone())));
    let mut host = HostRuntime::with_system_factories(factories).unwrap();
    let parent = host
        .create_world(
            Default::default(),
            &[
                crate::systems::world_attachment::WorldAttachmentSystem::ID,
                SystemId("fixture.attachment-phase"),
            ],
        )
        .unwrap();
    let child = host.create_world(Default::default(), &[]).unwrap();
    host.world_mut(parent)
        .unwrap()
        .enqueue(Batch {
            id: 1,
            operations: vec![Command::Create {
                alias: 0,
                metadata: Default::default(),
                adopt: false,
            }],
        })
        .unwrap();

    let entity = host
        .frame(0.1)
        .unwrap()
        .worlds
        .remove(&parent)
        .unwrap()
        .unwrap()
        .outcomes
        .remove(0)
        .result
        .unwrap()[0]
        .1;
    let attachment = WorldAttachment::spatial(host.world_ref(child).unwrap());
    let command = Command::insert_value(
        EntityRef::Handle(entity),
        ComponentValue::WorldAttachment(attachment.clone()),
    );
    *probe.lock().unwrap() = Probe {
        world: Some(parent),
        attachment: Some((entity, attachment)),
        results: Vec::new(),
    };
    let report = host.frame(0.1).unwrap();
    assert!(report.publication_errors.is_empty());
    assert_eq!(
        probe.lock().unwrap().results,
        [Err(ErrorReason::InvalidValue)]
    );
    assert!(
        host.publication(host.latest_publication(parent).unwrap())
            .unwrap()
            .attachments
            .is_empty()
    );
    probe.lock().unwrap().world = None;
    host.world_mut(parent)
        .unwrap()
        .enqueue(Batch {
            id: 2,
            operations: vec![command],
        })
        .unwrap();
    host.frame(0.1).unwrap();
    assert_eq!(
        host.publication(host.latest_publication(parent).unwrap())
            .unwrap()
            .attachments
            .len(),
        1
    );
}

#[test]
fn completed_publications_obey_strengthened_release_before_payload_drop() {
    strengthened_release(false);
}

#[test]
fn graphics_release_strengthened_to_revoke_invalidates_completed_cpu_publications() {
    strengthened_release(true);
}

fn strengthened_release(graphics: bool) {
    use crate::services::asset_management::*;
    use std::any::Any;

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

    struct Factory(Trace, Arc<Mutex<Option<AssetKey>>>);
    struct Producer(Trace, Arc<Mutex<Option<AssetKey>>>);

    impl SystemFactory for Factory {
        fn id(&self) -> SystemId {
            SystemId("fixture.leased-output")
        }

        fn create(
            &self,
            _: &mut SystemInitContext<'_>,
        ) -> Result<Box<dyn System>, SystemInitError> {
            Ok(Box::new(Producer(self.0.clone(), self.1.clone())))
        }
    }

    impl System for Producer {
        fn update(&mut self, _: &mut SystemUpdateContext<'_, '_>) {}

        fn publish_output(
            &self,
            world: &crate::WorldContext<'_>,
            output: &mut crate::WorldOutputBuilder<'_>,
        ) -> Result<(), ErrorReason> {
            if let Some(key) = *self.1.lock().unwrap()
                && world.asset_resources().get_typed::<Payload>(key).is_some()
            {
                output.retain(key);
                output.chunk(SystemId("fixture.leased-output"), vec![1_u8]);
            }
            Ok(())
        }

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
            self.0.lock().unwrap().push(format!(
                "before:{}:{:?}",
                context.world.id().0,
                event.kind
            ));
            if context.world.id().0 == 2
                && matches!(
                    event.kind,
                    AssetLifecycleKind::StatusChanged | AssetLifecycleKind::GraphicsInvalidated
                )
            {
                context.world.asset_acquisition.revoke_resource(event.key);
            }
        }
    }

    let trace = Trace::default();
    let selected = Arc::new(Mutex::new(None));
    let mut host = HostRuntime::with_system_factories(vec![Arc::new(Factory(
        trace.clone(),
        selected.clone(),
    ))])
    .unwrap();
    let first = host
        .create_world(Default::default(), &[SystemId("fixture.leased-output")])
        .unwrap();
    let second = host
        .create_world(Default::default(), &[SystemId("fixture.leased-output")])
        .unwrap();
    let loader_trace = trace.clone();
    let kind = AssetTypeId(65001);
    host.asset_resources_mut()
        .register_loader(kind, move || {
            let trace = loader_trace.clone();
            BufferedAssetLoader::new(move |_: &[u8]| Ok(Payload(trace.clone())))
        })
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
    *selected.lock().unwrap() = Some(key);
    host.progress_assets();
    host.frame(0.1).unwrap();
    let first_output = host.latest_publication(first).unwrap();
    let second_output = host.latest_publication(second).unwrap();
    assert!(host.publication_resource(first_output, key).is_some());
    assert!(host.publication_resource(second_output, key).is_some());
    if graphics {
        host.asset_resources_mut().invalidate_graphics(key);
    } else {
        host.asset_resources_mut().unload(key);
    }
    host.flush_resource_lifecycle();
    assert_eq!(
        *trace.lock().unwrap(),
        [
            if graphics {
                "before:1:GraphicsInvalidated"
            } else {
                "before:1:StatusChanged"
            },
            if graphics {
                "before:2:GraphicsInvalidated"
            } else {
                "before:2:StatusChanged"
            },
            "before:1:Removed",
            "before:2:Removed",
            "drop"
        ]
    );
    assert!(host.publication(first_output).is_none() && host.publication(second_output).is_none());
    assert!(host.publication_resource(first_output, key).is_none());
    assert!(host.asset_resources().get(key).is_none());
    let report = host.frame(0.1).unwrap();
    assert_eq!(report.evaluation_order, [first, second]);
    for world in [first, second] {
        let publication = host.latest_publication(world).unwrap();
        assert!(host.publication_resource(publication, key).is_none());
    }
    let replacement = host
        .asset_resources_mut()
        .upload(identity, vec![1])
        .unwrap();
    assert_eq!(replacement.slot, key.slot);
    assert_ne!(replacement, key);
    host.progress_assets();
    assert!(
        host.publication_resource(first_output, replacement)
            .is_none()
    );
}
