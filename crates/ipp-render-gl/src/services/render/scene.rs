//! Presentation-local composition of leased, completed World outputs.

use crate::RenderError;
use ipp_core::services::asset_management::{AssetKey, AssetProvider};
use ipp_core::systems::{
    camera::CameraPublication,
    canvas::CanvasPublication,
    geometry::{
        GeometryBounds, GeometryPlane, GeometryPublication, GeometryShapeTransform, GeometrySystem,
        PublishedGeometry,
    },
    render::{PublishedRenderItem, RenderPublication, RenderSystem},
};
use ipp_core::{EntityId, HostRuntime, OutputRef, WorldPublicationId, WorldRef};
use std::collections::BTreeMap;

/// World-qualified presentation lifetime; never a mutable ECS lookup token.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RenderEntity {
    /// Exact producing World lifetime.
    pub world: WorldRef,
    /// Generational entity within the producing World.
    pub entity: EntityId,
    /// Captured presentation component lifetime.
    pub incarnation: u64,
}

#[derive(Clone)]
pub(super) struct SceneOutputSurface {
    pub entity: RenderEntity,
    pub selection: OutputRef,
    pub publication: WorldPublicationId,
    pub model: [f32; 16],
    /// Surface-local to domain placement that `model` renders.
    pub placement: GeometryShapeTransform,
    pub extent: [f32; 2],
    pub token: ipp_core::WorldAttachmentToken,
    pub cache_policy: Option<ipp_core::SurfaceCachePolicy>,
    pub interaction_eligible: bool,
    /// Local Z between consecutive canvas layer plane ids.
    pub layer_spacing: f32,
    /// Local Z of the highest layer in use, its plane id times the spacing;
    /// zero while the canvas's layers share the Surface's plane.
    pub layer_depth: f32,
}

impl SceneOutputSurface {
    /// Whether layer planes leave the Surface's own plane.
    pub fn layered(&self) -> bool {
        self.layer_depth != 0.0
    }
}

/// Current fault eligibility only, not routed-input authorization.
pub(super) fn world_interaction_eligible(host: &HostRuntime, world: WorldRef) -> bool {
    host.world_fault(world) == Ok(None)
}

pub(super) struct SceneItem<'a> {
    pub entity: RenderEntity,
    pub value: ipp_core::RenderItem,
    pub published: &'a PublishedRenderItem,
}

impl std::ops::Deref for SceneItem<'_> {
    type Target = ipp_core::RenderItem;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

pub(super) struct SceneDebug {
    pub entity: RenderEntity,
    pub value: ipp_core::DebugRenderItem,
    pub color: [f32; 3],
}

impl std::ops::Deref for SceneDebug {
    type Target = ipp_core::DebugRenderItem;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

pub(super) struct SceneGeometry<'a> {
    pub value: &'a PublishedGeometry,
    pub placement: GeometryShapeTransform,
}

pub(super) struct RenderScene<'a> {
    pub host: &'a HostRuntime,
    pub selection: OutputRef,
    pub camera: &'a CameraPublication,
    pub state: ipp_core::RenderState,
    pub items: Vec<SceneItem<'a>>,
    pub debug: Vec<SceneDebug>,
    pub lights: Vec<(RenderEntity, [f32; 16], ipp_core::components::Light)>,
    pub geometry: BTreeMap<(WorldRef, EntityId), SceneGeometry<'a>>,
    pub surfaces: Vec<SceneOutputSurface>,
    resources: BTreeMap<AssetKey, WorldPublicationId>,
}

