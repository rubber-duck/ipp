//! Existing numeric property commits preserve observers, identity and failure semantics.
mod support;
use ipp_core::{systems::*, *};
use std::sync::{Arc, Mutex};
use support::WorldTestDriver;

type Patch = Option<(EntityId, Vec<(u32, DynamicValue)>)>;
#[derive(Default)]
struct Shared {
    patch: Patch,
    trace: Vec<(&'static str, f32)>,
    result: Option<Result<(), ErrorReason>>,
}
struct Factory(Arc<Mutex<Shared>>);
struct Writer(Arc<Mutex<Shared>>);

impl SystemFactory for Factory {
    fn id(&self) -> SystemId {
        SystemId("test.numeric-properties")
    }

    fn dependencies(&self) -> &[SystemDependency] {
        &[]
    }

    fn create(&self, _: &mut SystemInitContext<'_>) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(Writer(Arc::clone(&self.0))))
    }
}

impl Writer {
    fn observe(&self, context: &SystemCommitContext<'_>, phase: &'static str) {
        if !context.is_evaluated() {
            return;
        }
        for (entity, component) in context.changed_components() {
            if component != ComponentValue::CUSTOM_MATERIAL {
                continue;
            }
            assert!(context.retains_component(entity, component));
            let value = context
                .world()
                .effective_component(entity, component)
                .unwrap();
            let DynamicValue::F32(value) =
                value.dynamic_properties().unwrap().get("number").unwrap()
            else {
                panic!("numeric property");
            };
            self.0.lock().unwrap().trace.push((phase, value));
        }
    }
}

impl System for Writer {
    fn validate_commit(&self, context: &SystemCommitContext<'_>) -> Result<(), ErrorReason> {
        self.observe(context, "validate");
        Ok(())
    }

    fn before_commit(&mut self, context: &mut SystemCommitContext<'_>) {
        self.observe(context, "before");
    }

    fn after_commit(&mut self, context: &mut SystemCommitContext<'_>) {
        self.observe(context, "after");
    }

    fn update(&mut self, context: &mut SystemUpdateContext<'_, '_>) {
        let patch = self.0.lock().unwrap().patch.take();
        if let Some((entity, fields)) = patch {
            let result = context.world.apply_evaluated_properties(
                self,
                entity,
                ComponentValue::CUSTOM_MATERIAL,
                fields.into_iter().map(|(offset, value)| {
                    (offset, components::schema::FieldValue::Dynamic(value))
                }),
            );
            self.0.lock().unwrap().result = Some(result);
        }
    }
}

#[test]
fn numeric_patch_keeps_old_storage_until_observers_finish_and_rejects_invalid_edits() {
    let shared = Arc::new(Mutex::new(Shared::default()));
    let mut factories = compiled_system_factories();
    factories.push(Arc::new(Factory(Arc::clone(&shared))));
    let mut host = HostRuntime::with_system_factories(factories).unwrap();
    let world_id = host.create_world(Default::default()).unwrap();
    let mut world = host.world_mut(world_id).unwrap();
    let mut value = components::CustomMaterial::default();
    let key = value
        .properties
        .set("number", DynamicValue::F32(1.0))
        .unwrap();
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
                    value: ComponentValue::CustomMaterial(value),
                },
            ],
        })
        .unwrap();
    let entity = world
        .update_for_test(0.0)
        .unwrap()
        .outcomes
        .remove(0)
        .result
        .unwrap()[0]
        .1;
    // The last duplicate wins both in candidate validation and publication.
    shared.lock().unwrap().patch = Some((
        entity,
        vec![(key, DynamicValue::F32(2.0)), (key, DynamicValue::F32(3.0))],
    ));
    world.update_for_test(0.0).unwrap();
    assert_eq!(
        shared.lock().unwrap().trace,
        [("validate", 1.0), ("before", 1.0), ("after", 3.0)]
    );
    assert_eq!(shared.lock().unwrap().result, Some(Ok(())));
    for patch in [
        vec![(key, DynamicValue::F32(f32::NAN))],
        vec![(key, DynamicValue::Vec2([1.0; 2]))],
        vec![(key + 1, DynamicValue::F32(5.0))],
        vec![(
            key,
            DynamicValue::Asset(services::asset_management::AssetSource {
                kind: TEXTURE_TYPE,
                uri: "asset://2/1".into(),
                variant: 0,
            }),
        )],
    ] {
        shared.lock().unwrap().trace.clear();
        shared.lock().unwrap().patch = Some((entity, patch));
        world.update_for_test(0.0).unwrap();
        assert!(shared.lock().unwrap().result.as_ref().unwrap().is_err());
        assert!(shared.lock().unwrap().trace.is_empty());
        let value = world
            .inspect(entity)
            .unwrap()
            .effective
            .into_iter()
            .find(|v| v.type_id() == ComponentValue::CUSTOM_MATERIAL)
            .unwrap();
        assert_eq!(value.dynamic_properties().unwrap().key("number"), Some(key));
        assert_eq!(
            value.dynamic_properties().unwrap().get("number"),
            Some(DynamicValue::F32(3.0))
        );
    }
}
