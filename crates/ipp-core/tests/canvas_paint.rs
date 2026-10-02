//! CanvasPaint on a plain CanvasBox through the real Host's Canvas publication:
//! the painted fill, the instance the publication carries beside it, property
//! writes and animation that leave every paint entry alone, and persistence.
//! No renderer is installed, so paint shaders never load here; the renderer's
//! tests and the maintained default-skin scenario cover drawing them.

mod support;

use ipp_core::components::schema::FieldValue as SchemaValue;
use ipp_core::components::{CanvasBox, CanvasPaint, CanvasStyle};
use ipp_core::services::asset_management::shader::{
    ShaderBackendSource, ShaderDefinition, ShaderParameterKind, ShaderRecipe,
};
use ipp_core::services::asset_management::{AssetUpload, AssetUploadIdentity};
use ipp_core::services::world_serialization::WorldLoadOptions;
use ipp_core::systems::animation::{
    ANIMATION_TYPE, AnimationClip, AnimationControllerDescription, AnimationDriverDescription,
    AnimationInterpolation, AnimationKeyframe, AnimationPlaybackControl, AnimationTrack,
    AnimationTrackTarget, AnimationValue,
};
use ipp_core::systems::canvas::{
    CanvasPaintEntry, CanvasPrimitive, CanvasPublication, CanvasShapeFill,
};
use ipp_core::{
    Batch, Command, ComponentValue, DynamicValue, EntityId, EntityRef, HostRuntime, OutputRef,
    WorldId, WorldLimits,
};
use std::collections::BTreeMap;
use std::sync::Arc;
use support::selection::CANVAS_CONTENT;
use support::{CanvasTestHost, WorldTestDriver};

fn apply(host: &mut HostRuntime, world: WorldId, operations: Vec<Command>) -> Vec<EntityId> {
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
        .result
        .unwrap()
        .into_iter()
        .map(|(_, entity)| entity)
        .collect()
}

fn frame(host: &mut HostRuntime, dt: f64) {
    let report = host.frame(dt).unwrap();
    assert!(report.worlds.values().all(Result::is_ok));
    assert!(report.publication_errors.is_empty());
}

fn publication(host: &HostRuntime, output: OutputRef) -> CanvasPublication {
    let publication = host.latest_publication(output.world().id()).unwrap();
    host.output(publication, output)
        .unwrap()
        .data::<CanvasPublication>()
        .unwrap()
        .clone()
}

fn fill(canvas: &CanvasPublication) -> CanvasShapeFill {
    let [entry] = &canvas.entries[..] else {
        panic!("one box paints: {:?}", canvas.entries);
    };
    let CanvasPaintEntry::Primitive {
        primitive: CanvasPrimitive::Box {
            fill,
            ..
        },
        ..
    } = entry.as_ref()
    else {
        panic!("a box paints: {entry:?}");
    };
    *fill
}

fn set_property(host: &mut HostRuntime, world: WorldId, entity: EntityId, name: &str, value: f32) {
    apply(
        host,
        world,
        vec![Command::SetDynamicProperty {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::CANVAS_PAINT,
            name: name.into(),
            value: DynamicValue::F32(value),
        }],
    );
}

/// A tinted 40 x 20 box with a scanline paint whose spacing is 4.
fn painted_box(host: &mut HostRuntime) -> (WorldId, OutputRef, EntityId) {
    let world = host
        .create_world(WorldLimits::default(), CANVAS_CONTENT)
        .unwrap();
    let output = host.canvas_output(host.world_ref(world).unwrap(), [100.0, 50.0], 100.0);
    let mut paint = CanvasPaint {
        source: "paint:///scanlines".into(),
        ..Default::default()
    };
    paint
        .properties
        .set("spacing", DynamicValue::F32(4.0))
        .unwrap();
    let entity = apply(
        host,
        world,
        vec![
            Command::Create {
                alias: 1,
                metadata: Default::default(),
                adopt: false,
            },
            Command::insert_value(
                EntityRef::Alias(1),
                ComponentValue::CanvasBox(CanvasBox {
                    width: 40.0,
                    height: 20.0,
                    ..Default::default()
                }),
            ),
            Command::insert_value(
                EntityRef::Alias(1),
                ComponentValue::CanvasStyle(CanvasStyle {
                    red: 0.5,
                    ..Default::default()
                }),
            ),
            Command::insert_value(EntityRef::Alias(1), ComponentValue::CanvasPaint(paint)),
        ],
    )[0];
    frame(host, 0.0);
    (world, output, entity)
}

