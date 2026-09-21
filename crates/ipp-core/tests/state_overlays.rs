//! Production World lifecycle evidence; transport/React coverage lives in the parent harness.

mod support;
use support::WorldTestDriver;

use ipp_core::{
    Batch, Command, ComponentOverlayMode, ComponentValue, EntityId, EntityMetadata,
    EntityOverlayMode, EntityRef, ErrorReason, FieldValue, FieldWrite,
    MAX_STATE_OVERLAY_DIAGNOSTICS, StateOverlayHandleKind, StateOverlayLifecycleReason,
    StateOverlayRef, WorldLimits, WorldUpdateReport, components::Scalar,
};

fn scalar_id() -> u16 {
    ComponentValue::SCALAR
}

fn write(value: f32) -> FieldWrite {
    FieldWrite {
        offset: std::mem::offset_of!(Scalar, value) as u32,
        value: FieldValue::F32(value),
    }
}

fn run(world: &mut ipp_core::WorldContext<'_>, operations: Vec<Command>) -> WorldUpdateReport {
    world
        .enqueue(Batch {
            id: world.tick() + 1,
            operations,
        })
        .unwrap();
    world.update_for_test(0.0).unwrap()
}

fn ok(world: &mut ipp_core::WorldContext<'_>, operations: Vec<Command>) -> WorldUpdateReport {
    let report = run(world, operations);
    assert!(report.outcomes[0].result.is_ok(), "{report:?}");
    report
}

fn reject(
    world: &mut ipp_core::WorldContext<'_>,
    operations: Vec<Command>,
    reason: ErrorReason,
) -> WorldUpdateReport {
    let report = run(world, operations);
    assert_eq!(
        report.outcomes[0].result.as_ref().unwrap_err().reason,
        reason
    );
    report
}

fn create(world: &mut ipp_core::WorldContext<'_>, name: &str, value: Option<f32>) -> EntityId {
    let mut operations = vec![Command::Create {
        alias: 0,
        metadata: EntityMetadata {
            symbolic_id: Some(name.into()),
            classes: Vec::new(),
        },
    }];
    if let Some(value) = value {
        operations.push(Command::InsertComponent {
            entity: EntityRef::Alias(0),
            component: scalar_id(),
            fields: vec![write(value)],
        });
    }
    ok(world, operations).outcomes[0].result.as_ref().unwrap()[0].1
}

fn insert(entity: EntityId, value: f32) -> Command {
    Command::InsertComponent {
        entity: EntityRef::Handle(entity),
        component: scalar_id(),
        fields: vec![write(value)],
    }
}

fn remove(entity: EntityId) -> Command {
    Command::RemoveComponent {
        entity: EntityRef::Handle(entity),
        component: scalar_id(),
    }
}

fn base_write(entity: EntityId, value: f32) -> Command {
    Command::SetField {
        entity: EntityRef::Handle(entity),
        component: scalar_id(),
        field: write(value),
    }
}

fn attach(
    owner: StateOverlayRef,
    binding: StateOverlayRef,
    alias: u32,
    mode: ComponentOverlayMode,
    value: Option<f32>,
) -> Command {
    Command::AttachComponentStateOverlay {
        owner,
        binding,
        alias,
        component: scalar_id(),
        mode,
        fields: value.into_iter().map(write).collect(),
    }
}

fn bind(owner: StateOverlayRef, alias: u32, name: &str) -> Command {
    Command::AttachEntityOverlayBinding {
        owner,
        alias,
        symbolic_id: name.into(),
        mode: EntityOverlayMode::Bound,
    }
}

#[derive(Clone, Copy)]
struct Declaration {
    owner: u64,
    binding: u64,
    overlay: u64,
}

impl Declaration {
    fn update(self, value: f32) -> Command {
        Command::UpdateComponentStateOverlay {
            owner: StateOverlayRef::Handle(self.owner),
            overlay: StateOverlayRef::Handle(self.overlay),
            fields: vec![write(value)],
            clear: Vec::new(),
        }
    }

    fn clear(self) -> Command {
        Command::UpdateComponentStateOverlay {
            owner: StateOverlayRef::Handle(self.owner),
            overlay: StateOverlayRef::Handle(self.overlay),
            fields: Vec::new(),
            clear: vec![write(0.0).offset],
        }
    }

    fn release(self) -> Command {
        Command::ReleaseComponentStateOverlay {
            owner: StateOverlayRef::Handle(self.owner),
            overlay: StateOverlayRef::Handle(self.overlay),
        }
    }

    fn release_owner(self) -> Command {
        Command::ReleaseStateOverlayOwner {
            owner: StateOverlayRef::Handle(self.owner),
        }
    }
}

fn declare(
    world: &mut ipp_core::WorldContext<'_>,
    name: &str,
    mode: ComponentOverlayMode,
    value: Option<f32>,
) -> Declaration {
    let report = ok(
        world,
        vec![
            Command::CreateStateOverlayOwner {
                alias: 0,
            },
            bind(StateOverlayRef::Alias(0), 1, name),
            attach(
                StateOverlayRef::Alias(0),
                StateOverlayRef::Alias(1),
                2,
                mode,
                value,
            ),
        ],
    );
    let resources = &report.outcomes[0].state_overlays;
    Declaration {
        owner: resources[0].id,
        binding: resources[1].id,
        overlay: resources[2].id,
    }
}