impl<'a> RenderScene<'a> {
    pub fn new(
        host: &'a HostRuntime,
        selection: OutputRef,
        publication: WorldPublicationId,
    ) -> Result<Self, RenderError> {
        let camera = host
            .output(publication, selection)
            .and_then(|chunk| chunk.data::<CameraPublication>())
            .ok_or(RenderError::UnavailableOutput)?;
        let root = host
            .publication(publication)
            .ok_or(RenderError::UnavailableOutput)?;
        let state = root
            .chunk(RenderSystem::ID)
            .and_then(|chunk| chunk.data::<RenderPublication>())
            .map(|render| render.state)
            .unwrap_or_default();
        let mut scene = Self {
            host,
            selection,
            camera,
            state,
            items: Vec::new(),
            debug: Vec::new(),
            lights: Vec::new(),
            geometry: BTreeMap::new(),
            surfaces: Vec::new(),
            resources: BTreeMap::new(),
        };
        for contribution in host
            .spatial_contributions_for_output(publication, selection)
            .map_err(|_| RenderError::UnavailableOutput)?
        {
            let publication = contribution.publication;
            let interaction_eligible = world_interaction_eligible(host, publication.world)
                && contribution
                    .path
                    .iter()
                    .all(|(world, _)| world_interaction_eligible(host, *world));
            for edge in &publication.attachments {
                if edge.mode == ipp_core::WorldAttachmentMode::Spatial
                    || edge
                        .placement_output
                        .is_some_and(|owner| owner != selection)
                {
                    continue;
                }
                let Some(child) = host.attached_publication(edge) else {
                    continue;
                };
                let Some(selection) = edge.output else {
                    continue;
                };
                let Some(extent) = edge.surface_extent else {
                    continue;
                };
                let placement = GeometryShapeTransform::new(edge.placement)
                    .and_then(|placement| placement.then(&contribution.placement))
                    .map_err(|_| RenderError::InvalidTransform)?;
                let model = placement
                    .render_matrix()
                    .map_err(|_| RenderError::InvalidTransform)?;
                // Layer planes leave the Surface's plane only for a canvas
                // using a layer above the base under a nonzero spacing.
                let deepest = (selection.kind() == ipp_core::OutputKind::Canvas
                    && edge.layer_spacing != 0.0)
                    .then(|| host.output(child.id, selection))
                    .flatten()
                    .and_then(|chunk| chunk.data::<CanvasPublication>())
                    .and_then(|canvas| canvas.layers.last().copied())
                    .unwrap_or(0);
                scene.surfaces.push(SceneOutputSurface {
                    entity: RenderEntity {
                        world: publication.world,
                        entity: edge.anchor,
                        incarnation: match selection.target() {
                            ipp_core::OutputTarget::Camera {
                                incarnation,
                                ..
                            } => incarnation,
                            ipp_core::OutputTarget::Canvas => selection.world().incarnation(),
                        },
                    },
                    selection,
                    publication: child.id,
                    model,
                    placement,
                    extent: extent.map(|value| value as f32),
                    token: edge.token.clone(),
                    cache_policy: edge.surface_cache_policy,
                    interaction_eligible,
                    layer_spacing: edge.layer_spacing,
                    layer_depth: (f64::from(deepest) * f64::from(edge.layer_spacing)) as f32,
                });
            }
            for key in publication.resources() {
                scene.resources.insert(key, publication.id);
            }
            if let Some(geometry) = publication
                .chunk(GeometrySystem::ID)
                .and_then(|chunk| chunk.data::<GeometryPublication>())
            {
                for value in &geometry.entities {
                    scene.geometry.insert(
                        (publication.world, value.entity),
                        SceneGeometry {
                            value,
                            placement: contribution.placement,
                        },
                    );
                }
            }
            let Some(render) = publication
                .chunk(RenderSystem::ID)
                .and_then(|chunk| chunk.data::<RenderPublication>())
            else {
                continue;
            };
            for published in &render.items {
                let mut value = published.item;
                value.model = compose_model(value.model, contribution.placement)?;
                value.normal = compose_normal(value.normal, contribution.placement);
                if let Some(particle) = &mut value.particle {
                    particle.velocity = contribution
                        .placement
                        .vector(particle.velocity.map(f64::from))
                        .map(|value| value as f32);
                }
                scene.items.push(SceneItem {
                    entity: RenderEntity {
                        world: publication.world,
                        entity: value.entity,
                        incarnation: published.incarnation,
                    },
                    value,
                    published,
                });
            }
            for item in &render.debug {
                if !item.is_rendered && !scene.state.show_all_debug_geometries {
                    continue;
                }
                let mut value = *item;
                value.model = compose_model(value.model, contribution.placement)?;
                scene.debug.push(SceneDebug {
                    entity: RenderEntity {
                        world: publication.world,
                        entity: value.entity,
                        incarnation: 0,
                    },
                    value,
                    color: item
                        .color_override
                        .unwrap_or(scene.state.debug_geometry_color),
                });
            }
            for light in &render.lights {
                scene.lights.push((
                    RenderEntity {
                        world: publication.world,
                        entity: light.entity,
                        incarnation: light.incarnation,
                    },
                    compose_model(light.model, contribution.placement)?,
                    light.light,
                ));
            }
        }
        scene.items.sort_by_key(|item| item.entity);
        scene.lights.sort_by_key(|light| light.0);
        Ok(scene)
    }

