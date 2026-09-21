//! Stacked-overlay Surface ownership for GUI roots.
//!
//! Single content ownership spans every live restorable Surface
//! contribution, not just the effective and hidden-producer values: a lower
//! overlay's items masked by an upper empty overlay still block GuiRoot
//! admission, and withdrawing the upper layer beside a live GuiRoot fails
//! closed before anything installs. These tests mirror the maintained
//! native/worker ownership case with two separate overlay owners at core
//! level, verifying the invariant after both success and rejected
//! operations.
//!
//! Property-only overlays (dimensions etc.) carry no items field and
//! contribute no restorable content, so they never block admission or
//! withdrawal in either attachment order.

use super::*;
use crate::systems::surface::{Surface, SurfaceItemContent, SurfaceItemStyle};
use crate::{
    Batch, ComponentOverlayMode, ComponentValue, EntityMetadata, EntityOverlayMode, EntityRef,
    FieldWrite, HostRuntime, StateOverlayRef, WorldId, WorldLimits,
};
use crate::{
    commands::FieldValue as CommandFieldValue, components::schema::FieldValue as SchemaFieldValue,
};

struct Fixture {
    host: HostRuntime,
    world: WorldId,
    panel: EntityId,
}

fn world<'a>(fixture: &'a mut Fixture) -> crate::WorldContext<'a> {
    fixture.host.world_mut(fixture.world).unwrap()
}

/// Entity with an empty producer Surface and no GuiRoot yet.
fn setup() -> Fixture {
    let mut host = HostRuntime::new();
    let world = host.create_world(WorldLimits::default()).unwrap();
    let panel = {
        let mut context = host.world_mut(world).unwrap();
        context
            .enqueue(Batch {
                id: 1,
                operations: vec![
                    Command::Create {
                        alias: 1,
                        metadata: EntityMetadata::default(),
                    },
                    Command::InsertComponentValue {
                        entity: EntityRef::Alias(1),
                        value: ComponentValue::Surface({
                            let mut surface = Surface::default();
                            surface.width = 10.0;
                            surface.height = 10.0;
                            surface
                        }),
                    },
                ],
            })
            .unwrap();
        let report = context.step(0.0).unwrap();
        report.outcomes[0].result.as_ref().unwrap()[0].1
    };
    Fixture {
        host,
        world,
        panel,
    }
}

/// Items field value for one overlay declaration: populated with a single
/// label item, or the empty producer encoding that masks lower layers.
fn items_field(populated: bool) -> FieldWrite {
    let mut surface = Surface::default();
    if populated {
        surface
            .insert_item(
                0,
                SurfaceItemContent::Label("x".into()),
                SurfaceItemStyle::default(),
            )
            .unwrap();
    }
    let SchemaFieldValue::Bytes(bytes) = ComponentValue::Surface(surface)
        .field(Surface::items_field())
        .unwrap()
    else {
        panic!("surface items encode as bytes");
    };
    FieldWrite {
        offset: Surface::items_field(),
        value: CommandFieldValue::Bytes(bytes),
    }
}

/// Effective item count on one entity, or None without a Surface.
fn effective_items(fixture: &mut Fixture, entity: EntityId) -> Option<usize> {
    world(fixture).inspect(entity).and_then(|snapshot| {
        snapshot.effective.iter().find_map(|value| match value {
            ComponentValue::Surface(surface) => Some(surface.items().len()),
            _ => None,
        })
    })
}

/// Whether a live GuiRoot input exists on one entity.
fn has_gui_root(fixture: &mut Fixture, entity: EntityId) -> bool {
    world(fixture).inspect_gui(entity, None, 1, 1).is_ok()
}

/// Attach one Bound Surface overlay under a fresh owner. Returns the owner
/// and component overlay handles for later withdrawal.
fn attach_surface_overlay(
    fixture: &mut Fixture,
    symbol: &str,
    owner_alias: u32,
    populated: bool,
) -> (u64, u64) {
    attach_fields_overlay(fixture, symbol, owner_alias, vec![items_field(populated)])
}

