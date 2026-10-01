use super::*;
use ipp_core::{Batch, Command, ComponentValue, EntityRef, FieldValue, HostRuntime, OutputKind};

fn decode_field(bytes: &[u8]) -> Result<FieldValue, ProtocolError> {
    Reader {
        bytes,
        at: 0,
    }
    .field()
    .map(|field| field.value)
}

fn apply_queued(
    host: &mut HostRuntime,
    world: ipp_core::WorldId,
    batch: Batch,
) -> Result<Vec<(u32, ipp_core::EntityId)>, ipp_core::BatchError> {
    host.world_mut(world).unwrap().enqueue(batch).unwrap();

    host.frame(0.0)
        .unwrap()
        .worlds
        .remove(&world)
        .unwrap()
        .unwrap()
        .outcomes
        .remove(0)
        .result
}

#[test]
fn runtime_reference_fields_decode_syntax_without_claiming_lifetime_authority() {
    let mut host = HostRuntime::new();
    let world = host
        .create_world(
            Default::default(),
            &[
                ipp_core::systems::animation::AnimationSystem::ID,
                ipp_core::systems::asset_dependencies::AssetDependencySystem::ID,
                ipp_core::systems::hierarchy::HierarchySystem::ID,
                ipp_core::systems::look_at::LookAtSystem::ID,
                ipp_core::systems::hierarchy::FinalPropagationSystem::ID,
                ipp_core::systems::geometry::GeometrySystem::ID,
                ipp_core::systems::camera::CameraSystem::ID,
            ],
        )
        .unwrap();
    let reference = host.world_ref(world).unwrap();
    let entity = apply_queued(
        &mut host,
        world,
        Batch {
            id: 1,
            operations: vec![
                Command::Create {
                    alias: 0,
                    metadata: Default::default(),
                    adopt: false,
                },
                Command::insert_value(
                    EntityRef::Alias(0),
                    ComponentValue::Camera(Default::default()),
                ),
            ],
        },
    )
    .unwrap()[0]
        .1;

    let output = host
        .bind_output(reference, entity, OutputKind::Camera)
        .unwrap();
    let mut world_bytes = Writer::new(Vec::new());
    world_bytes
        .resolved_field(
            0,
            ipp_core::components::schema::FieldValue::World(Some(reference)),
        )
        .unwrap();
    let mut output_bytes = Writer::new(Vec::new());
    output_bytes
        .resolved_field(
            0,
            ipp_core::components::schema::FieldValue::Output(Some(output)),
        )
        .unwrap();
    let world_token = FieldValue::UnresolvedWorld(ipp_core::WorldReferenceToken::untrusted(
        world.0,
        reference.incarnation(),
    ));
    let output_token = FieldValue::UnresolvedOutput(ipp_core::OutputReferenceToken::untrusted(
        ipp_core::WorldReferenceToken::untrusted(world.0, reference.incarnation()),
        output.target(),
    ));
    assert_eq!(decode_field(&world_bytes.0), Ok(world_token.clone()));
    assert_eq!(decode_field(&output_bytes.0), Ok(output_token.clone()));
    for bytes in [&world_bytes.0, &output_bytes.0] {
        for length in 0..bytes.len() {
            assert!(decode_field(&bytes[..length]).is_err());
        }
        let mut malformed = bytes.clone();
        malformed[5] = 2;
        assert!(decode_field(&malformed).is_err());
    }
    let mut invalid_target = output_bytes.0.clone();
    invalid_target[22] = 255;
    assert!(decode_field(&invalid_target).is_err());
    assert!(
        apply_queued(
            &mut host,
            world,
            Batch {
                id: 2,
                operations: vec![
                    Command::RemoveComponent {
                        entity: EntityRef::Handle(entity),
                        component: ComponentValue::CAMERA
                    },
                    Command::insert_value(
                        EntityRef::Handle(entity),
                        ComponentValue::Camera(Default::default())
                    ),
                ]
            },
        )
        .is_ok()
    );

    assert_eq!(decode_field(&output_bytes.0), Ok(output_token));
    assert_eq!(decode_field(&world_bytes.0), Ok(world_token.clone()));
    assert_ne!(
        host.bind_output(reference, entity, OutputKind::Camera)
            .unwrap(),
        output
    );
    host.destroy_world(world);
    assert_eq!(decode_field(&world_bytes.0), Ok(world_token));
}

#[test]
fn stale_tokens_do_not_bypass_complete_message_syntax_validation() {
    let mut bytes = Writer::new(Vec::new());
    bytes.u64(1).unwrap();
    bytes.u64(2).unwrap();
    bytes.u8(REQUEST_SUBMIT_BATCH).unwrap();
    bytes.u32(3).unwrap();
    bytes.u8(1).unwrap();
    bytes.u32(2).unwrap();
    bytes.u8(COMMAND_CREATE).unwrap();
    bytes.u32(7).unwrap();
    bytes.u8(OPTION_NONE).unwrap();
    bytes.u32(0).unwrap();
    bytes.u8(0).unwrap();
    bytes.u8(COMMAND_SET).unwrap();
    bytes.u8(REF_ALIAS).unwrap();
    bytes.u32(7).unwrap();
    bytes.u16(ComponentValue::WORLD_ATTACHMENT).unwrap();
    bytes.u32(0).unwrap();
    bytes.u8(VALUE_WORLD).unwrap();
    bytes.u8(1).unwrap();
    bytes
        .world_reference(crate::references::WorldReference {
            id: 999,
            incarnation: 999,
        })
        .unwrap();
    assert!(decode_request(&bytes.0, 1).is_ok());
    bytes.u8(255).unwrap();
    assert!(matches!(
        decode_request(&bytes.0, 1),
        Err(ProtocolError::Malformed(_))
    ));
}

#[test]
fn world_canvas_references_resolve_by_world_lifetime_and_canvas_selection() {
    use crate::references::OutputReference;
    let mut host = HostRuntime::new();
    let canvas_world = host
        .create_world(
            Default::default(),
            &[ipp_core::systems::canvas::CanvasSystem::ID],
        )
        .unwrap();
    let plain_world = host.create_world(Default::default(), &[]).unwrap();
    let canvas = host.world_ref(canvas_world).unwrap();
    let plain = host.world_ref(plain_world).unwrap();

    let reference = OutputReference::canvas(canvas.into());
    assert_eq!(
        reference.resolve(&host),
        Ok(ipp_core::OutputRef::canvas(canvas))
    );
    assert_eq!(
        OutputReference::from(ipp_core::OutputRef::canvas(canvas)),
        reference
    );
    // A World without the Canvas System has no canvas.
    assert_eq!(
        OutputReference::canvas(plain.into()).resolve(&host),
        Err(ProtocolError::InvalidReference)
    );

    // An Output field naming the World canvas decodes to an unresolved token.
    let mut bytes = Writer::new(Vec::new());
    bytes
        .resolved_field(
            0,
            ipp_core::components::schema::FieldValue::Output(Some(ipp_core::OutputRef::canvas(
                canvas,
            ))),
        )
        .unwrap();
    assert_eq!(
        decode_field(&bytes.0),
        Ok(FieldValue::UnresolvedOutput(
            ipp_core::OutputReferenceToken::untrusted(
                ipp_core::WorldReferenceToken::untrusted(canvas_world.0, canvas.incarnation()),
                ipp_core::OutputTarget::Canvas,
            )
        ))
    );

    // The canvas lives as long as its World.
    host.destroy_world(canvas_world);
    assert_eq!(
        reference.resolve(&host),
        Err(ProtocolError::InvalidReference)
    );
}
