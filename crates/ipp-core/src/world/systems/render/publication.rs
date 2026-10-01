//! Owned material, palette, particle and light inputs for immutable submissions.

use super::{DebugRenderItem, Light, RenderItem, RenderSystem};
use crate::services::asset_management::{AssetKey, shader::SHADER_TYPE};
use crate::{ComponentValue, DynamicValue, EntityId, ErrorReason, WorldContext};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
/// Resolved property data requiring no live source-name lookup.
pub enum PublishedMaterialProperty {
    /// Owned numeric or scalar value.
    Value(DynamicValue),
    /// Exact decoded resource, or explicitly unavailable selection.
    Resource(Option<AssetKey>),
}

#[derive(Clone, Debug, PartialEq)]
/// Final material policy and resolved inputs for a completed draw.
pub struct PublishedCustomMaterial {
    /// Exact available immutable shader definition.
    pub shader: Option<AssetKey>,
    /// Resolved opaque, cutout or blend policy.
    pub alpha_mode: u32,
    /// Effective cutout threshold.
    pub alpha_cutoff: f32,
    /// Whether standard lighting inputs are requested.
    pub receives_light: bool,
    /// Whether shadow reception is requested.
    pub receives_shadows: bool,
    /// Whether shadow casting is requested.
    pub casts_shadows: bool,
    /// Whether the producer promises bounds enclose custom deformation.
    pub conservative_bounds: bool,
    /// Owned effective named inputs, including exact resource identities.
    pub properties: BTreeMap<std::sync::Arc<str>, PublishedMaterialProperty>,
}

#[derive(Clone, Debug, PartialEq)]
/// Complete non-Surface draw input with no component rereads.
pub struct PublishedRenderItem {
    /// Final mesh, material, placement and per-particle values.
    pub item: RenderItem,
    /// Captured presentation component lifetime.
    pub incarnation: u64,
    /// Owned custom policy and resolved property values.
    pub custom: Option<PublishedCustomMaterial>,
    #[cfg(feature = "skeletal-animation")]
    /// Owned final skin palette, independent of later pose evaluation.
    pub palette: Option<Vec<[f32; 16]>>,
}

#[derive(Clone, Debug, PartialEq)]
/// Owned direct-light input, transformed during domain composition.
pub struct PublishedLight {
    /// World-local light owner.
    pub entity: EntityId,
    /// Captured presentation component lifetime.
    pub incarnation: u64,
    /// Final World-local light placement.
    pub model: [f32; 16],
    /// Effective light parameters.
    pub light: Light,
}

#[derive(Clone, Debug, PartialEq)]
/// Immutable mesh/debug/light inputs; Surface and GUI producers remain separate.
pub struct RenderPublication {
    /// Complete entity-ordered mesh and particle draws.
    pub items: Vec<PublishedRenderItem>,
    /// All valid derived debug parts with individual visibility and color provenance.
    /// Containing-camera policy is deliberately unresolved, including while frozen.
    pub debug: Vec<DebugRenderItem>,
    /// All direct lights for composition after descendants complete.
    pub lights: Vec<PublishedLight>,
    /// Containing-domain settings; spatial descendants do not override them.
    pub state: crate::RenderState,
}

impl RenderSystem {
    pub(super) fn publish(
        &self,
        world: &WorldContext<'_>,
        output: &mut crate::host::WorldOutputBuilder<'_>,
    ) -> Result<(), ErrorReason> {
        let available = |key: AssetKey| {
            world
                .asset_resources()
                .get(key)
                .is_some_and(|provider| provider.data().is_some())
        };
        let mut items = Vec::new();
        for item in &self.state.items {
            let record = &world.world.state.entities[&item.entity];
            let component = ComponentValue::MESH_INSTANCE;
            #[cfg(feature = "particles")]
            let component = if let Some(particle) = item.particle {
                if particle.sprite {
                    ComponentValue::PARTICLE_SPRITE
                } else {
                    ComponentValue::PARTICLE_MESH
                }
            } else {
                component
            };
            let incarnation = record
                .input(component)
                .ok_or(ErrorReason::MissingComponent)?
                .incarnation;
            if item.mesh.asset != 0 {
                output.retain(AssetKey::from_u64(item.mesh.asset));
            }
            if let Some(texture) = item.texture {
                output.retain(AssetKey::from_u64(texture.asset));
            }
            #[cfg(feature = "mesh-poses")]
            if let Some((pose, _)) = item.pose {
                output.retain(AssetKey::from_u64(pose.asset));
            }
            let custom = world.custom_material(item.entity).map(|value| {
                let shader = world
                    .asset_source_key(SHADER_TYPE, &value.source, value.variant)
                    .filter(|key| available(*key));
                if let Some(key) = shader {
                    output.retain(key);
                }
                let properties = value
                    .properties
                    .descriptors()
                    .keys()
                    .filter_map(|name| {
                        let value = value.properties.get(name)?;
                        let property = if let DynamicValue::Asset(source) = value {
                            let key = world
                                .asset_source_key(source.kind, &source.uri, source.variant)
                                .filter(|key| available(*key));
                            if let Some(key) = key {
                                output.retain(key);
                            }
                            PublishedMaterialProperty::Resource(key)
                        } else {
                            PublishedMaterialProperty::Value(value)
                        };
                        Some((name.clone(), property))
                    })
                    .collect();
                PublishedCustomMaterial {
                    shader,
                    alpha_mode: value.alpha_mode,
                    alpha_cutoff: value.alpha_cutoff,
                    receives_light: value.receives_light,
                    receives_shadows: value.receives_shadows,
                    casts_shadows: value.casts_shadows,
                    conservative_bounds: value.conservative_bounds,
                    properties,
                }
            });
            items.push(PublishedRenderItem {
                item: *item,
                incarnation,
                custom,
                #[cfg(feature = "skeletal-animation")]
                palette: world.skin_palette(item.entity).map(<[_]>::to_vec),
            });
        }
        let lights = world
            .light_items()
            .filter_map(|(entity, model, light)| {
                Some(PublishedLight {
                    entity,
                    incarnation: world
                        .world
                        .state
                        .entities
                        .get(&entity)?
                        .input(ComponentValue::LIGHT)?
                        .incarnation,
                    model,
                    light,
                })
            })
            .collect();
        output.chunk(
            Self::ID,
            RenderPublication {
                items,
                debug: self.state.debug_items.clone(),
                lights,
                state: self.state.render_state,
            },
        );
        Ok(())
    }
}
