//! Public dynamic property storage, mutation and sparse override invariants.
mod support;
use ipp_core::services::asset_management::{AssetSource, AssetTypeId};
use ipp_core::*;
use support::WorldTestDriver;

fn run(world: &mut WorldContext<'_>, operations: Vec<Command>) -> BatchOutcome {
    world
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();
    let result = world.update_for_test(0.0).unwrap().outcomes.remove(0);
    assert!(result.result.is_ok(), "{result:?}");
    result
}

#[test]
fn storage_reuses_bytes_without_reusing_property_identity() {
    let mut properties = DynamicProperties::default();
    let first = properties
        .set("tint", DynamicValue::Vec4([0.1, 0.2, 0.3, 1.0]))
        .unwrap();
    let original_offset = properties.descriptors()["tint"].offset;
    for i in 0..200 {
        properties
            .set(&format!("extra{i}"), DynamicValue::Mat4([i as f32; 16]))
            .unwrap();
    }
    assert_eq!(properties.key("tint"), Some(first));
    assert_eq!(properties.descriptors()["tint"].offset, original_offset);
    properties.remove("tint");
    let replacement = properties
        .set("tint", DynamicValue::Vec4([1.0; 4]))
        .unwrap();
    assert_ne!(first, replacement);
    assert_eq!(properties.descriptors()["tint"].offset, original_offset);
    assert!(
        properties
            .set_key(first, DynamicValue::Vec4([0.0; 4]))
            .is_err()
    );
    let next = properties.set("tint", DynamicValue::Bool(true)).unwrap();
    assert_ne!(replacement, next);
    assert!(properties.set("bad", DynamicValue::F32(f32::NAN)).is_err());
    assert_eq!(properties.get("bad"), None);
}

#[test]
fn descriptor_and_value_roundtrip_preserves_keys_and_owned_textures() {
    let mut original = ComponentValue::CustomMaterial(components::CustomMaterial::default());
    let props = original.dynamic_properties_mut().unwrap();
    props.set("roughness", DynamicValue::F32(0.5)).unwrap();
    props
        .set(
            "image",
            DynamicValue::Asset(AssetSource {
                kind: TEXTURE_TYPE,
                uri: "file://texture".into(),
                variant: 3,
            }),
        )
        .unwrap();
    props.remove("roughness");
    props
        .set("transform", DynamicValue::Mat3([1.0; 9]))
        .unwrap();
    let mut restored = ComponentValue::CustomMaterial(components::CustomMaterial::default());
    for (key, value) in original.fields() {
        restored.set_field(key, value).unwrap();
    }
    let a = original.dynamic_properties().unwrap();
    let b = restored.dynamic_properties().unwrap();
    for name in ["image", "transform"] {
        assert_eq!(a.key(name), b.key(name));
        assert_eq!(a.get(name), b.get(name));
    }
}