fn values(world: &ipp_core::WorldContext<'_>, entity: EntityId) -> (Option<f32>, Option<f32>) {
    let snapshot = world.inspect(entity).unwrap();
    let scalar = |components: Vec<ComponentValue>| match components.first() {
        Some(ComponentValue::Scalar(scalar)) => Some(scalar.value),
        _ => None,
    };
    (scalar(snapshot.base), scalar(snapshot.effective))
}

#[test]
fn sparse_updates_preserve_attachment_order_and_reveal_latest_base() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let entity = create(&mut world, "target", Some(1.0));
    let lower = declare(
        &mut world,
        "target",
        ComponentOverlayMode::Bound,
        Some(10.0),
    );
    let upper = declare(&mut world, "target", ComponentOverlayMode::Auto, Some(20.0));
    assert_eq!(values(&world, entity), (Some(1.0), Some(20.0)));

    ok(
        &mut world,
        vec![base_write(entity, 3.0), lower.update(11.0)],
    );
    assert_eq!(values(&world, entity), (Some(3.0), Some(20.0)));
    ok(&mut world, vec![upper.clear()]);
    assert_eq!(values(&world, entity), (Some(3.0), Some(11.0)));
    ok(&mut world, vec![lower.clear(), base_write(entity, 4.0)]);
    assert_eq!(values(&world, entity), (Some(4.0), Some(4.0)));
    ok(&mut world, vec![upper.update(21.0), lower.update(12.0)]);
    assert_eq!(values(&world, entity), (Some(4.0), Some(21.0)));
    ok(&mut world, vec![upper.release_owner()]);
    assert_eq!(values(&world, entity), (Some(4.0), Some(12.0)));
    ok(&mut world, vec![lower.release_owner()]);
    assert_eq!(values(&world, entity), (Some(4.0), Some(4.0)));
}

#[test]
fn auto_shares_fallback_and_follows_producer_without_resubmission() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let entity = create(&mut world, "target", None);
    let lower = declare(&mut world, "target", ComponentOverlayMode::Auto, Some(10.0));
    let upper = declare(&mut world, "target", ComponentOverlayMode::Auto, Some(20.0));
    assert_eq!(values(&world, entity), (None, Some(20.0)));
    reject(
        &mut world,
        vec![base_write(entity, 1.0)],
        ErrorReason::MissingComponent,
    );

    for value in [1.0, 2.0] {
        assert!(
            ok(&mut world, vec![insert(entity, value)])
                .diagnostics
                .is_empty()
        );
        assert_eq!(values(&world, entity), (Some(value), Some(20.0)));
    }
    assert!(ok(&mut world, vec![remove(entity)]).diagnostics.is_empty());
    assert_eq!(values(&world, entity), (None, Some(20.0)));
    ok(&mut world, vec![upper.release_owner()]);
    assert_eq!(values(&world, entity), (None, Some(10.0)));
    ok(&mut world, vec![lower.clear()]);
    assert_eq!(values(&world, entity), (None, Some(0.0)));
    ok(&mut world, vec![insert(entity, 3.0)]);
    assert_eq!(values(&world, entity), (Some(3.0), Some(3.0)));
    ok(&mut world, vec![remove(entity), lower.release_owner()]);
    assert_eq!(values(&world, entity), (None, None));
}

#[test]
fn strict_fallback_binding_has_no_demand_and_never_reacquires() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let entity = create(&mut world, "target", None);
    let auto = declare(&mut world, "target", ComponentOverlayMode::Auto, Some(5.0));
    let strict = declare(&mut world, "target", ComponentOverlayMode::Bound, Some(9.0));
    let report = ok(&mut world, vec![auto.release()]);
    assert_eq!(values(&world, entity), (None, None));
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(report.diagnostics[0].state_overlay, strict.overlay);
    assert_eq!(
        report.diagnostics[0].reason,
        StateOverlayLifecycleReason::ComponentRemoved
    );
    assert!(world.update_for_test(0.0).unwrap().diagnostics.is_empty());

    ok(&mut world, vec![insert(entity, 7.0)]);
    reject(
        &mut world,
        vec![strict.update(100.0)],
        ErrorReason::InvalidStateOverlay,
    );
    assert_eq!(values(&world, entity), (Some(7.0), Some(7.0)));
    ok(
        &mut world,
        vec![
            strict.release(),
            strict.release(),
            auto.release_owner(),
            strict.release_owner(),
        ],
    );
    assert_eq!(values(&world, entity), (Some(7.0), Some(7.0)));
}

#[test]
fn every_base_fallback_transition_invalidates_strict_incarnations_only() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let entity = create(&mut world, "target", None);
    let auto = declare(&mut world, "target", ComponentOverlayMode::Auto, Some(5.0));
    for transition in [insert(entity, 1.0), insert(entity, 2.0), remove(entity)] {
        let strict = declare(&mut world, "target", ComponentOverlayMode::Bound, Some(9.0));
        let report = ok(&mut world, vec![transition]);
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].reason,
            StateOverlayLifecycleReason::ComponentReplaced
        );
        assert_eq!(report.diagnostics[0].state_overlay, strict.overlay);
        assert_eq!(report.diagnostics[0].owner, strict.owner);
        assert_eq!(report.diagnostics[0].entity, entity);
        assert_eq!(report.diagnostics[0].component, Some(scalar_id()));
        assert_eq!(values(&world, entity).1, Some(5.0));
        ok(&mut world, vec![strict.release_owner()]);
    }
    ok(&mut world, vec![auto.release_owner()]);
    assert_eq!(values(&world, entity), (None, None));
}

