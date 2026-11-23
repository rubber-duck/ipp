//! Independent component lifetimes for axis and callout Canvas paints.
use crate::components::schema::{ComponentAssetReference, ComponentLifecycle};
use crate::services::asset_management::AssetDemandSelection;
use crate::{ComponentValue, ErrorReason};
use ipp_schema_derive::SchemaComponent;
use std::{collections::BTreeSet, sync::Arc};

macro_rules! plot_paint {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[repr(C)]
        #[derive(Clone, Debug, Default, PartialEq, SchemaComponent)]
        pub struct $name {
            /// Shared Canvas paint definition.
            pub source: Arc<str>,
            /// Definition variant.
            pub variant: u32,
            /// Ordinary independently editable shader parameters.
            #[schema(ignore)]
            pub properties: crate::DynamicProperties,
        }

        impl ComponentLifecycle for $name {
            fn supports_numeric_property(offset: u32) -> bool {
                crate::components::dynamic_properties::is_dynamic_field(offset)
                    && offset != crate::components::dynamic_properties::DYNAMIC_METADATA
            }

            fn validate_numeric_properties(
                &self,
                fields: &[(u32, crate::components::schema::FieldValue)],
            ) -> Result<(), ErrorReason> {
                // Only existing numeric properties change in place; the source and its
                // variant are resource fields.
                for (offset, field) in fields {
                    let crate::components::schema::FieldValue::Dynamic(value) = field else {
                        return Err(ErrorReason::InvalidField);
                    };
                    if !Self::supports_numeric_property(*offset)
                        || value.kind() == crate::DynamicPropertyKind::Asset
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

            fn supports_dynamic_properties() -> bool {
                true
            }

            fn dynamic_properties(&self) -> Option<&crate::DynamicProperties> {
                Some(&self.properties)
            }

            fn dynamic_properties_mut(&mut self) -> Option<&mut crate::DynamicProperties> {
                Some(&mut self.properties)
            }

            fn validate(&self) -> Result<(), ErrorReason> {
                crate::services::asset_management::validate_source(&self.source)
            }

            /// Dynamic properties are indexed state: each write is checked where it
            /// happens, by its own value.
            fn validates_after_operation() -> bool {
                false
            }

            fn validate_field(&self, _offset: u32) -> Result<(), ErrorReason> {
                self.validate()
            }

            fn asset_references() -> &'static [ComponentAssetReference] {
                &[ComponentAssetReference {
                    kind: crate::services::asset_management::formats::shader::SHADER_TYPE.0,
                    source_offset: std::mem::offset_of!(Self, source) as u32,
                    variant_offset: std::mem::offset_of!(Self, variant) as u32,
                }]
            }

            fn resource_demand(&self, demand: &mut BTreeSet<AssetDemandSelection>) {
                if !self.source.is_empty() {
                    AssetDemandSelection::insert_into(
                        demand,
                        crate::services::asset_management::formats::shader::SHADER_TYPE,
                        &self.source,
                        self.variant,
                    );
                }
                self.properties.resource_demand(demand);
            }
        }
    };
}

plot_paint!(
    /// Chart axis stroke paint, independent of grids, text and marks.
    PlotAxisPaint
);

plot_paint!(
    /// Chart panel paint, independent of data marks and axes.
    PlotCalloutPaint
);

/// Exact lifetime of the requested paint on this chart.
pub(in crate::world::systems) fn paint_target(
    world: &crate::world::WorldSimulationState,
    entity: crate::EntityId,
    component: u16,
) -> Option<crate::systems::canvas::CanvasTarget> {
    Some(crate::systems::canvas::CanvasTarget {
        entity,
        component,
        incarnation: world
            .state
            .entities
            .get(&entity)?
            .input(component)?
            .incarnation,
    })
}

pub(in crate::world::systems) fn paint_instance(
    world: &crate::world::WorldSimulationState,
    assets: &crate::services::asset_management::AssetManagementService,
    target: crate::systems::canvas::CanvasTarget,
    previous: Option<&crate::systems::canvas::CanvasPaintInstance>,
) -> Option<crate::systems::canvas::CanvasPaintInstance> {
    if paint_target(world, target.entity, target.component)? != target {
        return None;
    }
    let index = target.entity.index() as usize;
    let (source, variant, values) = match target.component {
        ComponentValue::PLOT_AXIS_PAINT => {
            let paint = world.components.plot_axis_paint(index)?;
            (&paint.source, paint.variant, &paint.properties)
        }
        ComponentValue::PLOT_CALLOUT_PAINT => {
            let paint = world.components.plot_callout_paint(index)?;
            (&paint.source, paint.variant, &paint.properties)
        }
        _ => return None,
    };
    let shader = assets
        .find_source(
            world.id,
            crate::services::asset_management::formats::shader::SHADER_TYPE,
            source,
            variant,
        )
        .filter(|&key| assets.get(key).and_then(|entry| entry.data()).is_some());
    let properties = || {
        values.descriptors().keys().filter_map(|name| {
            let value = values.get(name)?;
            (value.kind() != crate::DynamicPropertyKind::Asset).then(|| (name.clone(), value))
        })
    };
    if let Some(previous) = previous.filter(|previous| {
        previous.target == target
            && previous.source == *source
            && previous.shader == shader
            && properties().eq(previous.properties.iter().cloned())
    }) {
        return Some(previous.clone());
    }
    let properties = properties().collect::<Vec<_>>().into();
    Some(crate::systems::canvas::CanvasPaintInstance {
        target,
        source: source.clone(),
        shader,
        properties,
    })
}