/// Attach one Bound Surface overlay carrying `fields` under a fresh owner.
/// Returns the owner and component overlay handles for later withdrawal.
fn attach_fields_overlay(
    fixture: &mut Fixture,
    symbol: &str,
    owner_alias: u32,
    fields: Vec<FieldWrite>,
) -> (u64, u64) {
    let panel = fixture.panel;
    {
        let mut context = world(fixture);
        context
            .enqueue(Batch {
                id: context.tick() + 1,
                operations: vec![Command::SetMetadata {
                    entity: EntityRef::Handle(panel),
                    metadata: EntityMetadata {
                        symbolic_id: Some(symbol.into()),
                        classes: vec![],
                    },
                }],
            })
            .unwrap();
        context.step(0.0).unwrap();
    }
    let mut context = world(fixture);
    context
        .enqueue(Batch {
            id: context.tick() + 1,
            operations: vec![
                Command::CreateStateOverlayOwner {
                    alias: owner_alias,
                },
                Command::AttachEntityOverlayBinding {
                    owner: StateOverlayRef::Alias(owner_alias),
                    alias: owner_alias + 1,
                    symbolic_id: symbol.into(),
                    mode: EntityOverlayMode::Bound,
                },
                Command::AttachComponentStateOverlay {
                    owner: StateOverlayRef::Alias(owner_alias),
                    binding: StateOverlayRef::Alias(owner_alias + 1),
                    alias: owner_alias + 2,
                    component: ComponentValue::SURFACE,
                    mode: ComponentOverlayMode::Bound,
                    fields,
                },
            ],
        })
        .unwrap();
    let report = context.step(0.0).unwrap();
    assert!(report.outcomes[0].result.is_ok(), "{report:?}");
    let handles: Vec<(u32, u64)> = report.outcomes[0]
        .state_overlays
        .iter()
        .map(|alias| (alias.alias, alias.id))
        .collect();
    let owner = handles
        .iter()
        .find(|(alias, _)| *alias == owner_alias)
        .expect("owner handle")
        .1;
    let overlay = handles
        .iter()
        .find(|(alias, _)| *alias == owner_alias + 2)
        .expect("overlay handle")
        .1;
    (owner, overlay)
}

fn admit_gui_root(fixture: &mut Fixture) -> Result<(), crate::BatchError> {
    let panel = fixture.panel;
    let mut context = world(fixture);
    context
        .enqueue(Batch {
            id: context.tick() + 1,
            operations: vec![Command::InsertComponentValue {
                entity: EntityRef::Handle(panel),
                value: ComponentValue::GuiRoot(GuiRoot::default()),
            }],
        })
        .unwrap();
    let report = context.step(0.0).unwrap();
    report.outcomes[0].result.clone().map(|_| ())
}

#[test]
fn stacked_overlays_block_gui_root_admission() {
    let mut fixture = setup();
    let panel = fixture.panel;
    // Lower owner A carries raw items; upper owner B masks them with empty
    // items, so the effective Surface reads empty.
    attach_surface_overlay(&mut fixture, "stacked-a", 10, true);
    attach_surface_overlay(&mut fixture, "stacked-b", 20, false);
    assert_eq!(effective_items(&mut fixture, panel), Some(0));
    // Admission fails closed across all live restorable contributions.
    assert!(admit_gui_root(&mut fixture).is_err());
    // Rejected admission installs nothing: no GuiRoot, still-masked items.
    assert!(!has_gui_root(&mut fixture, panel));
    assert_eq!(effective_items(&mut fixture, panel), Some(0));
}

#[test]
fn masked_withdrawal_then_admission_keeps_single_owner() {
    let mut fixture = setup();
    let panel = fixture.panel;
    attach_surface_overlay(&mut fixture, "stacked-a", 10, true);
    let (owner_b, overlay_b) = attach_surface_overlay(&mut fixture, "stacked-b", 20, false);
    // Withdrawing the masking upper layer restores A's raw items while no
    // GuiRoot lives there; the ordered admission right after still refuses
    // to stack a GuiRoot over the restored content. Batches never roll
    // back, so the stopped admission keeps the completed withdrawal.
    let report = {
        let mut context = world(&mut fixture);
        context
            .enqueue(Batch {
                id: context.tick() + 1,
                operations: vec![
                    Command::ReleaseComponentStateOverlay {
                        owner: StateOverlayRef::Handle(owner_b),
                        overlay: StateOverlayRef::Handle(overlay_b),
                    },
                    Command::InsertComponentValue {
                        entity: EntityRef::Handle(panel),
                        value: ComponentValue::GuiRoot(GuiRoot::default()),
                    },
                ],
            })
            .unwrap();
        context.step(0.0).unwrap()
    };
    assert!(report.outcomes[0].result.is_err(), "{report:?}");
    assert!(!has_gui_root(&mut fixture, panel));
    assert_eq!(effective_items(&mut fixture, panel), Some(1));
}

