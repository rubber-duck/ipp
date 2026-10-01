//! Symbolic references, adoption and compare-and-set through real Host batches:
//! each resolves or compares at its own operation boundary, and a refused
//! operation has no effect and stops its batch.

use ipp_core::components::Scalar;
use ipp_core::{
    Batch, BatchOutcome, Command, ComponentValue, EntityId, EntityMetadata, EntityRef, ErrorReason,
    FieldValue, FieldWrite, HostRuntime, OperationEffect, WorldId,
};
use std::mem::offset_of;
use std::sync::Arc;

fn apply(host: &mut HostRuntime, world: WorldId, operations: Vec<Command>) -> BatchOutcome {
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

fn named(alias: u32, symbol: &str, adopt: bool) -> Command {
    Command::Create {
        alias,
        metadata: EntityMetadata {
            symbolic_id: Some(symbol.into()),
            classes: vec!["declared".into()],
        },
        adopt,
    }
}

fn scalar(entity: EntityRef, value: f32, adopt: bool) -> Command {
    Command::InsertComponent {
        entity,
        component: ComponentValue::SCALAR,
        fields: vec![write(value)],
        adopt,
    }
}

fn write(value: f32) -> FieldWrite {
    FieldWrite {
        offset: offset_of!(Scalar, value) as u32,
        value: FieldValue::F32(value),
    }
}

fn set(entity: EntityRef, value: f32) -> Command {
    Command::SetField {
        entity,
        component: ComponentValue::SCALAR,
        field: write(value),
    }
}

fn set_if(entity: EntityRef, expected: f32, value: f32) -> Command {
    Command::set_field_if(
        entity,
        ComponentValue::SCALAR,
        write(value),
        FieldValue::F32(expected),
    )
}

fn value(host: &mut HostRuntime, world: WorldId, entity: EntityId) -> Option<f32> {
    host.world_mut(world)
        .unwrap()
        .inspect(entity)?
        .components
        .into_iter()
        .find_map(|component| match component {
            ComponentValue::Scalar(Scalar {
                value,
            }) => Some(value),
            _ => None,
        })
}

fn symbol(name: &str) -> EntityRef {
    EntityRef::Symbol(name.into())
}

fn fixture() -> (HostRuntime, WorldId, EntityId) {
    let mut host = HostRuntime::new();
    let world = host
        .create_world(
            Default::default(),
            &[
                ipp_core::systems::constraints::ConstraintSystem::ID,
                ipp_core::systems::hierarchy::HierarchySystem::ID,
            ],
        )
        .unwrap();
    let outcome = apply(
        &mut host,
        world,
        vec![
            named(1, "gauge", false),
            scalar(EntityRef::Alias(1), 1.0, false),
        ],
    );
    let entity = outcome.result.unwrap()[0].1;
    (host, world, entity)
}

#[test]
fn symbolic_references_resolve_at_each_operation_and_report_their_handles() {
    let (mut host, world, gauge) = fixture();

    // A symbol resolves against earlier operations of the same batch, and each
    // distinct pair is reported once in first-resolution order.
    let outcome = apply(
        &mut host,
        world,
        vec![
            set(symbol("gauge"), 2.0),
            named(2, "needle", false),
            scalar(symbol("needle"), 5.0, false),
            set(symbol("gauge"), 3.0),
        ],
    );
    let created = outcome.result.as_ref().unwrap();
    let needle = created[0].1;
    assert_eq!(
        outcome.symbols,
        [(Arc::from("gauge"), gauge), (Arc::from("needle"), needle)]
    );
    assert_eq!(value(&mut host, world, gauge), Some(3.0));
    assert_eq!(value(&mut host, world, needle), Some(5.0));
}

#[test]
fn a_moved_symbol_reports_each_handle_within_the_counted_bound() {
    let (mut host, world, gauge) = fixture();
    let unnamed = |entity| Command::SetMetadata {
        entity,
        metadata: EntityMetadata::default(),
    };
    let mut operations = vec![
        set(symbol("gauge"), 2.0),
        unnamed(symbol("gauge")),
        named(1, "gauge", false),
        scalar(symbol("gauge"), 7.0, false),
        set(symbol("gauge"), 8.0),
    ];
    let mut reports = ipp_core::BatchSymbolReports::default();
    for command in &mut operations {
        reports.add(command);
    }

    // The symbol resolves to two handles, the most its one creation allows,
    // so a Host that bounds the count before applying bounds the outcome.
    let outcome = apply(&mut host, world, operations);
    let moved = outcome.result.unwrap()[0].1;
    assert_eq!(
        outcome.symbols,
        [(Arc::from("gauge"), gauge), (Arc::from("gauge"), moved)]
    );
    assert_eq!(reports.reports(), outcome.symbols.len());
}

#[test]
fn a_missing_symbol_fails_its_operation_without_effect_and_stops_the_batch() {
    let (mut host, world, gauge) = fixture();
    let outcome = apply(
        &mut host,
        world,
        vec![
            set(symbol("gauge"), 2.0),
            set(symbol("absent"), 9.0),
            set(symbol("gauge"), 4.0),
        ],
    );
    let error = outcome.result.unwrap_err();
    assert_eq!(error.operation, Some(1));
    assert_eq!(error.reason, ErrorReason::MissingSymbolicId);
    assert_eq!(outcome.symbols, [(Arc::from("gauge"), gauge)]);
    // The applied prefix stays; nothing after the failure applies.
    assert_eq!(value(&mut host, world, gauge), Some(2.0));
}

#[test]
fn a_refused_duplicate_create_allocates_nothing_and_stops_the_batch() {
    let (mut host, world, gauge) = fixture();
    let before = host.world_mut(world).unwrap().entities().len();

    // The duplicate is refused at its own operation; the later operation never
    // applies and no entity, alias or symbol report remains of the refusal.
    let outcome = apply(
        &mut host,
        world,
        vec![named(1, "gauge", false), named(2, "later", false)],
    );
    let error = outcome.result.unwrap_err();
    assert_eq!(error.operation, Some(0));
    assert_eq!(error.reason, ErrorReason::DuplicateSymbolicId);
    assert!(error.aliases.is_empty());
    let context = host.world_mut(world).unwrap();
    assert_eq!(context.entities().len(), before);
    assert_eq!(context.lookup_id("gauge"), Some(gauge));
    assert_eq!(context.lookup_id("later"), None);
    drop(context);

    // The next creation receives the identity that directly follows the
    // fixture's entity, as if the refusal never happened.
    let next = apply(&mut host, world, vec![named(3, "next", false)])
        .result
        .unwrap()[0]
        .1;
    let mut reference = HostRuntime::new();
    let reference_world = reference
        .create_world(
            Default::default(),
            &[
                ipp_core::systems::constraints::ConstraintSystem::ID,
                ipp_core::systems::hierarchy::HierarchySystem::ID,
            ],
        )
        .unwrap();
    let expected = apply(
        &mut reference,
        reference_world,
        vec![named(1, "gauge", false), named(2, "next", false)],
    )
    .result
    .unwrap()[1]
        .1;
    assert_eq!(next, expected);
    assert_eq!(host.world_mut(world).unwrap().entities().len(), before + 1);
}

#[test]
fn adopting_create_binds_the_live_symbolic_entity_and_reports_adoption() {
    let (mut host, world, gauge) = fixture();

    // Without adoption a second entity may not claim the symbol.
    let duplicate = apply(&mut host, world, vec![named(1, "gauge", false)]);
    assert_eq!(
        duplicate.result.unwrap_err().reason,
        ErrorReason::DuplicateSymbolicId
    );

    // Adoption binds the alias to the live entity, writes the declared
    // metadata over it and reports the adoption at its operation index.
    let outcome = apply(
        &mut host,
        world,
        vec![
            named(1, "gauge", true),
            set(EntityRef::Alias(1), 6.0),
            named(2, "fresh", true),
        ],
    );
    let created = outcome.result.unwrap();
    assert_eq!(created[0], (1, gauge));
    let fresh = created[1].1;
    assert_ne!(fresh, gauge);
    assert_eq!(outcome.effects.len(), 1);
    assert_eq!(outcome.effects[0].operation, 0);
    assert_eq!(outcome.effects[0].effect, OperationEffect::Adopted);
    let context = host.world_mut(world).unwrap();
    assert_eq!(context.lookup_id("gauge"), Some(gauge));
    assert_eq!(
        context.inspect(gauge).unwrap().metadata.classes,
        vec![String::from("declared")]
    );
    assert_eq!(context.lookup_id("fresh"), Some(fresh));
    drop(context);
    assert_eq!(value(&mut host, world, gauge), Some(6.0));
}

#[test]
fn adopting_insert_writes_fields_in_place_and_an_invalid_adoption_has_no_effect() {
    let (mut host, world, gauge) = fixture();
    let incarnation = |host: &mut HostRuntime| {
        host.world_mut(world)
            .unwrap()
            .component_incarnation(gauge, ComponentValue::SCALAR)
    };
    let before = incarnation(&mut host);

    let outcome = apply(
        &mut host,
        world,
        vec![scalar(EntityRef::Handle(gauge), 4.0, true)],
    );
    assert!(outcome.result.is_ok(), "{outcome:?}");
    assert_eq!(outcome.effects.len(), 1);
    assert_eq!(outcome.effects[0].effect, OperationEffect::Adopted);
    assert_eq!(value(&mut host, world, gauge), Some(4.0));
    assert_eq!(incarnation(&mut host), before);

    // An invalid adopted write is refused as a whole and leaves the value.
    let outcome = apply(
        &mut host,
        world,
        vec![scalar(EntityRef::Handle(gauge), f32::NAN, true)],
    );
    assert_eq!(
        outcome.result.unwrap_err().reason,
        ErrorReason::InvalidValue
    );
    assert!(outcome.effects.is_empty());
    assert_eq!(value(&mut host, world, gauge), Some(4.0));
    assert_eq!(incarnation(&mut host), before);

    // Without adoption the insert replaces the component with a new lifetime.
    apply(
        &mut host,
        world,
        vec![scalar(EntityRef::Handle(gauge), 5.0, false)],
    )
    .result
    .unwrap();
    assert_eq!(value(&mut host, world, gauge), Some(5.0));
    assert_ne!(incarnation(&mut host), before);
}

#[test]
fn compare_and_set_writes_only_the_expected_value_and_a_mismatch_stops_the_batch() {
    let (mut host, world, gauge) = fixture();
    let entity = EntityRef::Handle(gauge);

    let outcome = apply(&mut host, world, vec![set_if(entity.clone(), 1.0, 2.0)]);
    assert!(outcome.result.is_ok(), "{outcome:?}");
    assert_eq!(value(&mut host, world, gauge), Some(2.0));

    let outcome = apply(
        &mut host,
        world,
        vec![set_if(entity.clone(), 1.0, 3.0), set(entity.clone(), 8.0)],
    );
    let error = outcome.result.unwrap_err();
    assert_eq!(error.operation, Some(0));
    assert_eq!(error.reason, ErrorReason::ValueMismatch);
    assert_eq!(value(&mut host, world, gauge), Some(2.0));

    // The expected value must have the field's type; an absent component fails.
    let outcome = apply(
        &mut host,
        world,
        vec![Command::set_field_if(
            entity.clone(),
            ComponentValue::SCALAR,
            write(3.0),
            FieldValue::U32(2),
        )],
    );
    assert_eq!(
        outcome.result.unwrap_err().reason,
        ErrorReason::InvalidField
    );
    let outcome = apply(
        &mut host,
        world,
        vec![Command::set_field_if(
            entity,
            ComponentValue::TRANSFORM,
            FieldWrite {
                offset: 0,
                value: FieldValue::F32(1.0),
            },
            FieldValue::F32(0.0),
        )],
    );
    assert_eq!(
        outcome.result.unwrap_err().reason,
        ErrorReason::MissingComponent
    );
    assert_eq!(value(&mut host, world, gauge), Some(2.0));
}
