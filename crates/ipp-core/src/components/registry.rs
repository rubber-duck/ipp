//! Explicit compiled membership is the only source of component wire identities.

use crate::components::schema::{ContractSink, write_string};
ipp_schema_derive::component_registry! {
    pub ComponentValue {
        Scalar = 1,
        #[cfg(test)]
        RowsFixture = 60001,
        LinearDriver = 2,
        Transform = 3,
        UnlitMaterial = 4,
        MeshInstance = 5,
        UnlitTexture = 6,
        Camera = 7,
        PbrMaterial = 10,
        Light = 11,
        Skeleton = 12,
        Skin = 13,
        BoundingGeometry = 14,
        PickingGeometry = 15,
        MeshPose = 16,
        ParentJoint = 17,
        LookAt = 18,
        BaseColorTexture = 19,
        CustomMaterial = 20,
        ParticleEmitter = 21,
        ParticlePlayback = 22,
        ParticleSprite = 23,
        ParticleMesh = 24,
        Surface = 25,
        // 26 was GuiRoot; retired identities are never reused.
        SurfaceCache = 27,
        WorldAttachment = 28,
        // 29 was Canvas; a World's canvas is Canvas System state.
        CanvasStyle = 30,
        CanvasText = 31,
        CanvasGlyphRun = 32,
        CanvasDrawing = 33,
        CanvasBitmap = 34,
        CanvasBox = 35,
        GuiBehavior = 36,
        GuiButton = 37,
        GuiCheckbox = 38,
        GuiSlider = 39,
        GuiTextInput = 40,
        GuiLayout = 41,
        GuiTheme = 42,
        GuiSkin = 43,
        GuiFont = 44,
        GuiThemeMotion = 45,
        GuiScrollView = 46,
        GuiVirtualList = 47,
        GuiVirtualItem = 48,
        CanvasBounds = 49,
        GuiOverlay = 50,
        GuiGroup = 51,
        CanvasPaint = 52,
        GuiColor = 53,
    }
}

/// Receives one occupied stable component cell typed as its component, from
/// [`ComponentStorage::visit_cell`]. Implementations bind through the derived
/// schema and lifecycle accessors of `C`; the pointer carries the storage
/// cell's binding contract (see `ComponentStorage` bound pointers).
pub(crate) trait ComponentCellVisitor {
    /// Result produced from the typed cell.
    type Output;

    /// Visit the stable cell of one component value.
    fn visit<C>(self, cell: std::ptr::NonNull<C>) -> Self::Output
    where
        C: crate::components::schema::SchemaComponent
            + crate::components::schema::ComponentLifecycle
            + 'static;
}