#[test]
fn a_painted_box_names_its_paint_and_the_publication_carries_the_instance() {
    let mut host = HostRuntime::new();
    let (world, output, entity) = painted_box(&mut host);
    let canvas = publication(&host, output);

    // The plain box's white fill is the paint's colour; the tint stays on the style.
    let CanvasShapeFill::Paint {
        color,
        paint,
    } = fill(&canvas)
    else {
        panic!("the box fills with its paint");
    };
    assert_eq!(color, [1.0; 4]);
    assert_eq!(paint.entity, entity);
    assert_eq!(paint.component, ComponentValue::CANVAS_PAINT);
    let instance = canvas.paint(paint).expect("published paint instance");
    assert_eq!(&*instance.source, "paint:///scanlines");
    // No renderer loads paint shaders here, so the instance has none yet.
    assert_eq!(instance.shader, None);
    assert_eq!(
        &instance.properties[..],
        &[(Arc::from("spacing"), DynamicValue::F32(4.0))]
    );
    // The paint's definition is demanded like any referenced asset.
    assert!(
        host.asset_resources()
            .iter()
            .any(|resource| &*resource.source().uri == "paint:///scanlines")
    );

    // Removing the paint restores the plain fill and empties the instances.
    apply(
        &mut host,
        world,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::CANVAS_PAINT,
        }],
    );
    frame(&mut host, 0.0);
    let canvas = publication(&host, output);
    assert_eq!(fill(&canvas), CanvasShapeFill::Solid([1.0; 4]));
    assert!(canvas.paints.is_empty());
}

#[test]
fn property_writes_change_only_the_paint_instance() {
    let mut host = HostRuntime::new();
    let (world, output, entity) = painted_box(&mut host);
    let before = publication(&host, output);

    set_property(&mut host, world, entity, "spacing", 6.0);
    frame(&mut host, 0.0);
    let after = publication(&host, output);
    // The entries keep their storage and paint revision: no geometry changes.
    assert!(Arc::ptr_eq(&before.entries[0], &after.entries[0]));
    assert_eq!(after.paint_revision, before.paint_revision);
    assert_ne!(after.paints_revision, before.paints_revision);
    assert_eq!(
        after.paints[0].properties[0].1,
        DynamicValue::F32(6.0),
        "the instance carries the written value"
    );

    // An unchanged frame republishes nothing.
    frame(&mut host, 0.0);
    let settled = publication(&host, output);
    assert_eq!(settled.paints_revision, after.paints_revision);
    assert!(Arc::ptr_eq(&settled.paints, &after.paints));
}