#[test]
fn admission_then_withdrawal_preserves_single_owner() {
    let mut fixture = setup();
    let panel = fixture.panel;
    attach_surface_overlay(&mut fixture, "stacked-a", 10, true);
    let (owner_b, overlay_b) = attach_surface_overlay(&mut fixture, "stacked-b", 20, false);
    // Admission is refused first, so the batch stops before the withdrawal
    // could install the masked items beside a GuiRoot: the upper layer
    // still masks, and no GuiRoot exists.
    let report = {
        let mut context = world(&mut fixture);
        context
            .enqueue(Batch {
                id: context.tick() + 1,
                operations: vec![
                    Command::InsertComponentValue {
                        entity: EntityRef::Handle(panel),
                        value: ComponentValue::GuiRoot(GuiRoot::default()),
                    },
                    Command::ReleaseComponentStateOverlay {
                        owner: StateOverlayRef::Handle(owner_b),
                        overlay: StateOverlayRef::Handle(overlay_b),
                    },
                ],
            })
            .unwrap();
        context.step(0.0).unwrap()
    };
    assert!(report.outcomes[0].result.is_err(), "{report:?}");
    assert!(!has_gui_root(&mut fixture, panel));
    assert_eq!(effective_items(&mut fixture, panel), Some(0));
    // Withdrawing the upper layer on its own still succeeds and restores
    // the lower items, GuiRoot-free.
    let report = {
        let mut context = world(&mut fixture);
        context
            .enqueue(Batch {
                id: context.tick() + 1,
                operations: vec![Command::ReleaseComponentStateOverlay {
                    owner: StateOverlayRef::Handle(owner_b),
                    overlay: StateOverlayRef::Handle(overlay_b),
                }],
            })
            .unwrap();
        context.step(0.0).unwrap()
    };
    assert!(report.outcomes[0].result.is_ok(), "{report:?}");
    assert_eq!(effective_items(&mut fixture, panel), Some(1));
}

#[test]
fn clean_admission_and_overlay_release_succeed() {
    let mut fixture = setup();
    let panel = fixture.panel;
    // No Surface contributions anywhere: admission succeeds.
    assert!(admit_gui_root(&mut fixture).is_ok());
    assert!(has_gui_root(&mut fixture, panel));
    assert_eq!(effective_items(&mut fixture, panel), Some(0));
    // A Bound GuiRoot property overlay attaches, then releases cleanly;
    // the invariant holds after both operations.
    {
        let mut context = world(&mut fixture);
        context
            .enqueue(Batch {
                id: context.tick() + 1,
                operations: vec![Command::SetMetadata {
                    entity: EntityRef::Handle(panel),
                    metadata: EntityMetadata {
                        symbolic_id: Some("gui-clean".into()),
                        classes: vec![],
                    },
                }],
            })
            .unwrap();
        context.step(0.0).unwrap();
    }
    let owner = {
        let mut context = world(&mut fixture);
        context
            .enqueue(Batch {
                id: context.tick() + 1,
                operations: vec![
                    Command::CreateStateOverlayOwner {
                        alias: 30,
                    },
                    Command::AttachEntityOverlayBinding {
                        owner: StateOverlayRef::Alias(30),
                        alias: 31,
                        symbolic_id: "gui-clean".into(),
                        mode: EntityOverlayMode::Bound,
                    },
                    Command::AttachComponentStateOverlay {
                        owner: StateOverlayRef::Alias(30),
                        binding: StateOverlayRef::Alias(31),
                        alias: 32,
                        component: ComponentValue::GUI_ROOT,
                        mode: ComponentOverlayMode::Bound,
                        fields: vec![],
                    },
                ],
            })
            .unwrap();
        let report = context.step(0.0).unwrap();
        assert!(report.outcomes[0].result.is_ok(), "{report:?}");
        report.outcomes[0]
            .state_overlays
            .iter()
            .find(|alias| alias.alias == 30)
            .expect("owner handle")
            .id
    };
    {
        let mut context = world(&mut fixture);
        context
            .enqueue(Batch {
                id: context.tick() + 1,
                operations: vec![Command::ReleaseStateOverlayOwner {
                    owner: StateOverlayRef::Handle(owner),
                }],
            })
            .unwrap();
        let report = context.step(0.0).unwrap();
        assert!(report.outcomes[0].result.is_ok(), "{report:?}");
    }
    assert!(has_gui_root(&mut fixture, panel));
    assert_eq!(effective_items(&mut fixture, panel), Some(0));
}