#[test]
fn owned_component_default_provenance_survives_field_writes_but_not_replacement() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let entity = create(&mut world, "target", None);
    let owned = declare(&mut world, "target", ComponentOverlayMode::Owned, Some(8.0));
    assert_eq!(values(&world, entity), (Some(0.0), Some(8.0)));
    ok(&mut world, vec![base_write(entity, 3.0), owned.clear()]);
    assert_eq!(values(&world, entity), (Some(3.0), Some(3.0)));
    ok(&mut world, vec![owned.release()]);
    assert_eq!(values(&world, entity), (None, None));

    let replacement_owner = declare(&mut world, "target", ComponentOverlayMode::Owned, Some(9.0));
    let auto = declare(&mut world, "target", ComponentOverlayMode::Auto, Some(10.0));
    let report = ok(&mut world, vec![insert(entity, 4.0)]);
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(
        report.diagnostics[0].state_overlay,
        replacement_owner.overlay
    );
    assert_eq!(
        report.diagnostics[0].reason,
        StateOverlayLifecycleReason::ComponentReplaced
    );
    ok(
        &mut world,
        vec![replacement_owner.release(), owned.release(), auto.release()],
    );
    assert_eq!(values(&world, entity), (Some(4.0), Some(4.0)));
}

#[test]
fn releasing_owned_component_transitions_to_shared_fallback_atomically() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let entity = create(&mut world, "target", None);
    let owned = declare(&mut world, "target", ComponentOverlayMode::Owned, Some(8.0));
    let auto = declare(&mut world, "target", ComponentOverlayMode::Auto, Some(10.0));
    let strict = declare(
        &mut world,
        "target",
        ComponentOverlayMode::Bound,
        Some(12.0),
    );
    let report = ok(&mut world, vec![owned.release_owner()]);
    assert_eq!(values(&world, entity), (None, Some(10.0)));
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(report.diagnostics[0].state_overlay, strict.overlay);
    assert_eq!(
        report.diagnostics[0].reason,
        StateOverlayLifecycleReason::ComponentReplaced
    );
    ok(
        &mut world,
        vec![auto.release_owner(), strict.release_owner()],
    );
    assert_eq!(values(&world, entity), (None, None));
}

#[test]
fn entity_deletion_invalidates_all_modes_and_cleanup_preserves_reused_symbol_and_slot() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let report = ok(
        &mut world,
        vec![
            Command::CreateStateOverlayOwner {
                alias: 0,
            },
            Command::AttachEntityOverlayBinding {
                owner: StateOverlayRef::Alias(0),
                alias: 1,
                symbolic_id: "target".into(),
                mode: EntityOverlayMode::Owned,
            },
        ],
    );
    let owner = report.outcomes[0].state_overlays[0].id;
    let binding = report.outcomes[0].state_overlays[1].id;
    let entity = report.outcomes[0].state_overlays[1].entity.unwrap();
    let auto = declare(&mut world, "target", ComponentOverlayMode::Auto, Some(8.0));
    let strict = declare(
        &mut world,
        "target",
        ComponentOverlayMode::Bound,
        Some(10.0),
    );
    let report = ok(
        &mut world,
        vec![Command::Delete {
            entity: EntityRef::Handle(entity),
        }],
    );
    assert_eq!(report.diagnostics.len(), 5);
    assert!(
        report
            .diagnostics
            .iter()
            .all(|d| d.reason == StateOverlayLifecycleReason::EntityDeleted)
    );
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.state_overlay == binding && d.component.is_none())
    );
    assert!(world.inspect(entity).is_none());

    let replacement = create(&mut world, "target", Some(4.0));
    assert_eq!(replacement.index(), entity.index());
    assert_ne!(replacement, entity);
    reject(
        &mut world,
        vec![auto.update(99.0)],
        ErrorReason::InvalidStateOverlay,
    );
    ok(
        &mut world,
        vec![
            Command::ReleaseStateOverlayOwner {
                owner: StateOverlayRef::Handle(owner),
            },
            auto.release_owner(),
            strict.release_owner(),
        ],
    );
    assert_eq!(values(&world, replacement), (Some(4.0), Some(4.0)));
}

