//! Persistent controller descriptions; targets are durable entity IDs in the file.

use crate::services::world_serialization::binary::{WorldBinaryReader, WorldBinaryWriter};
use crate::{EntityId, systems::animation::*};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn encode(
    writer: &mut WorldBinaryWriter,
    state: &AnimationPersistentState,
) -> Result<(), String> {
    writer.u64(state.next_id)?;
    writer.count(state.controllers.len())?;
    for controller in &state.controllers {
        encode_controller(writer, controller)?;
    }
    writer.count(state.transitions.len())?;
    for transition in &state.transitions {
        writer.u64(transition.id.to_bits())?;
        writer.u8(match transition.start_time {
            AnimationTransitionStartTime::Restart => 0,
            AnimationTransitionStartTime::Preserve => 1,
            AnimationTransitionStartTime::MatchPhase => 2,
            AnimationTransitionStartTime::Seek(_) => 3,
        })?;
        if let AnimationTransitionStartTime::Seek(time) = transition.start_time {
            writer.u64(time.to_bits())?;
        }
        match &transition.source {
            AnimationPersistentTransitionSource::Live(source) => {
                writer.u8(0)?;
                encode_controller(writer, source)?;
            }
            AnimationPersistentTransitionSource::Frozen {
                values,
                bindings,
                reference_time,
                reference_duration,
            } => {
                writer.u8(1)?;
                writer.u64(reference_time.to_bits())?;
                writer.u64(reference_duration.to_bits())?;
                encode_controller(writer, bindings)?;
                writer.count(values.len())?;
                for value in values {
                    writer.u64(value.target.to_bits())?;
                    encode_target(writer, &value.property)?;
                    encode_value(writer, &value.value)?;
                    encode_value(writer, &value.baseline)?;
                }
            }
        }
    }
    writer.count(state.directional_starts.len())?;
    for id in &state.directional_starts {
        writer.u64(id.to_bits())?;
    }
    Ok(())
}

pub(crate) fn decode_legacy(
    reader: &mut WorldBinaryReader<'_>,
) -> Result<AnimationPersistentState, String> {
    decode(reader, 1)
}

pub(super) fn decode(
    reader: &mut WorldBinaryReader<'_>,
    version: u32,
) -> Result<AnimationPersistentState, String> {
    let next_id = reader.u64()?;
    let count = bounded_count(reader, 26, 16384)?;
    reader.claim(count.saturating_mul(std::mem::size_of::<AnimationControllerSnapshot>()))?;
    let mut controllers = Vec::with_capacity(count);
    for _ in 0..count {
        controllers.push(decode_controller(reader, version)?);
    }
    let mut transitions = Vec::new();
    if version >= 4 {
        let count = bounded_count(reader, 10, 16384)?;
        reader.claim(count.saturating_mul(std::mem::size_of::<AnimationPersistentTransition>()))?;
        transitions.reserve(count);
        for _ in 0..count {
            let id = AnimationControllerId::from_bits(reader.u64()?);
            let start_time = match reader.u8()? {
                0 => AnimationTransitionStartTime::Restart,
                1 => AnimationTransitionStartTime::Preserve,
                2 => AnimationTransitionStartTime::MatchPhase,
                3 => AnimationTransitionStartTime::Seek(f64::from_bits(reader.u64()?)),
                _ => return Err("Invalid saved transition start time".into()),
            };
            let source = match reader.u8()? {
                0 => {
                    let source = decode_controller(reader, 4)?;
                    if source.transition.is_some() {
                        return Err("Invalid saved live transition source".into());
                    }
                    AnimationPersistentTransitionSource::Live(source)
                }
                1 => {
                    let reference_time = f64::from_bits(reader.u64()?);
                    let reference_duration = f64::from_bits(reader.u64()?);
                    let bindings = decode_controller(reader, 4)?;
                    if bindings.transition.is_some() {
                        return Err("Invalid saved frozen transition bindings".into());
                    }
                    let count = bounded_count(reader, 29, 65536)?;
                    reader.claim(
                        count.saturating_mul(std::mem::size_of::<AnimationFrozenTransitionValue>()),
                    )?;
                    let mut values = Vec::with_capacity(count);
                    for _ in 0..count {
                        values.push(AnimationFrozenTransitionValue {
                            target: EntityId::from_bits(reader.u64()?),
                            property: decode_target(reader)?,
                            value: decode_value(reader)?,
                            baseline: decode_value(reader)?,
                        });
                    }
                    AnimationPersistentTransitionSource::Frozen {
                        values,
                        bindings,
                        reference_time,
                        reference_duration,
                    }
                }
                _ => return Err("Invalid saved transition source".into()),
            };
            transitions.push(AnimationPersistentTransition {
                id,
                source,
                start_time,
            });
        }
    }
    let directional_starts = if version >= 5 {
        let count = reader.count(8)?;
        let mut ids = Vec::with_capacity(count);
        for _ in 0..count {
            ids.push(AnimationControllerId::from_bits(reader.u64()?));
        }
        ids
    } else {
        Vec::new()
    };
    Ok(AnimationPersistentState {
        next_id,
        controllers,
        transitions,
        directional_starts,
    })
}

