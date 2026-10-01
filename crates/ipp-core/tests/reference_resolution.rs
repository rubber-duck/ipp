//! Ordered admission of untrusted runtime reference tokens against live Host state.

mod support;

use support::selection::{ATTACHMENTS, CAMERA, select};

use ipp_core::{
    Batch, Command, ComponentValue, EntityRef, ErrorReason, FieldValue, FieldWrite, HostRuntime,
    OutputKind, OutputReferenceToken, WorldAttachment, WorldId, WorldReferenceToken,
};

fn create(alias: u32) -> Command {
    Command::Create {
        alias,
        metadata: Default::default(),
        adopt: false,
    }
}

fn attachment(alias: u32, field: FieldValue) -> Command {
    Command::InsertComponent {
        entity: EntityRef::Alias(alias),
        component: ComponentValue::WORLD_ATTACHMENT,
        fields: vec![FieldWrite {
            offset: std::mem::offset_of!(WorldAttachment, child) as u32,
            value: field,
        }],
        adopt: false,
    }
}

fn apply(
    host: &mut HostRuntime,
    world: WorldId,
    operations: Vec<Command>,
) -> ipp_core::BatchOutcome {
    host.world_mut(world)
        .unwrap()
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();
    host.frame(0.0)
        .unwrap()
        .worlds
        .remove(&world)
        .unwrap()
        .unwrap()
        .outcomes
        .remove(0)
}

#[test]
fn stale_world_failure_retains_prefix_aliases_and_prior_attachment_receipt() {
    let mut host = HostRuntime::new();
    let parent = host.create_world(Default::default(), ATTACHMENTS).unwrap();
    let child = host.create_world(Default::default(), &[]).unwrap();
    let child_ref = host.world_ref(child).unwrap();
    let live = FieldValue::UnresolvedWorld(WorldReferenceToken::untrusted(
        child.0,
        child_ref.incarnation(),
    ));
    let stale = FieldValue::UnresolvedWorld(WorldReferenceToken::untrusted(
        child.0,
        child_ref.incarnation() + 1,
    ));
    let outcome = apply(
        &mut host,
        parent,
        vec![
            create(1),
            attachment(1, live),
            create(2),
            attachment(2, stale),
        ],
    );
    let error = outcome.result.unwrap_err();
    assert_eq!(error.operation, Some(3));
    assert_eq!(error.reason, ErrorReason::InvalidEntity);
    assert_eq!(error.aliases.len(), 2);
    assert_eq!(outcome.effects.len(), 1);
    assert_eq!(outcome.effects[0].operation, 1);
    assert_eq!(host.world_mut(parent).unwrap().entities().len(), 2);
}

#[test]
fn queued_reference_is_revalidated_after_child_destruction() {
    let mut host = HostRuntime::new();
    let parent = host.create_world(Default::default(), ATTACHMENTS).unwrap();
    let child = host.create_world(Default::default(), &[]).unwrap();
    let child_ref = host.world_ref(child).unwrap();
    host.world_mut(parent)
        .unwrap()
        .enqueue(Batch {
            id: 9,
            operations: vec![
                create(1),
                attachment(
                    1,
                    FieldValue::UnresolvedWorld(WorldReferenceToken::untrusted(
                        child.0,
                        child_ref.incarnation(),
                    )),
                ),
            ],
        })
        .unwrap();
    host.destroy_world(child);
    let frame = host.frame(0.0).unwrap();
    let error = frame.worlds[&parent].as_ref().unwrap().outcomes[0]
        .result
        .as_ref()
        .unwrap_err();
    assert_eq!(error.operation, Some(1));
    assert_eq!(error.aliases.len(), 1);
}

#[test]
fn local_output_checks_current_prefix_and_foreign_output_checks_live_producer() {
    let mut host = HostRuntime::new();
    let parent = host.create_world(Default::default(), ATTACHMENTS).unwrap();
    let child = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, CAMERA]))
        .unwrap();
    let outcome = apply(
        &mut host,
        child,
        vec![
            create(1),
            Command::insert_value(
                EntityRef::Alias(1),
                ComponentValue::Camera(Default::default()),
            ),
        ],
    );
    let entity = outcome.result.unwrap()[0].1;
    let child_ref = host.world_ref(child).unwrap();
    let output = host
        .bind_output(child_ref, entity, OutputKind::Camera)
        .unwrap();
    let field = FieldValue::UnresolvedOutput(OutputReferenceToken::untrusted(
        WorldReferenceToken::untrusted(child.0, child_ref.incarnation()),
        output.target(),
    ));
    let write = || Command::InsertComponent {
        entity: EntityRef::Alias(2),
        component: ComponentValue::WORLD_ATTACHMENT,
        fields: vec![FieldWrite {
            offset: std::mem::offset_of!(WorldAttachment, output) as u32,
            value: field.clone(),
        }],
        adopt: false,
    };
    let outcome = apply(
        &mut host,
        child,
        vec![
            Command::RemoveComponent {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::CAMERA,
            },
            create(2),
            write(),
        ],
    );
    let error = outcome.result.unwrap_err();
    assert_eq!(error.operation, Some(2));
    assert_eq!(error.aliases.len(), 1);
    let outcome = apply(&mut host, parent, vec![create(2), write()]);
    assert_eq!(outcome.result.unwrap_err().operation, Some(1));
}
