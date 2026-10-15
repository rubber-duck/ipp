//! Authored callout appearance, independent of data-mark paint and data binding.
use crate::ErrorReason;
use crate::components::schema::{ComponentAssetReference, ComponentLifecycle};
use crate::services::asset_management::AssetDemandSelection;
use ipp_schema_derive::SchemaComponent;
use std::{collections::BTreeSet, sync::Arc};

/// Optional chart-local panel and leader appearance; dimensions scale with the frame font.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, SchemaComponent)]
pub struct PlotCalloutStyle {
    /// Horizontal padding in font-size units.
    pub padding_x: f32,
    /// Vertical padding in font-size units.
    pub padding_y: f32,
    /// Corner radius in font-size units.
    pub radius: f32,
    /// Border width in font-size units; zero delegates edges entirely to the paint.
    pub border_width: f32,
    /// Fallback panel red.
    pub red: f32,
    /// Fallback panel green.
    pub green: f32,
    /// Fallback panel blue.
    pub blue: f32,
    /// Fallback panel alpha.
    pub alpha: f32,
    /// Text red.
    pub text_red: f32,
    /// Text green.
    pub text_green: f32,
    /// Text blue.
    pub text_blue: f32,
    /// Text alpha.
    pub text_alpha: f32,
    /// Connector red.
    pub leader_red: f32,
    /// Connector green.
    pub leader_green: f32,
    /// Connector blue.
    pub leader_blue: f32,
    /// Connector alpha.
    pub leader_alpha: f32,
    /// Connector width in font-size units.
    pub leader_width: f32,
}

impl Default for PlotCalloutStyle {
    fn default() -> Self {
        Self {
            padding_x: 0.65,
            padding_y: 0.4,
            radius: 0.35,
            border_width: 0.0,
            red: 0.025,
            green: 0.04,
            blue: 0.05,
            alpha: 0.94,
            text_red: 0.8,
            text_green: 0.9,
            text_blue: 0.94,
            text_alpha: 1.0,
            leader_red: 0.4,
            leader_green: 0.5,
            leader_blue: 0.55,
            leader_alpha: 0.5,
            leader_width: 0.035,
        }
    }
}

impl ComponentLifecycle for PlotCalloutStyle {
    fn validate(&self) -> Result<(), ErrorReason> {
        if [
            self.padding_x,
            self.padding_y,
            self.radius,
            self.border_width,
            self.leader_width,
        ]
        .iter()
        .any(|v| !v.is_finite() || *v < 0.0)
            || [
                self.red,
                self.green,
                self.blue,
                self.alpha,
                self.text_red,
                self.text_green,
                self.text_blue,
                self.text_alpha,
                self.leader_red,
                self.leader_green,
                self.leader_blue,
                self.leader_alpha,
            ]
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err(ErrorReason::InvalidValue);
        }
        Ok(())
    }
}

/// Independent shared panel paint; property animations never rebuild Plot geometry.
#[repr(C)]
#[derive(Clone, Debug, Default, PartialEq, SchemaComponent)]
pub struct PlotCalloutPaint {
    /// Shared Canvas paint definition.
    pub source: Arc<str>,
    /// Definition variant.
    pub variant: u32,
    /// Ordinary independently editable shader parameters.
    #[schema(ignore)]
    pub properties: crate::DynamicProperties,
}

impl ComponentLifecycle for PlotCalloutPaint {
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

impl PlotCalloutStyle {
    pub(super) fn color(&self) -> [f32; 4] {
        [self.red, self.green, self.blue, self.alpha]
    }

    pub(super) fn text_color(&self) -> [f32; 4] {
        [
            self.text_red,
            self.text_green,
            self.text_blue,
            self.text_alpha,
        ]
    }

    pub(super) fn leader_color(&self) -> [f32; 4] {
        [
            self.leader_red,
            self.leader_green,
            self.leader_blue,
            self.leader_alpha,
        ]
    }
}

/// Native sizing includes every authored line without changing the label text.
pub(super) fn text_extent(text: &str, font: f32) -> [f32; 2] {
    [
        text.lines()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(0) as f32
            * font
            * 0.62,
        text.lines().count().max(1) as f32 * font * 1.2,
    ]
}

/// Exact callout paint lifetime, distinct from the chart's data-mark paint.
pub(in crate::world::systems) fn paint_target(
    world: &crate::world::WorldSimulationState,
    entity: crate::EntityId,
) -> Option<crate::systems::canvas::CanvasTarget> {
    world
        .components
        .plot_callout_paint(entity.index() as usize)?;
    let component = crate::ComponentValue::PLOT_CALLOUT_PAINT;
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
) -> Option<crate::systems::canvas::CanvasPaintInstance> {
    if paint_target(world, target.entity)? != target {
        return None;
    }
    let value = world
        .components
        .plot_callout_paint(target.entity.index() as usize)?;
    let shader = assets
        .find_source(
            world.id,
            crate::services::asset_management::formats::shader::SHADER_TYPE,
            &value.source,
            value.variant,
        )
        .filter(|&key| assets.get(key).and_then(|entry| entry.data()).is_some());
    let properties = value
        .properties
        .descriptors()
        .keys()
        .filter_map(|name| {
            let value = value.properties.get(name)?;
            (value.kind() != crate::DynamicPropertyKind::Asset).then(|| (name.clone(), value))
        })
        .collect::<Vec<_>>()
        .into();
    Some(crate::systems::canvas::CanvasPaintInstance {
        target,
        source: value.source.clone(),
        shader,
        properties,
    })
}