fn encode_controller(
    writer: &mut WorldBinaryWriter,
    controller: &AnimationControllerSnapshot,
) -> Result<(), String> {
    writer.u64(controller.id.to_bits())?;
    writer.u8(controller.state as u8)?;
    writer.u64(controller.time.to_bits())?;
    writer.u32(controller.description.speed.to_bits())?;
    writer.u8(u8::from(controller.description.looping))?;
    writer.u8(u8::from(controller.transition.is_some()))?;
    if let Some(transition) = controller.transition {
        writer.u64(transition.duration.to_bits())?;
        writer.u64(transition.elapsed.to_bits())?;
        writer.u8(transition.easing as u8)?;
        writer.u8(u8::from(transition.pending))?;
    }
    writer.count(controller.description.drivers.len())?;
    for driver in &controller.description.drivers {
        writer.string(&driver.source)?;
        writer.u32(driver.variant)?;
        writer.u32(driver.track)?;
        writer.u64(driver.target.to_bits())?;
        encode_target(writer, &driver.property)?;
        writer.u32(driver.weight.to_bits())?;
        writer.u8(u8::from(driver.additive))?;
        writer.u32(driver.reference_time.to_bits())?;
        writer.u8(u8::from(driver.repeat))?;
    }
    Ok(())
}

fn decode_controller(
    reader: &mut WorldBinaryReader<'_>,
    version: u32,
) -> Result<AnimationControllerSnapshot, String> {
    let id = AnimationControllerId::from_bits(reader.u64()?);
    let state = match reader.u8()? {
        0 => AnimationPlaybackStatus::Stopped,
        1 => AnimationPlaybackStatus::Playing,
        2 => AnimationPlaybackStatus::Paused,
        3 => AnimationPlaybackStatus::Completed,
        _ => return Err("Invalid saved animation status".into()),
    };
    let time = f64::from_bits(reader.u64()?);
    let speed = f32::from_bits(reader.u32()?);
    let looping = boolean(reader)?;
    let transition = if version >= 4 && boolean(reader)? {
        Some(AnimationControllerTransitionState {
            duration: f64::from_bits(reader.u64()?),
            elapsed: f64::from_bits(reader.u64()?),
            easing: match reader.u8()? {
                0 => AnimationTransitionEasing::Linear,
                1 => AnimationTransitionEasing::Smoothstep,
                _ => return Err("Invalid saved transition easing".into()),
            },
            pending: boolean(reader)?,
        })
    } else {
        None
    };
    let count = bounded_count(reader, 32, 256)?;
    reader.claim(count.saturating_mul(std::mem::size_of::<AnimationDriverDescription>()))?;
    let mut drivers = Vec::with_capacity(count);
    for _ in 0..count {
        let source = reader.string()?;
        let variant = reader.u32()?;
        let track = reader.u32()?;
        let target = EntityId::from_bits(reader.u64()?);
        let property = decode_target(reader)?;
        drivers.push(AnimationDriverDescription {
            source,
            variant,
            track,
            target,
            property,
            weight: f32::from_bits(reader.u32()?),
            additive: boolean(reader)?,
            reference_time: f32::from_bits(reader.u32()?),
            repeat: if version >= 3 {
                boolean(reader)?
            } else {
                false
            },
        });
    }
    Ok(AnimationControllerSnapshot {
        id,
        state,
        time,
        transition,
        description: AnimationControllerDescription {
            drivers,
            speed,
            looping,
        },
    })
}

