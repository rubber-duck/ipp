//! Explicit compiled membership is the only source of component wire identities.

use crate::components::schema::{ContractSink, write_string};
ipp_schema_derive::component_registry! {
    pub ComponentValue {
        Scalar = 1,
        #[cfg(test)]
        PreparedBuffer = 60000,
        LinearDriver = 2,
        #[runtime(crate::systems::hierarchy::ObjectTransformRuntime)]
        Transform = 3,
        UnlitMaterial = 4,
        MeshInstance = 5,
        UnlitTexture = 6,
        Camera = 7,
        PbrMaterial = 10,
        Light = 11,
        #[cfg(feature = "skeletal-animation")]
        Skeleton = 12,
        #[cfg(feature = "skeletal-animation")]
        Skin = 13,
        BoundingGeometry = 14,
        PickingGeometry = 15,
        #[cfg(feature = "mesh-poses")]
        MeshPose = 16,
        Hierarchy = 17,
        LookAt = 18,
        BaseColorTexture = 19,
        CustomMaterial = 20,
        #[cfg(feature = "particles")]
        ParticleEmitter = 21,
        #[cfg(feature = "particles")]
        ParticlePlayback = 22,
        #[cfg(feature = "particles")]
        ParticleSprite = 23,
        #[cfg(feature = "particles")]
        ParticleMesh = 24,
        #[cfg(feature = "surfaces")]
        Surface = 25,
        #[cfg(feature = "gui")]
        GuiRoot = 26,
        #[cfg(feature = "surfaces")]
        SurfaceCache = 27,
    }
}

/// Streams target/build identity and the feature-conditioned compiled registry.
pub fn write_contract(sink: &mut impl ContractSink) {
    sink.write(&4u16.to_le_bytes());
    write_string(sink, std::env::consts::ARCH);
    write_string(sink, std::env::consts::OS);
    sink.write(&[usize::BITS as u8]);
    // Feature identities are stable; retired baseline and split-rig IDs are not reused.
    let features = [
        (11, "builtin-assets", cfg!(feature = "builtin-assets")),
        (13, "shadows", cfg!(feature = "shadows")),
        (15, "mesh-poses", cfg!(feature = "mesh-poses")),
        (
            16,
            "skeletal-animation",
            cfg!(feature = "skeletal-animation"),
        ),
        (17, "particles", cfg!(feature = "particles")),
        (18, "surfaces", cfg!(feature = "surfaces")),
        (19, "gui", cfg!(feature = "gui")),
    ];
    sink.write(&[features.len() as u8]);
    for (id, name, enabled) in features {
        sink.write(&[id, u8::from(enabled)]);
        write_string(sink, name);
    }
    ComponentValue::write_contract(sink);
}

/// Create a producer component using the compiled factory.
pub fn create(id: u16) -> Result<ComponentValue, crate::ErrorReason> {
    ComponentValue::create(id).map_err(field_error)
}

/// Apply a command after the world has resolved provisional references.
///
/// The write is atomic per field and costs the written field, not the whole
/// component: it replaces the field in place and restores the previous value when
/// field-local lifecycle validation rejects the result. Replacing the dynamic
/// property metadata rebuilds every property, so it validates a staged copy.
pub fn write(
    component: &mut ComponentValue,
    field: &crate::FieldWrite,
) -> Result<(), crate::ErrorReason> {
    write_field(component, field).map(|_| ())
}

/// [`write`], also reporting whether the component's value equality changed.
pub(crate) fn write_field(
    component: &mut ComponentValue,
    field: &crate::FieldWrite,
) -> Result<bool, crate::ErrorReason> {
    use crate::components::dynamic_properties::{DYNAMIC_METADATA, is_dynamic_field};

    let value = schema_value(field)?;
    if field.offset == DYNAMIC_METADATA {
        let mut staged = component.clone();
        staged.set_field(field.offset, value).map_err(field_error)?;
        staged.validate_field_lifecycle(field.offset)?;
        let changed = staged != *component;
        *component = staged;
        return Ok(changed);
    }

    // Dynamic values compare by their stored bytes, exactly like whole values.
    let stored = |component: &ComponentValue| {
        component
            .dynamic_properties()
            .and_then(|properties| properties.stored_value(field.offset))
    };
    let dynamic = is_dynamic_field(field.offset);
    let before = dynamic.then(|| stored(component));
    let previous = component.field(field.offset).map_err(field_error)?;
    component
        .set_field(field.offset, value)
        .map_err(field_error)?;
    if let Err(reason) = component.validate_field_lifecycle(field.offset) {
        component
            .set_field(field.offset, previous)
            .expect("previous field value remains writable");
        return Err(reason);
    }
    Ok(match before {
        Some(before) => stored(component) != before,
        None => component.field(field.offset).ok().as_ref() != Some(&previous),
    })
}

/// Replay a field write that already passed validation on an identical value.
pub(crate) fn replay_field(
    component: &mut ComponentValue,
    field: &crate::FieldWrite,
) -> Result<(), crate::ErrorReason> {
    component
        .set_field(field.offset, schema_value(field)?)
        .map_err(field_error)
}

fn schema_value(
    field: &crate::FieldWrite,
) -> Result<crate::components::schema::FieldValue, crate::ErrorReason> {
    Ok(match field.value.clone() {
        crate::FieldValue::Dynamic(v) => crate::components::schema::FieldValue::Dynamic(v),
        crate::FieldValue::F32(v) => crate::components::schema::FieldValue::F32(v),
        crate::FieldValue::U32(v) => crate::components::schema::FieldValue::U32(v),
        crate::FieldValue::U64(v) => crate::components::schema::FieldValue::U64(v),
        crate::FieldValue::String(v) => crate::components::schema::FieldValue::String(v),
        crate::FieldValue::Bytes(v) => crate::components::schema::FieldValue::Bytes(v),
        crate::FieldValue::Bool(v) => crate::components::schema::FieldValue::Bool(v),
        crate::FieldValue::Entity(crate::EntityRef::Handle(id)) => {
            crate::components::schema::FieldValue::Entity(id)
        }
        crate::FieldValue::Entity(crate::EntityRef::Alias(_)) => {
            return Err(crate::ErrorReason::UnknownAlias);
        }
    })
}

fn field_error(error: crate::components::schema::FieldError) -> crate::ErrorReason {
    match error {
        crate::components::schema::FieldError::UnknownComponent => {
            crate::ErrorReason::UnknownComponent
        }
        crate::components::schema::FieldError::NonFinite => crate::ErrorReason::InvalidValue,
        crate::components::schema::FieldError::CreationUnavailable => {
            crate::ErrorReason::MissingCreationContract
        }
        _ => crate::ErrorReason::InvalidField,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FieldValue, FieldWrite, components::MeshInstance};

    #[test]
    fn resource_source_policy_runs_in_the_component_hook_after_typed_dispatch() {
        let source = "archive!/mesh?name=ordinary text";
        let mut component = ComponentValue::MeshInstance(MeshInstance::default());
        assert_eq!(
            write(
                &mut component,
                &FieldWrite {
                    offset: std::mem::offset_of!(MeshInstance, source) as u32,
                    value: FieldValue::String(source.into()),
                },
            ),
            Ok(())
        );

        assert_eq!(
            write(
                &mut component,
                &FieldWrite {
                    offset: std::mem::offset_of!(MeshInstance, source) as u32,
                    value: FieldValue::String("x".repeat(4097,)),
                },
            ),
            Ok(())
        );

        assert_eq!(
            component,
            ComponentValue::MeshInstance(MeshInstance {
                source: "x".repeat(4097),
                variant: 0,
            })
        );
    }
}