#[test]
fn binding_release_is_association_scoped_and_owned_release_deletes_original_after_rename() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let entity = create(&mut world, "target", Some(1.0));
    let first = declare(&mut world, "target", ComponentOverlayMode::Auto, Some(2.0));
    let report = ok(
        &mut world,
        vec![
            bind(StateOverlayRef::Handle(first.owner), 1, "target"),
            attach(
                StateOverlayRef::Handle(first.owner),
                StateOverlayRef::Alias(1),
                2,
                ComponentOverlayMode::Bound,
                Some(3.0),
            ),
        ],
    );
    let second_overlay = report.outcomes[0].state_overlays[1].id;
    ok(
        &mut world,
        vec![Command::ReleaseEntityOverlayBinding {
            owner: StateOverlayRef::Handle(first.owner),
            binding: StateOverlayRef::Handle(first.binding),
        }],
    );
    assert_eq!(values(&world, entity), (Some(1.0), Some(3.0)));
    ok(
        &mut world,
        vec![
            Command::ReleaseComponentStateOverlay {
                owner: StateOverlayRef::Handle(first.owner),
                overlay: StateOverlayRef::Handle(second_overlay),
            },
            first.release_owner(),
        ],
    );
    assert_eq!(values(&world, entity), (Some(1.0), Some(1.0)));

    let report = ok(
        &mut world,
        vec![
            Command::CreateStateOverlayOwner {
                alias: 0,
            },
            Command::AttachEntityOverlayBinding {
                owner: StateOverlayRef::Alias(0),
                alias: 1,
                symbolic_id: "owned".into(),
                mode: EntityOverlayMode::Owned,
            },
        ],
    );
    let owner = report.outcomes[0].state_overlays[0].id;
    let owned = report.outcomes[0].state_overlays[1].entity.unwrap();
    ok(
        &mut world,
        vec![Command::SetMetadata {
            entity: EntityRef::Handle(owned),
            metadata: EntityMetadata {
                symbolic_id: Some("renamed".into()),
                classes: vec![],
            },
        }],
    );
    let independent = create(&mut world, "owned", Some(4.0));
    ok(
        &mut world,
        vec![Command::ReleaseStateOverlayOwner {
            owner: StateOverlayRef::Handle(owner),
        }],
    );
    assert!(world.inspect(owned).is_none());
    assert_eq!(values(&world, independent), (Some(4.0), Some(4.0)));
}

#[test]
fn failed_lifecycle_batch_keeps_releases_invalidations_and_partial_owner_aliases() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let entity = create(&mut world, "target", Some(1.0));
    let strict = declare(&mut world, "target", ComponentOverlayMode::Bound, Some(5.0));
    let auto = declare(&mut world, "target", ComponentOverlayMode::Auto, Some(7.0));
    let failure = reject(
        &mut world,
        vec![
            remove(entity),
            auto.release(),
            Command::CreateStateOverlayOwner {
                alias: 0,
            },
            bind(StateOverlayRef::Alias(0), 1, "target"),
            attach(
                StateOverlayRef::Alias(0),
                StateOverlayRef::Alias(1),
                2,
                ComponentOverlayMode::Owned,
                Some(f32::NAN),
            ),
        ],
        ErrorReason::InvalidValue,
    );
    assert_eq!(
        failure.outcomes[0].result.as_ref().unwrap_err().operation,
        Some(4)
    );
    assert_eq!(values(&world, entity), (None, None));
    assert_eq!(failure.diagnostics[0].state_overlay, strict.overlay);
    let aliases = &failure.outcomes[0].state_overlays;
    assert_eq!(aliases.len(), 2);
    let owner = aliases[0].id;
    assert!(world.state_overlay_owner_is_live(owner));
    ok(
        &mut world,
        vec![
            Command::ReleaseStateOverlayOwner {
                owner: StateOverlayRef::Handle(owner),
            },
            insert(entity, 4.0),
        ],
    );
    assert!(!world.state_overlay_owner_is_live(owner));
    assert_eq!(values(&world, entity), (Some(4.0), Some(4.0)));
}

#[test]
fn scopes_kinds_aliases_and_stale_generations_are_authoritatively_checked() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let entity = create(&mut world, "target", Some(1.0));
    let first = declare(&mut world, "target", ComponentOverlayMode::Bound, Some(2.0));
    let second = declare(&mut world, "target", ComponentOverlayMode::Bound, Some(3.0));
    reject(
        &mut world,
        vec![Command::ReleaseComponentStateOverlay {
            owner: StateOverlayRef::Handle(first.owner),
            overlay: StateOverlayRef::Handle(second.overlay),
        }],
        ErrorReason::StateOverlayOwnershipMismatch,
    );
    reject(
        &mut world,
        vec![Command::ReleaseEntityOverlayBinding {
            owner: StateOverlayRef::Handle(first.owner),
            binding: StateOverlayRef::Handle(first.overlay),
        }],
        ErrorReason::InvalidStateOverlay,
    );
    reject(
        &mut world,
        vec![Command::ReleaseStateOverlayOwner {
            owner: StateOverlayRef::Handle(first.binding),
        }],
        ErrorReason::InvalidStateOverlay,
    );
    reject(
        &mut world,
        vec![attach(
            StateOverlayRef::Handle(first.owner),
            StateOverlayRef::Handle(second.binding),
            0,
            ComponentOverlayMode::Auto,
            None,
        )],
        ErrorReason::StateOverlayOwnershipMismatch,
    );
    reject(
        &mut world,
        vec![Command::ReleaseStateOverlayOwner {
            owner: StateOverlayRef::Handle(u64::MAX),
        }],
        ErrorReason::InvalidStateOverlay,
    );
    reject(
        &mut world,
        vec![Command::ReleaseStateOverlayOwner {
            owner: StateOverlayRef::Alias(0),
        }],
        ErrorReason::UnknownAlias,
    );
    reject(
        &mut world,
        vec![
            Command::CreateStateOverlayOwner {
                alias: 0,
            },
            Command::CreateStateOverlayOwner {
                alias: 0,
            },
        ],
        ErrorReason::DuplicateAlias,
    );

    ok(&mut world, vec![first.release()]);
    let report = ok(
        &mut world,
        vec![attach(
            StateOverlayRef::Handle(second.owner),
            StateOverlayRef::Handle(second.binding),
            0,
            ComponentOverlayMode::Bound,
            Some(9.0),
        )],
    );
    let replacement = report.outcomes[0].state_overlays[0].id;
    assert_eq!(replacement as u32, first.overlay as u32);
    assert_ne!(replacement, first.overlay);
    ok(&mut world, vec![first.release()]);
    reject(
        &mut world,
        vec![first.update(20.0)],
        ErrorReason::InvalidStateOverlay,
    );
    assert_eq!(values(&world, entity), (Some(1.0), Some(9.0)));
}