fn boolean(reader: &mut WorldBinaryReader<'_>) -> Result<bool, String> {
    match reader.u8()? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err("Invalid saved animation boolean".into()),
    }
}

fn encode_target(
    writer: &mut WorldBinaryWriter,
    target: &AnimationTrackTarget,
) -> Result<(), String> {
    match target {
        AnimationTrackTarget::AnimationProperty(property) => {
            writer.u8(0)?;
            writer.u16(property.component)?;
            writer.count(property.offsets.len())?;
            for offset in &property.offsets {
                writer.u32(*offset)?;
            }
        }
        #[cfg(feature = "skeletal-animation")]
        AnimationTrackTarget::Joints(joints) => {
            writer.u8(1)?;
            writer.u16(0)?;
            writer.count(joints.len())?;
            for joint in joints {
                writer.u32(*joint)?;
            }
        }
        AnimationTrackTarget::DynamicProperty {
            component,
            name,
        } => {
            writer.u8(2)?;
            writer.u16(*component)?;
            writer.count(0)?;
            writer.string(name)?;
        }
    }
    Ok(())
}

fn decode_target(reader: &mut WorldBinaryReader<'_>) -> Result<AnimationTrackTarget, String> {
    let kind = reader.u8()?;
    let component = reader.u16()?;
    let count = bounded_count(reader, 4, 4096)?;
    reader.claim(count.saturating_mul(std::mem::size_of::<u32>()))?;
    let indices = (0..count)
        .map(|_| reader.u32())
        .collect::<Result<Vec<_>, _>>()?;
    Ok(match kind {
        0 => AnimationTrackTarget::AnimationProperty(AnimationProperty {
            component,
            offsets: indices,
        }),
        #[cfg(feature = "skeletal-animation")]
        1 if component == 0 => AnimationTrackTarget::Joints(indices),
        2 if indices.is_empty() => AnimationTrackTarget::DynamicProperty {
            component,
            name: reader.string()?,
        },
        _ => return Err("Invalid saved transition target".into()),
    })
}

fn encode_value(writer: &mut WorldBinaryWriter, value: &AnimationValue) -> Result<(), String> {
    match value {
        AnimationValue::Field(crate::components::schema::FieldValue::F32(value)) => {
            writer.u8(0)?;
            writer.u32(value.to_bits())?;
        }
        AnimationValue::Rotation(value) => {
            writer.u8(1)?;
            for value in value {
                writer.u32(value.to_bits())?;
            }
        }
        #[cfg(feature = "skeletal-animation")]
        AnimationValue::Pose(values) => {
            writer.u8(2)?;
            writer.count(values.len())?;
            for value in values {
                for lane in [
                    value.x, value.y, value.z, value.qx, value.qy, value.qz, value.qw, value.sx,
                    value.sy, value.sz,
                ] {
                    writer.u32(lane.to_bits())?;
                }
            }
        }
        AnimationValue::Field(crate::components::schema::FieldValue::Dynamic(value)) => {
            writer.u8(3)?;
            encode_dynamic(writer, value)?;
        }
        _ => return Err("Unsupported frozen transition value".into()),
    }
    Ok(())
}

fn decode_value(reader: &mut WorldBinaryReader<'_>) -> Result<AnimationValue, String> {
    Ok(match reader.u8()? {
        0 => AnimationValue::Field(crate::components::schema::FieldValue::F32(f32::from_bits(
            reader.u32()?,
        ))),
        1 => AnimationValue::Rotation([
            f32::from_bits(reader.u32()?),
            f32::from_bits(reader.u32()?),
            f32::from_bits(reader.u32()?),
            f32::from_bits(reader.u32()?),
        ]),
        #[cfg(feature = "skeletal-animation")]
        2 => {
            let count = bounded_count(reader, 40, 4096)?;
            reader
                .claim(count.saturating_mul(std::mem::size_of::<crate::components::Transform>()))?;
            let mut values = Vec::with_capacity(count);
            for _ in 0..count {
                values.push(crate::components::Transform {
                    x: f32::from_bits(reader.u32()?),
                    y: f32::from_bits(reader.u32()?),
                    z: f32::from_bits(reader.u32()?),
                    qx: f32::from_bits(reader.u32()?),
                    qy: f32::from_bits(reader.u32()?),
                    qz: f32::from_bits(reader.u32()?),
                    qw: f32::from_bits(reader.u32()?),
                    sx: f32::from_bits(reader.u32()?),
                    sy: f32::from_bits(reader.u32()?),
                    sz: f32::from_bits(reader.u32()?),
                });
            }
            AnimationValue::Pose(values)
        }
        3 => AnimationValue::Field(crate::components::schema::FieldValue::Dynamic(
            decode_dynamic(reader)?,
        )),
        _ => return Err("Invalid frozen transition value".into()),
    })
}