/// Streams target/build identity and the compiled registry.
pub fn write_contract(sink: &mut impl ContractSink) {
    sink.write(&10u16.to_le_bytes());
    write_string(sink, std::env::consts::ARCH);
    write_string(sink, std::env::consts::OS);
    sink.write(&[usize::BITS as u8]);
    // Rows addressing and size bounds, so generated codecs and the generator's export
    // validation read them instead of repeating them.
    sink.write(&super::rows::ROW_REGION_SPAN.to_le_bytes());
    sink.write(&[super::rows::MAX_ROW_FIELDS as u8]);
    sink.write(&(super::rows::MAX_ROW_PROPERTIES as u16).to_le_bytes());
    sink.write(&super::rows::MAX_ROW_TEXT_BYTES.to_le_bytes());
    ComponentValue::write_contract(sink);
    crate::systems::gui::presentation::write_paint_contract(sink);
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
///
/// [`crate::FieldValue::Unset`] clears an optional row property; a rejected
/// write restores the previous value, including its absence.
pub(crate) fn write_field(
    component: &mut ComponentValue,
    field: &crate::FieldWrite,
) -> Result<bool, crate::ErrorReason> {
    use crate::components::dynamic_properties::{DYNAMIC_METADATA, is_dynamic_field};

    let value = schema_value(field)?;
    validate_asset_field(component.type_id(), field.offset, &value)?;
    let rows = matches!(value, crate::components::schema::FieldValue::Rows(_));
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
    let mut validation = component.validate_field_lifecycle(field.offset);
    if rows && validation.is_ok() {
        component.visit_row_field_assets(field.offset, &mut |source| {
            validation = validation.and(source.validate());
        });
    }
    if let Err(reason) = validation {
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

/// [`write_field`] on a stored value whose whole invariants must still hold.
///
/// Components that validate after every operation (see
/// `ComponentLifecycle::validates_after_operation`) check the complete result;
/// a result that fails restores the previous value, so a rejected write has no
/// effect.
pub(crate) fn write_stored_field(
    component: &mut ComponentValue,
    field: &crate::FieldWrite,
) -> Result<bool, crate::ErrorReason> {
    use crate::components::dynamic_properties::DYNAMIC_METADATA;

    if !component.validates_after_operation() {
        return write_field(component, field);
    }
    if field.offset == DYNAMIC_METADATA {
        let mut candidate = component.clone();
        let changed = write_field(&mut candidate, field)?;
        candidate.validate_lifecycle()?;
        *component = candidate;
        return Ok(changed);
    }

    let previous = component.field(field.offset).map_err(field_error)?;
    let changed = write_field(component, field)?;
    if let Err(reason) = component.validate_lifecycle() {
        component
            .set_field(field.offset, previous)
            .expect("previous field value remains writable");
        return Err(reason);
    }
    Ok(changed)
}

/// Whether a stored field equals the value a compare-and-set expects.
///
/// Text compares by shared reference first and then by content; every other
/// kind compares by value. An expected value of another type than the field's
/// is an invalid field write.
pub(crate) fn field_matches(
    component: &ComponentValue,
    expected: &crate::FieldWrite,
) -> Result<bool, crate::ErrorReason> {
    use crate::components::schema::{FieldValue, same_text};

    let expected_value = schema_value(expected)?;
    ComponentValue::validate_field(component.type_id(), expected.offset, expected_value.kind())
        .map_err(field_error)?;

    let current = component.field(expected.offset).map_err(field_error)?;
    Ok(match (&current, &expected_value) {
        (FieldValue::String(current), FieldValue::String(expected)) => same_text(current, expected),
        _ => current == expected_value,
    })
}

/// Assign a field of a value that is not yet visible, such as a component
/// being created, without validating it. The caller validates every assigned
/// field once all assignments are complete, so the result does not depend on
/// their order.
pub(crate) fn assign(
    component: &mut ComponentValue,
    field: &crate::FieldWrite,
) -> Result<(), crate::ErrorReason> {
    component
        .set_field(field.offset, schema_value(field)?)
        .map_err(field_error)
}

fn validate_asset_field(
    component: u16,
    offset: u32,
    value: &crate::components::schema::FieldValue,
) -> Result<(), crate::ErrorReason> {
    if let crate::components::schema::FieldValue::Dynamic(crate::DynamicValue::Asset(source)) =
        value
    {
        source.validate()?;
    }
    if let crate::components::schema::FieldValue::String(source) = value
        && let Some(reference) = ComponentValue::asset_references(component)
            .iter()
            .find(|reference| reference.source_offset == offset)
    {
        crate::services::asset_management::validate_reference(
            Some(crate::services::asset_management::AssetTypeId(
                reference.kind,
            )),
            source,
        )?;
    }
    Ok(())
}

pub(crate) fn validate_asset_references(value: &ComponentValue) -> Result<(), crate::ErrorReason> {
    for reference in ComponentValue::asset_references(value.type_id()) {
        let field = value.field(reference.source_offset).map_err(field_error)?;
        validate_asset_field(value.type_id(), reference.source_offset, &field)?;
    }
    let mut result = Ok(());
    value.visit_row_assets(&mut |source| {
        result = result.and(source.validate());
    });
    result?;
    if let Some(properties) = value.dynamic_properties() {
        let mut demand = std::collections::BTreeSet::new();
        properties.resource_demand(&mut demand);
        for selection in demand {
            selection.descriptor().validate()?;
        }
    }
    Ok(())
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
        crate::FieldValue::UnresolvedWorld(_) | crate::FieldValue::UnresolvedOutput(_) => {
            return Err(crate::ErrorReason::InvalidValue);
        }
        crate::FieldValue::World(value) => crate::components::schema::FieldValue::World(value),
        crate::FieldValue::Output(value) => crate::components::schema::FieldValue::Output(value),
        crate::FieldValue::Dynamic(v) => crate::components::schema::FieldValue::Dynamic(v),
        crate::FieldValue::F32(v) => crate::components::schema::FieldValue::F32(v),
        crate::FieldValue::U32(v) => crate::components::schema::FieldValue::U32(v),
        crate::FieldValue::U64(v) => crate::components::schema::FieldValue::U64(v),
        crate::FieldValue::String(v) => crate::components::schema::FieldValue::String(v),
        crate::FieldValue::Bytes(v) => crate::components::schema::FieldValue::Bytes(v),
        crate::FieldValue::Bool(v) => crate::components::schema::FieldValue::Bool(v),
        crate::FieldValue::Rows(v) => crate::components::schema::FieldValue::Rows(v),
        crate::FieldValue::Unset => crate::components::schema::FieldValue::Unset,
        crate::FieldValue::Entity(crate::EntityRef::Handle(id)) => {
            crate::components::schema::FieldValue::Entity(id)
        }
        crate::FieldValue::Entity(crate::EntityRef::Alias(_) | crate::EntityRef::Symbol(_)) => {
            return Err(crate::ErrorReason::UnknownAlias);
        }
    })
}

fn field_error(error: crate::components::schema::FieldError) -> crate::ErrorReason {
    match error {
        crate::components::schema::FieldError::UnknownComponent => {
            crate::ErrorReason::UnknownComponent
        }
        crate::components::schema::FieldError::NonFinite
        | crate::components::schema::FieldError::TextTooLong => crate::ErrorReason::InvalidValue,
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
    fn private_dynamic_assignments_defer_reference_admission_to_the_final_value() {
        use crate::services::asset_management::{AssetSource, AssetTypeId};
        let asset = |uri: &str| {
            crate::DynamicValue::Asset(AssetSource {
                kind: AssetTypeId(2),
                uri: uri.into(),
                variant: 0,
            })
        };
        let mut component = ComponentValue::CustomMaterial(Default::default());
        let key = component
            .dynamic_properties_mut()
            .unwrap()
            .set("selected", asset("asset://2/42"))
            .unwrap();
        let valid = component.clone();
        let invalid = FieldWrite {
            offset: key,
            value: FieldValue::Dynamic(asset("producer://7/2/not-an-id")),
        };
        assign(&mut component, &invalid).unwrap();
        assert_eq!(
            validate_asset_references(&component),
            Err(crate::ErrorReason::InvalidAsset)
        );
        assign(
            &mut component,
            &FieldWrite {
                offset: key,
                value: FieldValue::Dynamic(asset("asset://2/42")),
            },
        )
        .unwrap();
        assert_eq!(validate_asset_references(&component), Ok(()));
        assert_eq!(component, valid);
        assert_eq!(
            write(&mut component, &invalid),
            Err(crate::ErrorReason::InvalidAsset)
        );
        assert_eq!(component, valid);
        assert!(
            component
                .dynamic_properties_mut()
                .unwrap()
                .set_key(key, asset("asset://malformed"))
                .is_err()
        );
        assert_eq!(component, valid);
    }

    #[test]
    fn static_asset_fields_validate_the_declared_kind_before_replacement() {
        for mut component in [
            ComponentValue::MeshInstance(MeshInstance::default()),
            ComponentValue::CustomMaterial(crate::components::CustomMaterial::default()),
            ComponentValue::UnlitTexture(crate::components::UnlitTexture::default()),
        ] {
            let reference = ComponentValue::asset_references(component.type_id())[0];
            let previous = component.clone();
            for source in [
                "asset://malformed".to_owned(),
                format!("asset://{}/42", reference.kind + 1),
            ] {
                let field = FieldWrite {
                    offset: reference.source_offset,
                    value: FieldValue::String(source.into()),
                };
                assert_eq!(
                    write(&mut component, &field),
                    Err(crate::ErrorReason::InvalidAsset)
                );
                assert_eq!(component, previous);
            }
            write(
                &mut component,
                &FieldWrite {
                    offset: reference.source_offset,
                    value: FieldValue::String(format!("asset://{}/42", reference.kind).into()),
                },
            )
            .unwrap();
        }
    }

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
                    value: FieldValue::String("x".repeat(4097).into()),
                },
            ),
            Ok(())
        );

        assert_eq!(
            component,
            ComponentValue::MeshInstance(MeshInstance {
                source: "x".repeat(4097).into(),
                variant: 0,
            })
        );
    }
}