    pub fn resource(&self, key: AssetKey) -> Option<&'a AssetProvider> {
        self.host
            .publication_resource(*self.resources.get(&key)?, key)
    }

    pub fn mesh_metadata(
        &self,
        key: ipp_core::MeshKey,
    ) -> Option<&'a ipp_core::services::asset_management::mesh_metadata::MeshMetadata> {
        self.resource(AssetKey::from_u64(key.asset))?
            .data()?
            .metadata()
            .downcast_ref()
    }

    pub fn visible(&self, entity: RenderEntity, planes: &[GeometryPlane; 6]) -> bool {
        self.geometry
            .get(&(entity.world, entity.entity))
            .is_none_or(|geometry| {
                geometry.value.culling.as_ref().is_none_or(|shape| {
                    let local = planes.map(|plane| geometry.placement.local_plane(&plane));
                    shape.intersects_frustum(&local)
                })
            })
    }

    /// Whether a Surface may be visible: its published culling geometry, grown
    /// along its normal to the deepest layer plane while its canvas's layers
    /// separate. The parent World's bounds cannot see the child's layers, so the
    /// growth applies here, where both are known.
    pub fn surface_visible(
        &self,
        surface: &SceneOutputSurface,
        planes: &[GeometryPlane; 6],
    ) -> bool {
        use ipp_core::systems::geometry::{GeometryBounds, GeometryShape};

        if self.visible(surface.entity, planes) {
            return true;
        }
        if !surface.layered() {
            return false;
        }
        let [width, height] = surface.extent.map(|value| f64::from(value) * 0.5);
        let depth = f64::from(surface.layer_depth);
        let layers = GeometryShape::Box {
            min: [-width, -height, depth.min(0.0)],
            max: [width, height, depth.max(0.0)],
        };
        layers.intersects_frustum(&planes.map(|plane| surface.placement.local_plane(&plane)))
    }

    pub fn visual_bounds(&self, entity: RenderEntity) -> Option<[[f64; 3]; 2]> {
        let geometry = self.geometry.get(&(entity.world, entity.entity))?;
        let bounds = geometry.value.visual_bounds?;
        let matrix = geometry.placement.matrix();
        Some(std::array::from_fn(|end| {
            std::array::from_fn(|axis| {
                let mut value = matrix[12 + axis];
                for coordinate in 0..3 {
                    let lower = bounds[0][coordinate] * matrix[coordinate * 4 + axis];
                    let upper = bounds[1][coordinate] * matrix[coordinate * 4 + axis];
                    value += if end == 0 {
                        lower.min(upper)
                    } else {
                        lower.max(upper)
                    };
                }
                value
            })
        }))
    }
}

fn compose_model(
    model: [f32; 16],
    placement: GeometryShapeTransform,
) -> Result<[f32; 16], RenderError> {
    let outer = placement.matrix();
    let result = std::array::from_fn(|index| {
        let column = index / 4;
        let row = index % 4;
        (0..4)
            .map(|coordinate| {
                outer[coordinate * 4 + row] * f64::from(model[column * 4 + coordinate])
            })
            .sum::<f64>() as f32
    });
    if result.iter().all(|value| value.is_finite()) {
        Ok(result)
    } else {
        Err(RenderError::InvalidTransform)
    }
}

#[derive(Default)]
pub(super) struct SceneVisibility {
    matches: std::collections::BTreeSet<(RenderEntity, usize)>,
}

impl SceneVisibility {
    pub fn prepare(
        &mut self,
        scene: &RenderScene<'_>,
        items: &[SceneItem<'_>],
        frustums: &[[GeometryPlane; 6]],
    ) {
        self.matches.clear();
        for item in items {
            for (index, frustum) in frustums.iter().enumerate() {
                if scene.visible(item.entity, frustum) {
                    self.matches.insert((item.entity, index));
                }
            }
        }
    }

    pub fn matches(&self, entity: RenderEntity, index: usize) -> bool {
        self.matches.contains(&(entity, index))
    }
}

fn compose_normal(
    normal: Result<[f32; 16], ipp_core::ErrorReason>,
    placement: GeometryShapeTransform,
) -> Result<[f32; 16], ipp_core::ErrorReason> {
    let normal = normal?;
    let inverse = placement.inverse_matrix();
    let mut result = [0.0_f64; 16];
    for column in 0..3 {
        for row in 0..3 {
            result[column * 4 + row] = (0..3)
                .map(|coordinate| {
                    inverse[row * 4 + coordinate] * f64::from(normal[column * 4 + coordinate])
                })
                .sum();
        }
    }
    let scale = result.iter().copied().map(f64::abs).fold(0.0_f64, f64::max);
    if scale == 0.0 || !scale.is_finite() {
        return Err(ipp_core::ErrorReason::InvalidValue);
    }
    Ok(result.map(|value| (value / scale) as f32))
}

#[cfg(test)]
pub(super) fn test_entity(bits: u64) -> RenderEntity {
    static WORLD: std::sync::OnceLock<WorldRef> = std::sync::OnceLock::new();
    let world = *WORLD.get_or_init(|| {
        let mut host = HostRuntime::new();
        let world = host.create_world(Default::default(), &[]).unwrap();
        host.world_ref(world).unwrap()
    });
    RenderEntity {
        world,
        entity: EntityId::from_bits(bits),
        incarnation: 1,
    }
}

#[cfg(test)]
#[path = "scene_tests.rs"]
mod tests;
