use super::{DataBindingRuntime, decode_data_windows, encode_data_windows};
use crate::{
    DynamicProperties, DynamicPropertyKind, ErrorReason,
    components::schema::{ComponentLifecycle, FieldValue, SchemaComponent},
    services::data::*,
};
use std::sync::Arc;

/// Mutable-buffer projection authoring. `foo` is an expression Asset, and
/// `foo_parameter` is its optional ordinary typed shared parameter.
#[repr(C)]
#[derive(Clone, Debug, Default, PartialEq, SchemaComponent)]
pub struct BufferDataSourceBinding {
    /// Host-wide stable source name; empty permits incomplete authoring.
    pub source: Arc<str>,
    /// Authored output asset references and optional typed companion parameters.
    #[schema(ignore)]
    pub properties: DynamicProperties,
    /// Private reconstructible evaluation and presentation state, excluded persistence.
    #[schema(ignore)]
    pub runtime: DataBindingRuntime,
}

/// Append-only projection authoring with intersecting raw-source windows.
#[repr(C)]
#[derive(Clone, Debug, Default, PartialEq, SchemaComponent)]
pub struct StreamingDataSourceBinding {
    /// Host-wide stable source name; empty permits incomplete authoring.
    pub source: Arc<str>,
    /// Portable IPPW version 1; use `set_windows`/`windows` for typed Rust access.
    /// Empty bytes select the Data Service's default cap, never a binding-local cap.
    pub windows: Vec<u8>,
    /// Authored output asset references and optional typed companion parameters.
    #[schema(ignore)]
    pub properties: DynamicProperties,
    /// Private reconstructible evaluation and presentation state, excluded persistence.
    #[schema(ignore)]
    pub runtime: DataBindingRuntime,
}

impl StreamingDataSourceBinding {
    /// Decode intersecting raw-source window constraints.
    pub fn windows(&self) -> Result<Vec<DataWindow>, ErrorReason> {
        decode_data_windows(&self.windows)
    }

    /// Syntax validation only; World admission also validates forward-moving anchors
    /// against committed demand and current Host time before changing stored fields.
    pub fn set_windows(&mut self, windows: &[DataWindow]) -> Result<(), ErrorReason> {
        self.windows = encode_data_windows(windows)?;
        self.runtime.invalidate();
        Ok(())
    }
}

pub(super) fn request(
    value: &crate::ComponentValue,
) -> Option<Result<DataConsumerRequest, ErrorReason>> {
    match value {
        crate::ComponentValue::BufferDataSourceBinding(v) => Some(Ok(DataConsumerRequest {
            name: v.source.to_string(),
            kind: DataSourceKind::Buffer,
            windows: vec![],
        })),
        crate::ComponentValue::StreamingDataSourceBinding(v) => {
            Some(v.windows().map(|windows| DataConsumerRequest {
                name: v.source.to_string(),
                kind: DataSourceKind::Streaming,
                windows,
            }))
        }
        _ => None,
    }
}

fn validate(source: &str, properties: &DynamicProperties) -> Result<(), ErrorReason> {
    if source.chars().any(char::is_control) {
        return Err(ErrorReason::InvalidValue);
    }
    for (name, descriptor) in properties.descriptors() {
        if descriptor.kind == DynamicPropertyKind::Asset {
            if name.ends_with("_parameter") || name.is_empty() {
                return Err(ErrorReason::InvalidValue);
            }
            let asset = properties.asset(name).ok_or(ErrorReason::InvalidAsset)?;
            if asset.kind != crate::services::asset_management::expression::EXPRESSION_TYPE {
                return Err(ErrorReason::InvalidAsset);
            }
            asset.validate()?;
        } else if !name.ends_with("_parameter") {
            return Err(ErrorReason::InvalidValue);
        }
    }
    Ok(())
}

macro_rules! lifecycle {
    ($component:ty, $extra:expr) => {
        impl ComponentLifecycle for $component {
            fn supports_dynamic_properties() -> bool {
                true
            }

            fn dynamic_properties(&self) -> Option<&DynamicProperties> {
                Some(&self.properties)
            }

            fn dynamic_properties_mut(&mut self) -> Option<&mut DynamicProperties> {
                Some(&mut self.properties)
            }

            fn preserve_runtime(&mut self, previous: &mut Self) {
                self.runtime = std::mem::take(&mut previous.runtime);
                self.runtime.invalidate();
            }

            fn after_field_write(&mut self, _: u32) {
                self.runtime.invalidate();
            }

            fn validate(&self) -> Result<(), ErrorReason> {
                validate(&self.source, &self.properties)?;
                ($extra)(self)
            }

            fn resource_demand(
                &self,
                demand: &mut std::collections::BTreeSet<
                    crate::services::asset_management::service::AssetDemandSelection,
                >,
            ) {
                self.properties.resource_demand(demand);
            }

            fn supports_numeric_property(offset: u32) -> bool {
                crate::components::dynamic_properties::is_dynamic_field(offset)
                    && offset != crate::components::dynamic_properties::DYNAMIC_METADATA
            }

            fn validate_numeric_properties(
                &self,
                fields: &[(u32, FieldValue)],
            ) -> Result<(), ErrorReason> {
                for (offset, field) in fields {
                    let FieldValue::Dynamic(value) = field else {
                        return Err(ErrorReason::InvalidField);
                    };
                    if !Self::supports_numeric_property(*offset)
                        || value.kind() == DynamicPropertyKind::Asset
                        || self
                            .properties
                            .get_key(*offset)
                            .is_none_or(|previous| previous.kind() != value.kind())
                    {
                        return Err(ErrorReason::InvalidField);
                    }
                    value.validate().map_err(|_| ErrorReason::InvalidValue)?;
                }
                Ok(())
            }
        }
    };
}

lifecycle!(
    BufferDataSourceBinding,
    |_: &BufferDataSourceBinding| Ok(())
);
lifecycle!(
    StreamingDataSourceBinding,
    |v: &StreamingDataSourceBinding| v.windows().map(|_| ())
);