fn encode_dynamic(
    writer: &mut WorldBinaryWriter,
    value: &crate::DynamicValue,
) -> Result<(), String> {
    use crate::DynamicValue::*;
    macro_rules! floats {
        ($tag:expr, $values:expr) => {{
            writer.u8($tag)?;
            for value in $values {
                writer.u32(value.to_bits())?;
            }
        }};
    }
    match value {
        F32(value) => floats!(0, [*value]),
        I32(value) => {
            writer.u8(1)?;
            writer.u32(*value as u32)?;
        }
        U32(value) => {
            writer.u8(2)?;
            writer.u32(*value)?;
        }
        Bool(_) | Asset(_) => return Err("Discrete frozen transition value".into()),
        Vec2(value) => floats!(3, *value),
        Vec3(value) => floats!(4, *value),
        Vec4(value) => floats!(5, *value),
        Mat2(value) => floats!(6, *value),
        Mat3(value) => floats!(7, *value),
        Mat4(value) => floats!(8, *value),
    }
    Ok(())
}

fn decode_dynamic(reader: &mut WorldBinaryReader<'_>) -> Result<crate::DynamicValue, String> {
    Ok(match reader.u8()? {
        0 => crate::DynamicValue::F32(read_f32(reader)?),
        1 => crate::DynamicValue::I32(reader.u32()? as i32),
        2 => crate::DynamicValue::U32(reader.u32()?),
        3 => crate::DynamicValue::Vec2(read_f32_array(reader)?),
        4 => crate::DynamicValue::Vec3(read_f32_array(reader)?),
        5 => crate::DynamicValue::Vec4(read_f32_array(reader)?),
        6 => crate::DynamicValue::Mat2(read_f32_array(reader)?),
        7 => crate::DynamicValue::Mat3(read_f32_array(reader)?),
        8 => crate::DynamicValue::Mat4(read_f32_array(reader)?),
        _ => return Err("Invalid frozen dynamic value".into()),
    })
}

fn read_f32(reader: &mut WorldBinaryReader<'_>) -> Result<f32, String> {
    Ok(f32::from_bits(reader.u32()?))
}

fn read_f32_array<const N: usize>(reader: &mut WorldBinaryReader<'_>) -> Result<[f32; N], String> {
    let mut values = [0.0; N];
    for value in &mut values {
        *value = read_f32(reader)?;
    }
    Ok(values)
}

fn bounded_count(
    reader: &mut WorldBinaryReader<'_>,
    minimum_bytes: usize,
    maximum: usize,
) -> Result<usize, String> {
    let count = reader.count(minimum_bytes)?;
    if count > maximum {
        return Err("Saved animation collection limit".into());
    }
    Ok(count)
}