#[test]
fn attachment_mismatches_and_invalid_field_edits_are_atomic() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let entity = create(&mut world, "target", None);
    let owner = ok(
        &mut world,
        vec![Command::CreateStateOverlayOwner {
            alias: 0,
        }],
    )
    .outcomes[0]
        .state_overlays[0]
        .id;
    reject(
        &mut world,
        vec![bind(StateOverlayRef::Handle(owner), 0, "absent")],
        ErrorReason::MissingSymbolicId,
    );
    reject(
        &mut world,
        vec![Command::AttachEntityOverlayBinding {
            owner: StateOverlayRef::Handle(owner),
            alias: 0,
            symbolic_id: "target".into(),
            mode: EntityOverlayMode::Owned,
        }],
        ErrorReason::DuplicateSymbolicId,
    );
    reject(
        &mut world,
        vec![
            bind(StateOverlayRef::Handle(owner), 0, "target"),
            attach(
                StateOverlayRef::Handle(owner),
                StateOverlayRef::Alias(0),
                1,
                ComponentOverlayMode::Bound,
                None,
            ),
        ],
        ErrorReason::MissingComponent,
    );
    let auto = declare(&mut world, "target", ComponentOverlayMode::Auto, Some(5.0));
    reject(
        &mut world,
        vec![attach(
            StateOverlayRef::Handle(auto.owner),
            StateOverlayRef::Handle(auto.binding),
            0,
            ComponentOverlayMode::Owned,
            None,
        )],
        ErrorReason::ComponentExists,
    );
    ok(&mut world, vec![insert(entity, 2.0)]);
    reject(
        &mut world,
        vec![attach(
            StateOverlayRef::Handle(auto.owner),
            StateOverlayRef::Handle(auto.binding),
            0,
            ComponentOverlayMode::Owned,
            None,
        )],
        ErrorReason::ComponentExists,
    );
    for command in [
        auto.update(f32::INFINITY),
        Command::UpdateComponentStateOverlay {
            owner: StateOverlayRef::Handle(auto.owner),
            overlay: StateOverlayRef::Handle(auto.overlay),
            fields: vec![],
            clear: vec![1],
        },
        Command::UpdateComponentStateOverlay {
            owner: StateOverlayRef::Handle(auto.owner),
            overlay: StateOverlayRef::Handle(auto.overlay),
            fields: vec![FieldWrite {
                offset: 0,
                value: FieldValue::Entity(EntityRef::Handle(entity)),
            }],
            clear: vec![0],
        },
    ] {
        assert!(run(&mut world, vec![command]).outcomes[0].result.is_err());
        assert_eq!(values(&world, entity), (Some(2.0), Some(5.0)));
    }
    reject(
        &mut world,
        vec![Command::AttachComponentStateOverlay {
            owner: StateOverlayRef::Handle(auto.owner),
            binding: StateOverlayRef::Handle(auto.binding),
            alias: 0,
            component: u16::MAX,
            mode: ComponentOverlayMode::Auto,
            fields: vec![],
        }],
        ErrorReason::UnknownComponent,
    );
}

#[test]
fn aliases_retain_original_entity_after_same_batch_release_and_namespaces_are_separate() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let report = ok(
        &mut world,
        vec![
            Command::Create {
                alias: 0,
                metadata: EntityMetadata::default(),
            },
            Command::CreateStateOverlayOwner {
                alias: 0,
            },
            Command::AttachEntityOverlayBinding {
                owner: StateOverlayRef::Alias(0),
                alias: 1,
                symbolic_id: "owned".into(),
                mode: EntityOverlayMode::Owned,
            },
            attach(
                StateOverlayRef::Alias(0),
                StateOverlayRef::Alias(1),
                2,
                ComponentOverlayMode::Auto,
                Some(1.0),
            ),
            Command::ReleaseStateOverlayOwner {
                owner: StateOverlayRef::Alias(0),
            },
        ],
    );
    let resources = &report.outcomes[0].state_overlays;
    assert_eq!(resources.len(), 3);
    assert_eq!(resources[0].kind, StateOverlayHandleKind::Owner);
    assert_eq!(resources[0].entity, None);
    assert_eq!(
        resources[1].kind,
        StateOverlayHandleKind::EntityOverlayBinding
    );
    assert_eq!(
        resources[2].kind,
        StateOverlayHandleKind::ComponentStateOverlay
    );
    let original = resources[1].entity.unwrap();
    assert_ne!(original.to_bits(), 0);
    assert_eq!(resources[2].entity, Some(original));
    assert!(world.inspect(original).is_none());
    assert_eq!(world.entities().len(), 1);
}