#[test]
fn named_base_writes_keep_sparse_overrides_and_latest_producer_value() {
    let mut host = HostRuntime::new();
    let id = host.create_world(WorldLimits::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let outcome = run(
        &mut world,
        vec![
            Command::Create {
                alias: 1,
                metadata: EntityMetadata {
                    symbolic_id: Some("material".into()),
                    classes: vec![],
                },
            },
            Command::InsertComponent {
                entity: EntityRef::Alias(1),
                component: ComponentValue::CUSTOM_MATERIAL,
                fields: vec![],
            },
            Command::SetDynamicProperty {
                entity: EntityRef::Alias(1),
                component: ComponentValue::CUSTOM_MATERIAL,
                name: "tint".into(),
                value: DynamicValue::Vec3([0.2; 3]),
            },
        ],
    );
    let entity = outcome.result.unwrap()[0].1;
    let property = |world: &WorldContext<'_>| {
        world
            .inspect(entity)
            .unwrap()
            .effective
            .into_iter()
            .find_map(|v| match v {
                ComponentValue::CustomMaterial(material) => Some(material.properties.get("tint")),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(property(&world), Some(DynamicValue::Vec3([0.2; 3])));
    run(
        &mut world,
        vec![Command::SetDynamicProperty {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::CUSTOM_MATERIAL,
            name: "tint".into(),
            value: DynamicValue::Vec3([0.7; 3]),
        }],
    );
    assert_eq!(property(&world), Some(DynamicValue::Vec3([0.7; 3])));
    run(
        &mut world,
        vec![Command::RemoveDynamicProperty {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::CUSTOM_MATERIAL,
            name: "tint".into(),
        }],
    );
    assert_eq!(property(&world), None);
}

fn material(world: &WorldContext<'_>, entity: EntityId, base: bool) -> DynamicProperties {
    let snapshot = world.inspect(entity).unwrap();
    let values = if base {
        snapshot.base
    } else {
        snapshot.effective
    };
    values
        .into_iter()
        .find_map(|value| match value {
            ComponentValue::CustomMaterial(value) => Some(value.properties),
            _ => None,
        })
        .unwrap()
}

#[test]
fn overlays_restore_hidden_values_and_do_not_rebind_retyped_properties() {
    let mesh = |source: &str| {
        DynamicValue::Asset(AssetSource {
            kind: MESH_TYPE,
            uri: source.into(),
            variant: 3,
        })
    };
    for (original, overridden, updated) in [
        (
            DynamicValue::Vec3([0.2; 3]),
            DynamicValue::Vec3([0.9; 3]),
            DynamicValue::Vec3([0.6; 3]),
        ),
        (
            mesh("file:///base.mesh"),
            mesh("file:///override.mesh"),
            mesh("file:///updated.mesh"),
        ),
    ] {
        let mut host = HostRuntime::new();
        let id = host.create_world(WorldLimits::default()).unwrap();
        let mut world = host.world_mut(id).unwrap();
        let entity = run(
            &mut world,
            vec![
                Command::Create {
                    alias: 1,
                    metadata: EntityMetadata {
                        symbolic_id: Some("material".into()),
                        classes: vec![],
                    },
                },
                Command::InsertComponent {
                    entity: EntityRef::Alias(1),
                    component: ComponentValue::CUSTOM_MATERIAL,
                    fields: vec![],
                },
                Command::SetDynamicProperty {
                    entity: EntityRef::Alias(1),
                    component: ComponentValue::CUSTOM_MATERIAL,
                    name: "tint".into(),
                    value: original.clone(),
                },
            ],
        )
        .result
        .unwrap()[0]
            .1;
        let aliases = run(
            &mut world,
            vec![
                Command::CreateStateOverlayOwner {
                    alias: 1,
                },
                Command::AttachEntityOverlayBinding {
                    owner: StateOverlayRef::Alias(1),
                    alias: 2,
                    symbolic_id: "material".into(),
                    mode: EntityOverlayMode::Bound,
                },
                Command::AttachComponentStateOverlay {
                    owner: StateOverlayRef::Alias(1),
                    binding: StateOverlayRef::Alias(2),
                    alias: 3,
                    component: ComponentValue::CUSTOM_MATERIAL,
                    mode: ComponentOverlayMode::Bound,
                    fields: vec![],
                },
                Command::UpdateDynamicComponentStateOverlay {
                    owner: StateOverlayRef::Alias(1),
                    overlay: StateOverlayRef::Alias(3),
                    properties: vec![("tint".into(), overridden.clone())],
                    clear: vec![],
                },
            ],
        )
        .state_overlays;
        let owner = StateOverlayRef::Handle(aliases[0].id);
        let overlay = StateOverlayRef::Handle(aliases[2].id);
        assert_eq!(
            material(&world, entity, false).get("tint"),
            Some(overridden.clone())
        );
        run(
            &mut world,
            vec![Command::SetDynamicProperty {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::CUSTOM_MATERIAL,
                name: "tint".into(),
                value: updated.clone(),
            }],
        );
        assert_eq!(
            material(&world, entity, true).get("tint"),
            Some(updated.clone())
        );
        assert_eq!(
            material(&world, entity, false).get("tint"),
            Some(overridden.clone())
        );
        run(
            &mut world,
            vec![Command::SetDynamicProperty {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::CUSTOM_MATERIAL,
                name: "tint".into(),
                value: DynamicValue::Bool(true),
            }],
        );
        assert_eq!(
            material(&world, entity, false).get("tint"),
            Some(DynamicValue::Bool(true))
        );
        run(
            &mut world,
            vec![Command::UpdateDynamicComponentStateOverlay {
                owner,
                overlay,
                properties: vec![],
                clear: vec!["tint".into()],
            }],
        );
        run(
            &mut world,
            vec![Command::ReleaseStateOverlayOwner {
                owner,
            }],
        );
        assert_eq!(
            material(&world, entity, false).get("tint"),
            Some(DynamicValue::Bool(true))
        );
    }
}

#[test]
fn owned_overlay_defines_properties_in_the_attachment_batch() {
    let mut host = HostRuntime::new();
    let id = host.create_world(WorldLimits::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let outcome = run(
        &mut world,
        vec![
            Command::CreateStateOverlayOwner {
                alias: 1,
            },
            Command::AttachEntityOverlayBinding {
                owner: StateOverlayRef::Alias(1),
                alias: 2,
                symbolic_id: "owned".into(),
                mode: EntityOverlayMode::Owned,
            },
            Command::AttachComponentStateOverlay {
                owner: StateOverlayRef::Alias(1),
                binding: StateOverlayRef::Alias(2),
                alias: 3,
                component: ComponentValue::CUSTOM_MATERIAL,
                mode: ComponentOverlayMode::Owned,
                fields: vec![],
            },
            Command::UpdateDynamicComponentStateOverlay {
                owner: StateOverlayRef::Alias(1),
                overlay: StateOverlayRef::Alias(3),
                properties: vec![("amount".into(), DynamicValue::F32(0.7))],
                clear: vec![],
            },
        ],
    );
    let entity = outcome.state_overlays[1].entity.unwrap();
    assert_eq!(
        material(&world, entity, false).get("amount"),
        Some(DynamicValue::F32(0.7))
    );
    let owner = StateOverlayRef::Handle(outcome.state_overlays[0].id);
    let overlay = StateOverlayRef::Handle(outcome.state_overlays[2].id);
    run(
        &mut world,
        vec![Command::SetDynamicProperty {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::CUSTOM_MATERIAL,
            name: "amount".into(),
            value: DynamicValue::F32(0.3),
        }],
    );
    assert_eq!(
        material(&world, entity, false).get("amount"),
        Some(DynamicValue::F32(0.7))
    );
    run(
        &mut world,
        vec![Command::UpdateDynamicComponentStateOverlay {
            owner,
            overlay,
            properties: vec![],
            clear: vec!["amount".into()],
        }],
    );
    assert_eq!(
        material(&world, entity, false).get("amount"),
        Some(DynamicValue::F32(0.3)),
        "omitting an owned override reveals an explicitly authored producer value"
    );
}

#[test]
fn named_animation_interpolates_and_property_loss_preserves_sibling_binding() {
    use services::asset_management::{AssetUpload, AssetUploadIdentity};
    use systems::animation::*;
    let mut host = HostRuntime::new();
    let id = host.create_world(WorldLimits::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let entity = run(
        &mut world,
        vec![
            Command::Create {
                alias: 1,
                metadata: Default::default(),
            },
            Command::InsertComponent {
                entity: EntityRef::Alias(1),
                component: ComponentValue::CUSTOM_MATERIAL,
                fields: vec![],
            },
            Command::SetDynamicProperty {
                entity: EntityRef::Alias(1),
                component: ComponentValue::CUSTOM_MATERIAL,
                name: "a".into(),
                value: DynamicValue::Vec2([7.0; 2]),
            },
            Command::SetDynamicProperty {
                entity: EntityRef::Alias(1),
                component: ComponentValue::CUSTOM_MATERIAL,
                name: "b".into(),
                value: DynamicValue::Vec2([8.0; 2]),
            },
        ],
    )
    .result
    .unwrap()[0]
        .1;
    let tracks: Vec<_> = ["a", "b"]
        .into_iter()
        .map(|name| AnimationTrack {
            target: AnimationTrackTarget::DynamicProperty {
                component: ComponentValue::CUSTOM_MATERIAL,
                name: name.into(),
            },
            keys: vec![
                AnimationKeyframe {
                    time: 0.0,
                    value: AnimationValue::Field(components::schema::FieldValue::Dynamic(
                        DynamicValue::Vec2([0.0; 2]),
                    )),
                    interpolation: AnimationInterpolation::Linear,
                },
                AnimationKeyframe {
                    time: 2.0,
                    value: AnimationValue::Field(components::schema::FieldValue::Dynamic(
                        DynamicValue::Vec2([10.0; 2]),
                    )),
                    interpolation: AnimationInterpolation::Step,
                },
            ],
        })
        .collect();
    let clip = AnimationClip::new(2.0, tracks).unwrap();
    assert_eq!(&clip.encode()[4..8], &3u32.to_le_bytes());
    let clip = AnimationClip::decode(&clip.encode()).unwrap();
    world
        .enqueue_asset(AssetUpload {
            id: 1,
            key: AssetUploadIdentity {
                kind: ANIMATION_TYPE,
                asset: 1,
                variant: 0,
            },
            bytes: clip.encode(),
        })
        .unwrap();
    assert!(world.await_upload_for_test().assets[0].result.is_ok());
    let controller = world
        .create_animation_controller(AnimationControllerDescription {
            drivers: clip
                .tracks()
                .iter()
                .enumerate()
                .map(|(i, track)| AnimationDriverDescription {
                    source: "asset://10/1".into(),
                    variant: 0,
                    track: i as u32,
                    target: entity,
                    property: track.target().clone(),
                    weight: 1.0,
                    additive: false,
                    reference_time: 0.0,
                    repeat: false,
                })
                .collect(),
            ..Default::default()
        })
        .unwrap();
    world
        .enqueue_playback(controller, AnimationPlaybackControl::Play)
        .unwrap();
    world
        .enqueue_playback(controller, AnimationPlaybackControl::Seek(1.0))
        .unwrap();
    world
        .enqueue_playback(controller, AnimationPlaybackControl::Pause)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert_eq!(
        material(&world, entity, false).get("a"),
        Some(DynamicValue::Vec2([5.0; 2]))
    );
    // Unrelated additions relocate the component-owned byte buffer while keeping
    // the bound component and selected property identities live.
    run(
        &mut world,
        (0..12)
            .map(|index| Command::SetDynamicProperty {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::CUSTOM_MATERIAL,
                name: format!("extra_{index}"),
                value: DynamicValue::Mat4([index as f32; 16]),
            })
            .collect(),
    );
    assert_eq!(
        material(&world, entity, false).get("a"),
        Some(DynamicValue::Vec2([5.0; 2]))
    );
    assert_eq!(
        material(&world, entity, false).get("b"),
        Some(DynamicValue::Vec2([5.0; 2]))
    );
    run(
        &mut world,
        vec![Command::RemoveDynamicProperty {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::CUSTOM_MATERIAL,
            name: "a".into(),
        }],
    );
    assert_eq!(
        material(&world, entity, false).get("b"),
        Some(DynamicValue::Vec2([5.0; 2]))
    );
    world
        .enqueue_playback(controller, AnimationPlaybackControl::Seek(1.5))
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert_eq!(
        material(&world, entity, false).get("b"),
        Some(DynamicValue::Vec2([7.5; 2]))
    );
    world
        .enqueue_playback(controller, AnimationPlaybackControl::Stop)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert_eq!(
        material(&world, entity, false).get("b"),
        Some(DynamicValue::Vec2([8.0; 2]))
    );
}

#[test]
fn world_save_load_preserves_dynamic_descriptors_without_shader_availability() {
    use services::world_serialization::{WorldLoadOptions, WorldPersistenceLimits};
    let mut host = HostRuntime::new();
    let id = host.create_world(WorldLimits::default()).unwrap();
    {
        let mut world = host.world_mut(id).unwrap();
        let mut material = components::CustomMaterial {
            source: "file:///unavailable.shader".into(),
            ..Default::default()
        };
        material
            .properties
            .set("basis", DynamicValue::Mat4([0.25; 16]))
            .unwrap();
        material
            .properties
            .set(
                "texture",
                DynamicValue::Asset(AssetSource {
                    kind: TEXTURE_TYPE,
                    uri: "file:///unavailable.png".into(),
                    variant: 7,
                }),
            )
            .unwrap();
        material
            .properties
            .set(
                "geometry",
                DynamicValue::Asset(AssetSource {
                    kind: MESH_TYPE,
                    uri: "file:///unavailable.mesh".into(),
                    variant: 4,
                }),
            )
            .unwrap();
        run(
            &mut world,
            vec![
                Command::Create {
                    alias: 1,
                    metadata: Default::default(),
                },
                Command::InsertComponentValue {
                    entity: EntityRef::Alias(1),
                    value: ComponentValue::CustomMaterial(material),
                },
            ],
        );
    }
    {
        use systems::animation::*;
        let mut world = host.world_mut(id).unwrap();
        let entity = world.entities()[0].id;
        let controller = world
            .create_animation_controller(AnimationControllerDescription {
                drivers: vec![AnimationDriverDescription {
                    source: "file:///unavailable.animation".into(),
                    variant: 0,
                    track: 0,
                    target: entity,
                    property: AnimationTrackTarget::DynamicProperty {
                        component: ComponentValue::CUSTOM_MATERIAL,
                        name: "basis".into(),
                    },
                    weight: 1.0,
                    additive: false,
                    reference_time: 0.0,
                    repeat: false,
                }],
                ..Default::default()
            })
            .unwrap();
        world
            .control_animation_controller(controller, AnimationPlaybackControl::Play)
            .unwrap();
        world
            .control_animation_controller(controller, AnimationPlaybackControl::Pause)
            .unwrap();
        world
            .control_animation_controller(controller, AnimationPlaybackControl::Seek(0.75))
            .unwrap();
    }
    let saved = host
        .save_world(id, 42, WorldPersistenceLimits::default())
        .unwrap();
    let loaded = host
        .load_world(
            &saved,
            42,
            WorldLoadOptions {
                symbolic_id: Some("restored".into()),
                ..Default::default()
            },
            WorldLimits::default(),
            WorldPersistenceLimits::default(),
        )
        .unwrap();
    let world = host.world_mut(loaded).unwrap();
    let entity = world.entities()[0].id;
    let animation = world.animation_persistent_state();
    assert_eq!(
        animation.controllers[0].state,
        systems::animation::AnimationPlaybackStatus::Paused
    );
    assert_eq!(animation.controllers[0].time, 0.75);
    assert_eq!(
        animation.controllers[0].description.drivers[0].property,
        systems::animation::AnimationTrackTarget::DynamicProperty {
            component: ComponentValue::CUSTOM_MATERIAL,
            name: "basis".into()
        }
    );
    assert_eq!(
        material(&world, entity, true).get("basis"),
        Some(DynamicValue::Mat4([0.25; 16]))
    );
    assert_eq!(
        material(&world, entity, true).get("texture"),
        Some(DynamicValue::Asset(AssetSource {
            kind: TEXTURE_TYPE,
            uri: "file:///unavailable.png".into(),
            variant: 7
        }))
    );
    assert_eq!(
        material(&world, entity, true).get("geometry"),
        Some(DynamicValue::Asset(AssetSource {
            kind: MESH_TYPE,
            uri: "file:///unavailable.mesh".into(),
            variant: 4,
        }))
    );
}

#[test]
fn auto_overlay_does_not_reuse_dynamic_identities_after_component_replacement() {
    let mut host = HostRuntime::new();
    let id = host.create_world(WorldLimits::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    let entity = run(
        &mut world,
        vec![
            Command::Create {
                alias: 1,
                metadata: EntityMetadata {
                    symbolic_id: Some("material".into()),
                    classes: vec![],
                },
            },
            Command::InsertComponent {
                entity: EntityRef::Alias(1),
                component: ComponentValue::CUSTOM_MATERIAL,
                fields: vec![],
            },
            Command::SetDynamicProperty {
                entity: EntityRef::Alias(1),
                component: ComponentValue::CUSTOM_MATERIAL,
                name: "tint".into(),
                value: DynamicValue::Vec3([0.2; 3]),
            },
        ],
    )
    .result
    .unwrap()[0]
        .1;
    let aliases = run(
        &mut world,
        vec![
            Command::CreateStateOverlayOwner {
                alias: 1,
            },
            Command::AttachEntityOverlayBinding {
                owner: StateOverlayRef::Alias(1),
                alias: 2,
                symbolic_id: "material".into(),
                mode: EntityOverlayMode::Bound,
            },
            Command::AttachComponentStateOverlay {
                owner: StateOverlayRef::Alias(1),
                binding: StateOverlayRef::Alias(2),
                alias: 3,
                component: ComponentValue::CUSTOM_MATERIAL,
                mode: ComponentOverlayMode::Auto,
                fields: vec![],
            },
            Command::UpdateDynamicComponentStateOverlay {
                owner: StateOverlayRef::Alias(1),
                overlay: StateOverlayRef::Alias(3),
                properties: vec![("tint".into(), DynamicValue::Vec3([0.9; 3]))],
                clear: vec![],
            },
        ],
    )
    .state_overlays;
    let owner = StateOverlayRef::Handle(aliases[0].id);
    let overlay = StateOverlayRef::Handle(aliases[2].id);
    assert_eq!(
        material(&world, entity, false).get("tint"),
        Some(DynamicValue::Vec3([0.9; 3]))
    );
    let mut replacement = components::CustomMaterial::default();
    replacement
        .properties
        .set("other", DynamicValue::Vec3([0.1; 3]))
        .unwrap();
    run(
        &mut world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(entity),
            value: ComponentValue::CustomMaterial(replacement),
        }],
    );
    assert_eq!(
        material(&world, entity, false).get("other"),
        Some(DynamicValue::Vec3([0.1; 3]))
    );
    run(
        &mut world,
        vec![Command::UpdateDynamicComponentStateOverlay {
            owner,
            overlay,
            properties: vec![("other".into(), DynamicValue::Vec3([0.4; 3]))],
            clear: vec![],
        }],
    );
    assert_eq!(
        material(&world, entity, false).get("other"),
        Some(DynamicValue::Vec3([0.4; 3]))
    );
}

#[test]
fn dynamic_curve_types_roundtrip_and_sample_without_scalarizing_matrices() {
    use systems::animation::*;
    for (a, b, midpoint) in [
        (
            DynamicValue::Mat3([0.0; 9]),
            DynamicValue::Mat3([2.0; 9]),
            DynamicValue::Mat3([1.0; 9]),
        ),
        (
            DynamicValue::I32(-3),
            DynamicValue::I32(2),
            DynamicValue::I32(-1),
        ),
        (
            DynamicValue::U32(0),
            DynamicValue::U32(3),
            DynamicValue::U32(2),
        ),
        (
            DynamicValue::Bool(false),
            DynamicValue::Bool(true),
            DynamicValue::Bool(false),
        ),
        (
            DynamicValue::Asset(AssetSource {
                kind: MESH_TYPE,
                uri: "file:///a.mesh".into(),
                variant: 1,
            }),
            DynamicValue::Asset(AssetSource {
                kind: TEXTURE_TYPE,
                uri: "file:///b.png".into(),
                variant: 2,
            }),
            DynamicValue::Asset(AssetSource {
                kind: MESH_TYPE,
                uri: "file:///a.mesh".into(),
                variant: 1,
            }),
        ),
    ] {
        let discrete = matches!(a, DynamicValue::Bool(_) | DynamicValue::Asset(_));
        let value = |v| AnimationValue::Field(components::schema::FieldValue::Dynamic(v));
        let clip = AnimationClip::new(
            1.0,
            vec![AnimationTrack {
                target: AnimationTrackTarget::DynamicProperty {
                    component: ComponentValue::CUSTOM_MATERIAL,
                    name: "parameter".into(),
                },
                keys: vec![
                    AnimationKeyframe {
                        time: 0.0,
                        value: value(a),
                        interpolation: if discrete {
                            AnimationInterpolation::Step
                        } else {
                            AnimationInterpolation::Linear
                        },
                    },
                    AnimationKeyframe {
                        time: 1.0,
                        value: value(b.clone()),
                        interpolation: AnimationInterpolation::Step,
                    },
                ],
            }],
        )
        .unwrap();
        let decoded = AnimationClip::decode(&clip.encode()).unwrap();
        assert_eq!(decoded.sample(0, 0.5), value(midpoint));
        assert_eq!(decoded.sample(0, 1.0), value(b));
    }
}

#[test]
fn asset_properties_preserve_type_and_variant_without_shader_interpretation() {
    let mut properties = DynamicProperties::default();
    let mesh = DynamicValue::Asset(AssetSource {
        kind: MESH_TYPE,
        uri: "file:///mesh".into(),
        variant: 7,
    });
    let key = properties.set("input", mesh.clone()).unwrap();
    assert_eq!(
        properties.descriptors()["input"].kind,
        DynamicPropertyKind::Asset
    );
    assert!(properties.buffer().is_empty());
    assert_eq!(DynamicValue::decode(&mesh.encode()).unwrap(), mesh);
    assert_eq!(&mesh.encode()[..7], &[12, 1, 0, 7, 0, 0, 0]);

    // Asset payload type belongs to the reference value; consumers own compatibility.
    let other = DynamicValue::Asset(AssetSource {
        kind: AssetTypeId(60000),
        uri: "file:///extension".into(),
        variant: 42,
    });
    assert_eq!(properties.set("input", other.clone()).unwrap(), key);
    assert_eq!(DynamicValue::decode(&other.encode()).unwrap(), other);
    let fields = properties.fields();
    let mut restored = DynamicProperties::default();
    for (field, value) in fields {
        restored.set_field(field, value).unwrap();
    }
    assert_eq!(restored.key("input"), Some(key));
    assert_eq!(restored.get("input"), Some(other));
    assert!(DynamicValue::decode(&[12, 1, 0, 7, 0, 0]).is_err());
    assert!(DynamicValue::decode(&[11, 7, 0, 0, 0]).is_err());
    assert_eq!(properties.remove("input"), Some(key));
    assert!(properties.get_key(key).is_none());
    assert_ne!(properties.set("input", mesh).unwrap(), key);
}