#[test]
fn width_overlay_release_succeeds_beside_live_gui_root() {
    let mut fixture = setup();
    let panel = fixture.panel;
    assert!(admit_gui_root(&mut fixture).is_ok());
    // A non-items Surface contribution stays unrelated to content
    // ownership: it attaches beside the live GuiRoot and releases cleanly.
    let width = FieldWrite {
        offset: std::mem::offset_of!(Surface, width) as u32,
        value: CommandFieldValue::F32(5.0),
    };
    {
        let mut context = world(&mut fixture);
        context
            .enqueue(Batch {
                id: context.tick() + 1,
                operations: vec![Command::SetMetadata {
                    entity: EntityRef::Handle(panel),
                    metadata: EntityMetadata {
                        symbolic_id: Some("gui-wide".into()),
                        classes: vec![],
                    },
                }],
            })
            .unwrap();
        context.step(0.0).unwrap();
    }
    let owner = {
        let mut context = world(&mut fixture);
        context
            .enqueue(Batch {
                id: context.tick() + 1,
                operations: vec![
                    Command::CreateStateOverlayOwner {
                        alias: 40,
                    },
                    Command::AttachEntityOverlayBinding {
                        owner: StateOverlayRef::Alias(40),
                        alias: 41,
                        symbolic_id: "gui-wide".into(),
                        mode: EntityOverlayMode::Bound,
                    },
                    Command::AttachComponentStateOverlay {
                        owner: StateOverlayRef::Alias(40),
                        binding: StateOverlayRef::Alias(41),
                        alias: 42,
                        component: ComponentValue::SURFACE,
                        mode: ComponentOverlayMode::Bound,
                        fields: vec![width],
                    },
                ],
            })
            .unwrap();
        let report = context.step(0.0).unwrap();
        assert!(report.outcomes[0].result.is_ok(), "{report:?}");
        report.outcomes[0]
            .state_overlays
            .iter()
            .find(|alias| alias.alias == 40)
            .expect("owner handle")
            .id
    };
    {
        let mut context = world(&mut fixture);
        context
            .enqueue(Batch {
                id: context.tick() + 1,
                operations: vec![Command::ReleaseStateOverlayOwner {
                    owner: StateOverlayRef::Handle(owner),
                }],
            })
            .unwrap();
        let report = context.step(0.0).unwrap();
        assert!(report.outcomes[0].result.is_ok(), "{report:?}");
    }
    assert!(has_gui_root(&mut fixture, panel));
    assert_eq!(effective_items(&mut fixture, panel), Some(0));
}

/// Width/height-only property fields: dimensions without raw items, hence
/// no restorable content for the ownership guard to count.
fn dimension_fields(width: f32, height: f32) -> Vec<FieldWrite> {
    vec![
        FieldWrite {
            offset: std::mem::offset_of!(Surface, width) as u32,
            value: CommandFieldValue::F32(width),
        },
        FieldWrite {
            offset: std::mem::offset_of!(Surface, height) as u32,
            value: CommandFieldValue::F32(height),
        },
    ]
}

/// Effective Surface dimensions on one entity, or None without a Surface.
fn effective_dimensions(fixture: &mut Fixture, entity: EntityId) -> Option<(f32, f32)> {
    world(fixture).inspect(entity).and_then(|snapshot| {
        snapshot.effective.iter().find_map(|value| match value {
            ComponentValue::Surface(surface) => Some((surface.width, surface.height)),
            _ => None,
        })
    })
}

#[test]
fn dimension_only_overlay_then_root_admits_gui_root() {
    let mut fixture = setup();
    let panel = fixture.panel;
    // Fresh producer mount order: the binder attaches a Surface property
    // overlay before the producer inserts the GuiRoot.
    attach_fields_overlay(&mut fixture, "fresh-dims", 50, dimension_fields(5.0, 4.0));
    assert_eq!(effective_dimensions(&mut fixture, panel), Some((5.0, 4.0)));
    assert_eq!(effective_items(&mut fixture, panel), Some(0));
    assert!(admit_gui_root(&mut fixture).is_ok());
    assert!(has_gui_root(&mut fixture, panel));
    assert_eq!(effective_items(&mut fixture, panel), Some(0));
    assert_eq!(effective_dimensions(&mut fixture, panel), Some((5.0, 4.0)));
}