#[test]
fn state_overlay_storage_grows_while_ingress_budgets_reject_without_leaks() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(WorldLimits {
            max_batch_bytes: 65536,
            ..WorldLimits::default()
        })
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let entity = create(&mut world, "target", Some(1.0));
    let declaration = declare(&mut world, "target", ComponentOverlayMode::Bound, Some(5.0));
    ok(
        &mut world,
        vec![Command::CreateStateOverlayOwner {
            alias: 0,
        }],
    );
    assert_eq!(values(&world, entity), (Some(1.0), Some(5.0)));
    ok(&mut world, vec![declaration.release_owner()]);
    let replacement = declare(&mut world, "target", ComponentOverlayMode::Bound, Some(6.0));
    assert_ne!(declaration.owner, replacement.owner);
    assert_eq!(values(&world, entity), (Some(1.0), Some(6.0)));
    for command in [
        Command::AttachEntityOverlayBinding {
            owner: StateOverlayRef::Handle(replacement.owner),
            alias: 0,
            symbolic_id: String::with_capacity(100_000),
            mode: EntityOverlayMode::Owned,
        },
        Command::UpdateComponentStateOverlay {
            owner: StateOverlayRef::Handle(replacement.owner),
            overlay: StateOverlayRef::Handle(replacement.overlay),
            fields: Vec::with_capacity(100_000),
            clear: Vec::new(),
        },
        Command::UpdateComponentStateOverlay {
            owner: StateOverlayRef::Handle(replacement.owner),
            overlay: StateOverlayRef::Handle(replacement.overlay),
            fields: Vec::new(),
            clear: Vec::with_capacity(100_000),
        },
    ] {
        assert_eq!(
            world.enqueue(Batch {
                id: 0,
                operations: vec![command]
            }),
            Err(ErrorReason::Capacity)
        );
    }
}

#[test]
fn frame_diagnostic_budget_rejects_overflowing_batch_and_resets_next_frame() {
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(WorldLimits {
            max_operations: 1100,
            // The largest registered component determines Command's inline size.
            max_batch_bytes: 512 * 1024,
            max_staging_bytes: 48 * 1024 * 1024,
            ..WorldLimits::default()
        })
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let entity = create(&mut world, "target", Some(0.0));
    let wave = |value| {
        let mut operations = vec![
            Command::CreateStateOverlayOwner {
                alias: 0,
            },
            bind(StateOverlayRef::Alias(0), 1, "target"),
        ];
        for alias in 2..1026 {
            operations.push(attach(
                StateOverlayRef::Alias(0),
                StateOverlayRef::Alias(1),
                alias,
                ComponentOverlayMode::Bound,
                None,
            ));
        }
        operations.push(insert(entity, value));
        operations.push(Command::ReleaseStateOverlayOwner {
            owner: StateOverlayRef::Alias(0),
        });
        operations
    };
    for id in 1..=17 {
        world
            .enqueue(Batch {
                id,
                operations: wave(id as f32),
            })
            .unwrap();
    }
    let report = world.update_for_test(0.0).unwrap();
    assert_eq!(report.diagnostics.len(), MAX_STATE_OVERLAY_DIAGNOSTICS);
    assert!(
        report.outcomes[..16]
            .iter()
            .all(|outcome| outcome.result.is_ok())
    );
    let rejected = &report.outcomes[16];
    assert_eq!(
        rejected.result.as_ref().unwrap_err().reason,
        ErrorReason::Capacity
    );
    assert_eq!(rejected.result.as_ref().unwrap_err().operation, Some(1026));
    assert_eq!(rejected.state_overlays.len(), 1026);
    assert_eq!(values(&world, entity), (Some(17.0), Some(17.0)));
    let report = ok(&mut world, wave(17.0));
    assert_eq!(report.diagnostics.len(), 2048);
    assert_eq!(values(&world, entity), (Some(17.0), Some(17.0)));
    assert!(world.update_for_test(0.0).unwrap().diagnostics.is_empty());
}

mod constraints {
    use super::*;
    use ipp_core::components::LinearDriver;

    fn driver(target: EntityId, source: EntityId, scale: f32) -> Command {
        Command::InsertComponent {
            entity: EntityRef::Handle(target),
            component: ComponentValue::LINEAR_DRIVER,
            fields: vec![
                FieldWrite {
                    offset: std::mem::offset_of!(LinearDriver, source) as u32,
                    value: FieldValue::Entity(EntityRef::Handle(source)),
                },
                FieldWrite {
                    offset: std::mem::offset_of!(LinearDriver, scale) as u32,
                    value: FieldValue::F32(scale),
                },
            ],
        }
    }

