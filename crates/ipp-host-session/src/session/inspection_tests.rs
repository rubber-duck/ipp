use crate::{Host, HostServices, WorldSession};
use ipp_core::components::CustomMaterial;
use ipp_core::{Batch, Command, ComponentValue, DynamicValue, EntityId, EntityRef, WorldId};
use ipp_protocol::world::{InspectionQuery, ResponseBody};

struct Platform;

impl HostServices for Platform {
    const NAME: &'static str = "inspection-test";

    fn initialize(
        _: &mut ipp_core::HostRuntime,
        _schedulers: &crate::services::task_scheduler::TaskSchedulers,
    ) -> Result<Self, String> {
        Ok(Self)
    }

    fn service_resources(&mut self, _: &mut ipp_core::HostRuntime) -> Result<(), String> {
        Ok(())
    }
}

/// Names within the 64 KiB protocol string bound; twenty exceed one message.
const NAME_BYTES: usize = 60_000;
const LARGE_PROPERTIES: usize = 20;

fn large(index: usize) -> String {
    format!("large_{index}_{}", "x".repeat(NAME_BYTES))
}

fn apply(
    host: &mut Host<Platform>,
    world: WorldId,
    operations: Vec<Command>,
) -> Vec<(u32, EntityId)> {
    host.runtime
        .world_mut(world)
        .unwrap()
        .enqueue(Batch {
            id: 99,
            operations,
        })
        .unwrap();

    host.runtime
        .frame(0.0)
        .unwrap()
        .worlds
        .remove(&world)
        .unwrap()
        .unwrap()
        .outcomes
        .remove(0)
        .result
        .unwrap()
}

fn property(entity: EntityId, name: String) -> Command {
    Command::SetDynamicProperty {
        entity: EntityRef::Handle(entity),
        component: ComponentValue::CUSTOM_MATERIAL,
        name,
        value: DynamicValue::F32(0.5),
    }
}

fn entities(host: &mut Host<Platform>) -> ResponseBody {
    host.session_mut(1).unwrap().inspection_page(
        InspectionQuery {
            collection: 1,
            ..Default::default()
        },
        1,
        0,
        0.0,
    )
}

#[test]
fn oversized_entity_record_is_an_explicit_error_not_a_truncation() {
    let mut host = Host::<Platform>::new().unwrap();
    let world = host
        .runtime
        .create_world(Default::default(), crate::test_support::TEST_RENDER_SYSTEMS)
        .unwrap();
    host.sessions
        .insert(1, WorldSession::new(1, world, true, false));
    let entity = apply(
        &mut host,
        world,
        vec![
            Command::Create {
                alias: 1,
                metadata: Default::default(),
                adopt: false,
            },
            Command::insert_value(
                EntityRef::Alias(1),
                ComponentValue::CustomMaterial(CustomMaterial::default()),
            ),
        ],
    )[0]
    .1;
    apply(&mut host, world, vec![property(entity, "kept".into())]);
    for index in 0..LARGE_PROPERTIES {
        apply(&mut host, world, vec![property(entity, large(index))]);
    }

    let ResponseBody::Error {
        code,
        message,
    } = entities(&mut host)
    else {
        panic!("an oversized inspection record must fail explicitly");
    };
    assert_eq!(code, 1);
    assert!(
        message.starts_with("Inspection record cannot be encoded"),
        "{message}"
    );

    apply(
        &mut host,
        world,
        (0..LARGE_PROPERTIES)
            .map(|index| Command::RemoveDynamicProperty {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::CUSTOM_MATERIAL,
                name: large(index),
            })
            .collect(),
    );
    let ResponseBody::Inspect {
        entities,
        next,
        ..
    } = entities(&mut host)
    else {
        panic!("the reduced record must encode again");
    };
    assert_eq!(next, 0);
    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0].id, entity);
}

#[test]
fn canvas_collection_reports_the_committed_state_and_the_last_evaluated_extent() {
    let mut host = Host::<Platform>::new().unwrap();
    let state = ipp_core::CanvasState {
        extent: [320.0, 180.0],
        units_per_metre: 40.0,
    };
    let world = host
        .runtime
        .create_world_with_options(
            Default::default(),
            ipp_core::WorldCreateOptions {
                canvas: Some(state),
                ..ipp_core::WorldCreateOptions::new([ipp_core::systems::canvas::CanvasSystem::ID])
            },
        )
        .unwrap();
    host.sessions
        .insert(1, WorldSession::new(1, world, true, false));
    let canvas = |host: &mut Host<Platform>| {
        let ResponseBody::Inspect {
            canvas,
            ..
        } = host.session_mut(1).unwrap().inspection_page(
            InspectionQuery {
                collection: 8,
                ..Default::default()
            },
            1,
            0,
            0.0,
        )
        else {
            panic!("Canvas inspection page");
        };
        canvas.unwrap()
    };
    let before = canvas(&mut host);
    assert_eq!(before.state, state);
    assert_eq!(before.evaluated, None);

    host.runtime.frame(0.0).unwrap();
    let evaluated = canvas(&mut host);
    assert_eq!(evaluated.state, state);
    assert_eq!(evaluated.evaluated.unwrap().extent, [320.0, 180.0]);

    // Other collections carry no Canvas record.
    let ResponseBody::Inspect {
        canvas: absent,
        ..
    } = entities(&mut host)
    else {
        panic!("entity inspection page");
    };
    assert_eq!(absent, None);
}