pub(super) fn validate_values(state: &AnimationPersistentState) -> Result<(), String> {
    if state.next_id == 0
        || state.controllers.len() > MAX_CONTROLLERS
        || state.transitions.len() > MAX_CONTROLLERS
    {
        return Err("Invalid saved animation identity counter or count".into());
    }

    let mut previous = 0;
    let mut controllers = BTreeMap::new();
    for controller in &state.controllers {
        let id = controller.id.to_bits();
        if id <= previous || id >= state.next_id {
            return Err("Invalid saved animation controller".into());
        }
        validate_controller(controller, true, id)?;
        previous = id;
        controllers.insert(id, controller);
    }
    let mut directional_ids = BTreeSet::new();
    for id in &state.directional_starts {
        let bits = id.to_bits();
        let controller = controllers
            .get(&bits)
            .ok_or("Saved directional start references a missing controller")?;
        if !directional_ids.insert(bits)
            || !matches!(
                controller.state,
                AnimationPlaybackStatus::Playing | AnimationPlaybackStatus::Paused
            )
        {
            return Err("Invalid saved directional start".into());
        }
    }

    let mut transition_ids = BTreeSet::new();
    for transition in &state.transitions {
        let id = transition.id.to_bits();
        let destination = controllers
            .get(&id)
            .ok_or("Saved transition references a missing controller")?;
        if destination.transition.is_none() || !transition_ids.insert(id) {
            return Err("Invalid or duplicate saved transition sidecar".into());
        }
        if let AnimationTransitionStartTime::Seek(time) = transition.start_time
            && (!time.is_finite() || time < 0.0)
        {
            return Err("Invalid saved transition start time".into());
        }
        match &transition.source {
            AnimationPersistentTransitionSource::Live(source) => {
                validate_controller(source, false, id)?;
            }
            AnimationPersistentTransitionSource::Frozen {
                values,
                bindings,
                reference_time,
                reference_duration,
            } => {
                validate_controller(bindings, false, id)?;
                if values.len() > 65536
                    || !reference_duration.is_finite()
                    || *reference_duration < 0.0
                    || !reference_time.is_finite()
                    || *reference_time < 0.0
                    || reference_time > reference_duration
                {
                    return Err("Invalid saved frozen transition metadata".into());
                }
                let mut keys = BTreeSet::new();
                for value in values {
                    validate_frozen_value(value)?;
                    if !keys.insert((value.target.to_bits(), target_key(&value.property))) {
                        return Err("Duplicate saved frozen transition value".into());
                    }
                }
            }
        }
    }

    if controllers
        .iter()
        .any(|(id, controller)| controller.transition.is_some() != transition_ids.contains(id))
    {
        return Err("Saved transition summary and sidecar mismatch".into());
    }
    Ok(())
}

fn validate_controller(
    controller: &AnimationControllerSnapshot,
    allow_transition: bool,
    expected_id: u64,
) -> Result<(), String> {
    if controller.id.to_bits() != expected_id
        || !controller.time.is_finite()
        || controller.time < 0.0
        || !controller.description.speed.is_finite()
        || controller.description.drivers.len() > 256
        || (!allow_transition && controller.transition.is_some())
    {
        return Err("Invalid saved animation controller".into());
    }
    if let Some(transition) = controller.transition
        && (!transition.duration.is_finite()
            || transition.duration < 0.0
            || !transition.elapsed.is_finite()
            || transition.elapsed < 0.0
            || transition.elapsed > transition.duration)
    {
        return Err("Invalid saved transition summary".into());
    }
    for driver in &controller.description.drivers {
        validate_target(&driver.property, false)?;
        if driver.source.is_empty()
            || driver.source.len() > 2048
            || driver.track >= 256
            || !driver.weight.is_finite()
            || !(0.0..=1.0).contains(&driver.weight)
            || !driver.reference_time.is_finite()
            || driver.reference_time < 0.0
        {
            return Err("Invalid saved animation driver".into());
        }
    }
    Ok(())
}