    #[test]
    fn evaluation_uses_overlays_and_never_retains_previous_frame_outputs() {
        let mut world_host = ipp_core::HostRuntime::new();
        let world_id = world_host
            .create_world(ipp_core::WorldLimits::default())
            .unwrap();
        let mut world = world_host.world_mut(world_id).unwrap();
        let source = create(&mut world, "source", Some(2.0));
        let target = create(&mut world, "target", Some(1.0));
        let input = declare(&mut world, "source", ComponentOverlayMode::Bound, Some(3.0));
        let output = declare(&mut world, "target", ComponentOverlayMode::Bound, Some(9.0));
        ok(&mut world, vec![driver(target, source, 2.0)]);
        assert_eq!(values(&world, source), (Some(2.0), Some(3.0)));
        assert_eq!(values(&world, target), (Some(1.0), Some(6.0)));
        for _ in 0..3 {
            world.update_for_test(0.25).unwrap();
            assert_eq!(values(&world, source), (Some(2.0), Some(3.0)));
            assert_eq!(values(&world, target), (Some(1.0), Some(6.0)));
        }
        ok(&mut world, vec![input.update(4.0)]);
        assert_eq!(world.driver_bound(target), Some(true));
        assert_eq!(values(&world, target), (Some(1.0), Some(8.0)));
        ok(
            &mut world,
            vec![Command::RemoveComponent {
                entity: EntityRef::Handle(target),
                component: ComponentValue::LINEAR_DRIVER,
            }],
        );
        assert_eq!(values(&world, target), (Some(1.0), Some(9.0)));
        ok(&mut world, vec![output.clear()]);
        assert_eq!(values(&world, target), (Some(1.0), Some(1.0)));
    }

    #[test]
    #[cfg(debug_assertions)]
    fn revealing_invalid_hidden_base_reports_failure_without_restoring_overlay() {
        let mut world_host = ipp_core::HostRuntime::new();
        let world_id = world_host
            .create_world(ipp_core::WorldLimits::default())
            .unwrap();
        let mut world = world_host.world_mut(world_id).unwrap();
        let source = create(&mut world, "source", Some(2.0));
        let target = create(&mut world, "target", Some(1.0));
        let overlay = declare(&mut world, "source", ComponentOverlayMode::Bound, Some(3.0));
        ok(
            &mut world,
            vec![driver(target, source, 2.0), base_write(source, f32::MAX)],
        );
        assert_eq!(values(&world, source), (Some(f32::MAX), Some(3.0)));
        reject(&mut world, vec![overlay.clear()], ErrorReason::InvalidValue);
        assert_eq!(values(&world, source), (Some(f32::MAX), Some(f32::MAX)));
        assert!(values(&world, target).1.unwrap().is_infinite());
        assert_eq!(world.driver_bound(target), Some(true));
        ok(
            &mut world,
            vec![base_write(source, 4.0), overlay.release_owner()],
        );
        assert_eq!(values(&world, target), (Some(1.0), Some(8.0)));
    }

    #[test]
    fn fallback_drivers_bind_effective_incarnations_and_invalidate_before_transition() {
        let mut world_host = ipp_core::HostRuntime::new();
        let world_id = world_host
            .create_world(ipp_core::WorldLimits::default())
            .unwrap();
        let mut world = world_host.world_mut(world_id).unwrap();
        let source = create(&mut world, "source", None);
        let target = create(&mut world, "target", None);
        let source_overlay = declare(&mut world, "source", ComponentOverlayMode::Auto, Some(3.0));
        let target_overlay = declare(&mut world, "target", ComponentOverlayMode::Auto, Some(1.0));
        ok(&mut world, vec![driver(target, source, 2.0)]);
        assert_eq!(values(&world, target), (None, Some(6.0)));
        reject(
            &mut world,
            vec![insert(source, 4.0), target_overlay.update(f32::NAN)],
            ErrorReason::InvalidValue,
        );
        assert_eq!(values(&world, source), (Some(4.0), Some(3.0)));
        assert_eq!(world.driver_bound(target), Some(false));
        ok(&mut world, vec![insert(source, 4.0)]);
        assert_eq!(values(&world, source), (Some(4.0), Some(3.0)));
        assert_eq!(world.driver_bound(target), Some(false));
        assert_eq!(values(&world, target), (None, Some(1.0)));
        ok(&mut world, vec![driver(target, source, 2.0)]);
        assert_eq!(values(&world, target), (None, Some(6.0)));
        ok(&mut world, vec![remove(source)]);
        assert_eq!(world.driver_bound(target), Some(false));
        assert_eq!(values(&world, source), (None, Some(3.0)));
        ok(
            &mut world,
            vec![driver(target, source, 2.0), source_overlay.release_owner()],
        );
        assert_eq!(world.driver_bound(target), Some(false));
        assert_eq!(values(&world, source), (None, None));
    }