#[test]
fn an_animated_property_reaches_the_paint_instance_and_persists() {
    let mut host = HostRuntime::new();
    let (world, output, entity) = painted_box(&mut host);
    let target = AnimationTrackTarget::DynamicProperty {
        component: ComponentValue::CANVAS_PAINT,
        name: "spacing".into(),
    };
    let key = |time: f64, value: f32, interpolation| AnimationKeyframe {
        time,
        value: AnimationValue::Field(SchemaValue::Dynamic(DynamicValue::F32(value))),
        interpolation,
    };
    let clip = AnimationClip::new(
        1.0,
        vec![AnimationTrack {
            target: target.clone(),
            keys: vec![
                key(0.0, 2.0, AnimationInterpolation::Linear),
                key(1.0, 10.0, AnimationInterpolation::Step),
            ],
        }],
    )
    .unwrap();
    let asset = 92;
    let mut context = host.world_mut(world).unwrap();
    context
        .enqueue_asset(AssetUpload {
            id: asset,
            key: AssetUploadIdentity {
                kind: ANIMATION_TYPE,
                asset,
                variant: 0,
            },
            bytes: clip.encode(),
        })
        .unwrap();
    assert!(context.await_upload_for_test().assets[0].result.is_ok());
    let controller = context
        .create_animation_controller(AnimationControllerDescription {
            drivers: vec![AnimationDriverDescription {
                source: Arc::<str>::from(format!("asset://10/{asset}")),
                variant: 0,
                track: 0,
                target: entity,
                property: target,
                entity_bindings: Vec::new(),
                weight: 1.0,
                additive: false,
                reference_time: 0.0,
                repeat: false,
            }],
            ..Default::default()
        })
        .unwrap();
    context
        .enqueue_playback(controller, AnimationPlaybackControl::Play)
        .unwrap();
    context
        .enqueue_playback(controller, AnimationPlaybackControl::Seek(0.5))
        .unwrap();
    drop(context);
    let before = publication(&host, output);
    for _ in 0..3 {
        frame(&mut host, 0.0);
    }
    let animated = publication(&host, output);
    let sampled = host
        .world_mut(world)
        .unwrap()
        .inspect(entity)
        .unwrap()
        .components
        .into_iter()
        .find_map(|value| match value {
            ComponentValue::CanvasPaint(paint) => paint.properties.get("spacing"),
            _ => None,
        })
        .unwrap();
    let DynamicValue::F32(spacing) = sampled else {
        panic!("a float property: {sampled:?}");
    };
    assert!(
        spacing > 2.0 && spacing < 10.0,
        "a sample inside the clip: {spacing}"
    );
    assert_eq!(
        animated.paints[0].properties[0].1, sampled,
        "the sampled value reaches the instance"
    );
    assert!(Arc::ptr_eq(&before.entries[0], &animated.entries[0]));

    // Later frames keep sampling the clip into the published instance.
    frame(&mut host, 0.25);
    let later = publication(&host, output);
    assert_ne!(
        later.paints[0].properties[0].1, sampled,
        "a later sample reaches the instance"
    );
    assert!(Arc::ptr_eq(&before.entries[0], &later.entries[0]));
    let sampled = later.paints[0].properties[0].1.clone();

    // A saved World keeps the paint and its stored properties.
    let bytes = host.save_world(world, 45, Default::default()).unwrap();
    let restored = host
        .load_world(
            &bytes,
            45,
            WorldLoadOptions {
                symbolic_id: Some("canvas-paint-restored".into()),
                ..Default::default()
            },
            WorldLimits::default(),
            Default::default(),
        )
        .unwrap()
        .root
        .id();
    let paint = host
        .world_mut(restored)
        .unwrap()
        .inspect(entity)
        .unwrap()
        .components
        .into_iter()
        .find_map(|value| match value {
            ComponentValue::CanvasPaint(paint) => Some(paint),
            _ => None,
        })
        .expect("restored paint");
    assert_eq!(&*paint.source, "paint:///scanlines");
    assert_eq!(paint.properties.get("spacing"), Some(sampled));
}

#[test]
fn a_paint_definition_round_trips_and_excludes_material_stages() {
    let paint = |body: &str| ShaderDefinition {
        parameters: BTreeMap::from([
            ("spacing".into(), ShaderParameterKind::F32),
            ("tint".into(), ShaderParameterKind::Vec4),
        ]),
        backends: BTreeMap::from([(
            "glsl-es-300".into(),
            ShaderBackendSource {
                paint: body.into(),
                ..Default::default()
            },
        )]),
        ..Default::default()
    };
    let definition = paint("return color * p_tint;");
    assert!(definition.is_paint());
    assert_eq!(
        definition.paint_body("glsl-es-300"),
        Some("return color * p_tint;")
    );
    let decoded = ShaderDefinition::decode(&definition.encode().unwrap()).unwrap();
    assert_eq!(decoded, definition);

    // A paint has no material stages, recipe features, streams or non-float inputs.
    let mut staged = paint("return color;");
    staged.backends.get_mut("glsl-es-300").unwrap().fragment =
        "vec4 materialFragment() { return vec4(1); }".into();
    let mut recipe = paint("return color;");
    recipe.recipe = ShaderRecipe {
        features: 1,
        ..Default::default()
    };
    let mut streams = paint("return color;");
    streams.required_attributes = 1;
    let mut texture = paint("return color;");
    texture
        .parameters
        .insert("image".into(), ShaderParameterKind::Texture2D);
    let mut integer = paint("return color;");
    integer
        .parameters
        .insert("count".into(), ShaderParameterKind::I32);
    for invalid in [staged, recipe, streams, texture, integer] {
        assert!(invalid.validate().is_err(), "{invalid:?}");
        assert!(invalid.encode().is_err());
    }

    // A material keeps its blank paint entry.
    let material = ShaderDefinition {
        backends: BTreeMap::from([(
            "glsl-es-300".into(),
            ShaderBackendSource {
                fragment: "vec4 materialFragment() { return vec4(1); }".into(),
                ..Default::default()
            },
        )]),
        ..Default::default()
    };
    assert!(!material.is_paint());
    assert_eq!(
        ShaderDefinition::decode(&material.encode().unwrap()).unwrap(),
        material
    );
}