fn validate_target(target: &AnimationTrackTarget, _frozen: bool) -> Result<(), String> {
    if target.indices().len() > 4096
        || (target.indices().is_empty()
            && !matches!(target, AnimationTrackTarget::DynamicProperty { .. }))
    {
        return Err("Invalid saved animation target".into());
    }
    if let AnimationTrackTarget::DynamicProperty {
        component,
        name,
    } = target
    {
        crate::DynamicProperties::validate_name(name)
            .map_err(|_| "Invalid saved dynamic property name")?;
        if !crate::ComponentValue::supports_dynamic_properties(*component) {
            return Err("Invalid saved dynamic component".into());
        }
    }
    #[cfg(feature = "skeletal-animation")]
    if let AnimationTrackTarget::Joints(joints) = target
        && _frozen
        && joints.len() != 1
    {
        return Err("Frozen transition joints must contain one ordinal".into());
    }
    Ok(())
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum TargetKey<'a> {
    Property(u16, &'a [u32]),
    #[cfg(feature = "skeletal-animation")]
    Joints(&'a [u32]),
    Dynamic(u16, &'a str),
}

fn target_key(target: &AnimationTrackTarget) -> TargetKey<'_> {
    match target {
        AnimationTrackTarget::AnimationProperty(property) => {
            TargetKey::Property(property.component, &property.offsets)
        }
        #[cfg(feature = "skeletal-animation")]
        AnimationTrackTarget::Joints(joints) => TargetKey::Joints(joints),
        AnimationTrackTarget::DynamicProperty {
            component,
            name,
        } => TargetKey::Dynamic(*component, name),
    }
}

fn validate_frozen_value(value: &AnimationFrozenTransitionValue) -> Result<(), String> {
    validate_target(&value.property, true)?;
    let valid = super::driver::frozen_transition_target_supported(&value.property, &value.value)
        && value_matches_target(&value.property, &value.value)
        && value_matches_target(&value.property, &value.baseline)
        && match (&value.value, &value.baseline) {
            (
                AnimationValue::Field(crate::components::schema::FieldValue::F32(a)),
                AnimationValue::Field(crate::components::schema::FieldValue::F32(b)),
            ) => a.is_finite() && b.is_finite(),
            (AnimationValue::Rotation(a), AnimationValue::Rotation(b)) => {
                a.iter().chain(b).all(|lane| lane.is_finite())
            }
            (
                AnimationValue::Field(crate::components::schema::FieldValue::Dynamic(a)),
                AnimationValue::Field(crate::components::schema::FieldValue::Dynamic(b)),
            ) => dynamic_kind(a) == dynamic_kind(b) && dynamic_finite(a) && dynamic_finite(b),
            #[cfg(feature = "skeletal-animation")]
            (AnimationValue::Pose(a), AnimationValue::Pose(b)) => {
                a.len() == 1 && b.len() == 1 && transform_finite(&a[0]) && transform_finite(&b[0])
            }
            _ => false,
        };
    if valid {
        Ok(())
    } else {
        Err("Invalid saved frozen transition value".into())
    }
}

fn value_matches_target(target: &AnimationTrackTarget, value: &AnimationValue) -> bool {
    match target {
        AnimationTrackTarget::AnimationProperty(_) => matches!(
            value,
            AnimationValue::Field(crate::components::schema::FieldValue::F32(_))
                | AnimationValue::Rotation(_)
        ),
        #[cfg(feature = "skeletal-animation")]
        AnimationTrackTarget::Joints(_) => matches!(value, AnimationValue::Pose(_)),
        AnimationTrackTarget::DynamicProperty {
            ..
        } => matches!(
            value,
            AnimationValue::Field(crate::components::schema::FieldValue::Dynamic(_))
        ),
    }
}

fn dynamic_kind(value: &crate::DynamicValue) -> u8 {
    use crate::DynamicValue::*;
    match value {
        F32(_) => 0,
        I32(_) => 1,
        U32(_) => 2,
        Bool(_) => 3,
        Asset(_) => 4,
        Vec2(_) => 5,
        Vec3(_) => 6,
        Vec4(_) => 7,
        Mat2(_) => 8,
        Mat3(_) => 9,
        Mat4(_) => 10,
    }
}

fn dynamic_finite(value: &crate::DynamicValue) -> bool {
    use crate::DynamicValue::*;
    match value {
        F32(value) => value.is_finite(),
        I32(_) | U32(_) => true,
        Bool(_) | Asset(_) => false,
        Vec2(values) => values.iter().all(|value| value.is_finite()),
        Vec3(values) => values.iter().all(|value| value.is_finite()),
        Vec4(values) => values.iter().all(|value| value.is_finite()),
        Mat2(values) => values.iter().all(|value| value.is_finite()),
        Mat3(values) => values.iter().all(|value| value.is_finite()),
        Mat4(values) => values.iter().all(|value| value.is_finite()),
    }
}

#[cfg(feature = "skeletal-animation")]
fn transform_finite(value: &crate::components::Transform) -> bool {
    [
        value.x, value.y, value.z, value.qx, value.qy, value.qz, value.qw, value.sx, value.sy,
        value.sz,
    ]
    .iter()
    .all(|value| value.is_finite())
}

#[cfg(test)]
#[path = "codec_tests.rs"]
mod tests;