    #[test]
    fn linear_driver_overlays_rebind_reference_fields_and_restore_hidden_base() {
        let mut world_host = ipp_core::HostRuntime::new();
        let world_id = world_host
            .create_world(ipp_core::WorldLimits::default())
            .unwrap();
        let mut world = world_host.world_mut(world_id).unwrap();
        let first = create(&mut world, "first", Some(2.0));
        let second = create(&mut world, "second", Some(4.0));
        let target = create(&mut world, "target", Some(1.0));
        ok(&mut world, vec![driver(target, first, 2.0)]);

        let report = ok(
            &mut world,
            vec![
                Command::CreateStateOverlayOwner {
                    alias: 0,
                },
                bind(StateOverlayRef::Alias(0), 1, "target"),
                Command::AttachComponentStateOverlay {
                    owner: StateOverlayRef::Alias(0),
                    binding: StateOverlayRef::Alias(1),
                    alias: 2,
                    component: ComponentValue::LINEAR_DRIVER,
                    mode: ComponentOverlayMode::Bound,
                    fields: vec![FieldWrite {
                        offset: std::mem::offset_of!(LinearDriver, scale) as u32,
                        value: FieldValue::F32(3.0),
                    }],
                },
            ],
        );
        let owner = report.outcomes[0].state_overlays[0].id;
        let overlay = report.outcomes[0].state_overlays[2].id;
        assert_eq!(values(&world, target), (Some(1.0), Some(6.0)));

        ok(
            &mut world,
            vec![Command::UpdateComponentStateOverlay {
                owner: StateOverlayRef::Handle(owner),
                overlay: StateOverlayRef::Handle(overlay),
                fields: vec![FieldWrite {
                    offset: std::mem::offset_of!(LinearDriver, source) as u32,
                    value: FieldValue::Entity(EntityRef::Handle(second)),
                }],
                clear: Vec::new(),
            }],
        );
        assert_eq!(values(&world, target), (Some(1.0), Some(12.0)));

        ok(
            &mut world,
            vec![Command::UpdateComponentStateOverlay {
                owner: StateOverlayRef::Handle(owner),
                overlay: StateOverlayRef::Handle(overlay),
                fields: Vec::new(),
                clear: vec![std::mem::offset_of!(LinearDriver, source) as u32],
            }],
        );
        assert_eq!(values(&world, target), (Some(1.0), Some(6.0)));

        ok(
            &mut world,
            vec![Command::ReleaseComponentStateOverlay {
                owner: StateOverlayRef::Handle(owner),
                overlay: StateOverlayRef::Handle(overlay),
            }],
        );
        assert_eq!(values(&world, target), (Some(1.0), Some(4.0)));
    }
}

#[test]
fn overlay_attachment_and_update_resolve_prior_entity_aliases_before_retention() {
    use ipp_core::components::LinearDriver;
    let mut world_host = ipp_core::HostRuntime::new();
    let world_id = world_host
        .create_world(ipp_core::WorldLimits::default())
        .unwrap();
    let mut world = world_host.world_mut(world_id).unwrap();
    let source_field = |alias| FieldWrite {
        offset: std::mem::offset_of!(LinearDriver, source) as u32,
        value: FieldValue::Entity(EntityRef::Alias(alias)),
    };
    let create = |alias, name: &str| Command::Create {
        alias,
        metadata: EntityMetadata {
            symbolic_id: Some(name.into()),
            classes: vec![],
        },
    };
    let scalar = |alias, value| Command::InsertComponent {
        entity: EntityRef::Alias(alias),
        component: ComponentValue::SCALAR,
        fields: vec![write(value)],
    };
    let report = ok(
        &mut world,
        vec![
            create(1, "source"),
            scalar(1, 2.0),
            create(2, "target"),
            scalar(2, 0.0),
            Command::CreateStateOverlayOwner {
                alias: 10,
            },
            bind(StateOverlayRef::Alias(10), 11, "target"),
            Command::AttachComponentStateOverlay {
                owner: StateOverlayRef::Alias(10),
                binding: StateOverlayRef::Alias(11),
                alias: 12,
                component: ComponentValue::LINEAR_DRIVER,
                mode: ComponentOverlayMode::Auto,
                fields: vec![
                    source_field(1),
                    FieldWrite {
                        offset: std::mem::offset_of!(LinearDriver, scale) as u32,
                        value: FieldValue::F32(3.0),
                    },
                ],
            },
        ],
    );
    let target = world.lookup_id("target").unwrap();
    let owner = StateOverlayRef::Handle(report.outcomes[0].state_overlays[0].id);
    let overlay = StateOverlayRef::Handle(report.outcomes[0].state_overlays[2].id);
    let value = |world: &ipp_core::WorldContext<'_>| {
        world
            .inspect(target)
            .unwrap()
            .effective
            .into_iter()
            .find_map(|value| match value {
                ComponentValue::Scalar(value) => Some(value.value),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(value(&world), 6.0);
    ok(
        &mut world,
        vec![
            create(3, "replacement-source"),
            scalar(3, 5.0),
            Command::UpdateComponentStateOverlay {
                owner,
                overlay,
                fields: vec![source_field(3)],
                clear: vec![],
            },
        ],
    );
    assert_eq!(value(&world), 15.0);
    reject(
        &mut world,
        vec![Command::UpdateComponentStateOverlay {
            owner,
            overlay,
            fields: vec![source_field(99)],
            clear: vec![],
        }],
        ErrorReason::UnknownAlias,
    );
    reject(
        &mut world,
        vec![
            create(4, "discarded-source"),
            scalar(4, 8.0),
            Command::Delete {
                entity: EntityRef::Alias(4),
            },
            Command::UpdateComponentStateOverlay {
                owner,
                overlay,
                fields: vec![source_field(4)],
                clear: vec![],
            },
        ],
        ErrorReason::InvalidEntity,
    );
    assert_eq!(value(&world), 15.0);
    assert!(world.lookup_id("discarded-source").is_none());
}