#[test]
fn dimension_overlay_withdrawal_beside_live_gui_root_succeeds() {
    let mut fixture = setup();
    let panel = fixture.panel;
    let (owner_a, overlay_a) =
        attach_fields_overlay(&mut fixture, "dims-a", 50, dimension_fields(5.0, 4.0));
    let (owner_b, _) =
        attach_fields_overlay(&mut fixture, "dims-b", 60, dimension_fields(7.0, 6.0));
    assert!(admit_gui_root(&mut fixture).is_ok());
    // Withdrawing one property overlay beside the live root succeeds; the
    // remaining property overlay keeps applying without content conflict.
    let report = {
        let mut context = world(&mut fixture);
        context
            .enqueue(Batch {
                id: context.tick() + 1,
                operations: vec![Command::ReleaseComponentStateOverlay {
                    owner: StateOverlayRef::Handle(owner_a),
                    overlay: StateOverlayRef::Handle(overlay_a),
                }],
            })
            .unwrap();
        context.step(0.0).unwrap()
    };
    assert!(report.outcomes[0].result.is_ok(), "{report:?}");
    assert!(has_gui_root(&mut fixture, panel));
    assert_eq!(effective_items(&mut fixture, panel), Some(0));
    assert_eq!(effective_dimensions(&mut fixture, panel), Some((7.0, 6.0)));
    // Releasing the last property owner succeeds and restores the producer
    // dimensions beside the live root.
    let report = {
        let mut context = world(&mut fixture);
        context
            .enqueue(Batch {
                id: context.tick() + 1,
                operations: vec![Command::ReleaseStateOverlayOwner {
                    owner: StateOverlayRef::Handle(owner_b),
                }],
            })
            .unwrap();
        context.step(0.0).unwrap()
    };
    assert!(report.outcomes[0].result.is_ok(), "{report:?}");
    assert!(has_gui_root(&mut fixture, panel));
    assert_eq!(effective_items(&mut fixture, panel), Some(0));
    assert_eq!(
        effective_dimensions(&mut fixture, panel),
        Some((10.0, 10.0))
    );
}

#[test]
fn raw_items_overlay_beside_live_gui_root_rejected() {
    let mut fixture = setup();
    let panel = fixture.panel;
    assert!(admit_gui_root(&mut fixture).is_ok());
    // Hidden raw-content additions to a GUI-owned Surface stay rejected,
    // even though no content would be masked or restored.
    {
        let mut context = world(&mut fixture);
        context
            .enqueue(Batch {
                id: context.tick() + 1,
                operations: vec![Command::SetMetadata {
                    entity: EntityRef::Handle(panel),
                    metadata: EntityMetadata {
                        symbolic_id: Some("gui-raw".into()),
                        classes: vec![],
                    },
                }],
            })
            .unwrap();
        context.step(0.0).unwrap();
    }
    let report = {
        let mut context = world(&mut fixture);
        context
            .enqueue(Batch {
                id: context.tick() + 1,
                operations: vec![
                    Command::CreateStateOverlayOwner {
                        alias: 70,
                    },
                    Command::AttachEntityOverlayBinding {
                        owner: StateOverlayRef::Alias(70),
                        alias: 71,
                        symbolic_id: "gui-raw".into(),
                        mode: EntityOverlayMode::Bound,
                    },
                    Command::AttachComponentStateOverlay {
                        owner: StateOverlayRef::Alias(70),
                        binding: StateOverlayRef::Alias(71),
                        alias: 72,
                        component: ComponentValue::SURFACE,
                        mode: ComponentOverlayMode::Bound,
                        fields: vec![items_field(true)],
                    },
                ],
            })
            .unwrap();
        context.step(0.0).unwrap()
    };
    assert!(report.outcomes[0].result.is_err(), "{report:?}");
    assert!(has_gui_root(&mut fixture, panel));
    assert_eq!(effective_items(&mut fixture, panel), Some(0));
}

#[test]
fn producer_items_refuse_gui_root_adoption() {
    let mut fixture = setup();
    let panel = fixture.panel;
    // Foreign raw content on the producer Surface refuses GuiRoot adoption:
    // the failed insert installs no root beside the items.
    {
        let mut surface = Surface::default();
        surface.width = 10.0;
        surface.height = 10.0;
        surface
            .insert_item(
                0,
                SurfaceItemContent::Label("foreign".into()),
                SurfaceItemStyle::default(),
            )
            .unwrap();
        let mut context = world(&mut fixture);
        context
            .enqueue(Batch {
                id: context.tick() + 1,
                operations: vec![Command::InsertComponentValue {
                    entity: EntityRef::Handle(panel),
                    value: ComponentValue::Surface(surface),
                }],
            })
            .unwrap();
        let report = context.step(0.0).unwrap();
        assert!(report.outcomes[0].result.is_ok(), "{report:?}");
    }
    assert_eq!(effective_items(&mut fixture, panel), Some(1));
    assert!(admit_gui_root(&mut fixture).is_err());
    assert!(!has_gui_root(&mut fixture, panel));
}
