//! One evaluation path for the distinct bounding and picking components.

use super::{GeometrySystem, GeometrySystemState};
use crate::services::asset_management::AssetManagementService;
use crate::world::systems::asset_dependencies::{cached_source_key, source_key_from_fields};
use crate::world::{WorldSimulationState, systems::SystemRuntimeAccess};
use crate::{
    EntityId, ErrorReason,
    services::asset_management::AssetKey,
    services::asset_management::service::AssetDemandSelection,
    systems::camera,
    systems::geometry::{
        CompoundGeometryShape, GEOMETRY_TYPE, GeometryBounds, GeometryDefinition, GeometryShape,
        GeometryShapeTransform, TransformedGeometryShape,
    },
};

#[cfg(feature = "skeletal-animation")]
use crate::ComponentValue;

struct GeometryEvaluationInput<'a> {
    geometry: &'a [u8],
    source: &'a str,
    variant: u32,
    #[cfg(feature = "skeletal-animation")]
    skeleton: EntityId,
}

#[derive(Debug)]
pub(crate) struct GeometryEvaluationState {
    last_model: Option<[f64; 16]>,
    changed: bool,
    pub(super) enclosure: Option<super::GeometryEnclosure>,
    pub(super) visual: Option<super::GeometryEnclosure>,
    pub(super) culling_valid: bool,
    pub(super) program: Option<super::program::GeometryProgram>,
    inline: Option<GeometryDefinition>,
    source_key: Option<AssetKey>,
    #[cfg(feature = "skeletal-animation")]
    binding: Option<(EntityId, u64, AssetKey)>,
    #[cfg(feature = "skeletal-animation")]
    invalidated: bool,
    pub(in crate::world) evaluated: Result<CompoundGeometryShape, ErrorReason>,
}

impl GeometryEvaluationState {
    /// Reuse the rigid program's initialized shape and update its derived bounds
    /// without moving the compound through the general evaluator. Lifecycle
    /// invalidation clears the program, so preparation still owns all rebinding.
    fn update_rigid(&mut self, storage: &crate::components::registry::ComponentStorage) -> bool {
        #[cfg(feature = "skeletal-animation")]
        if self.invalidated {
            return false;
        }
        let Some(super::program::GeometryProgram::Rigid {
            shape: GeometryShape::Box {
                min,
                max,
            },
            model,
        }) = &self.program
        else {
            return false;
        };
        let Ok(evaluated) = &mut self.evaluated else {
            return false;
        };
        let [part] = evaluated.parts.as_mut_slice() else {
            return false;
        };
        let Some(Ok(model)) = model.borrow(storage) else {
            return false;
        };
        let matrix = model.matrix_ref();
        self.changed = self.last_model.as_ref() != Some(matrix);
        if !self.changed {
            return true;
        }

        self.last_model = Some(*matrix);
        // Picking and debug rendering still consume the exact oriented shape.
        // Keep its retained slot current instead of clearing/reinserting a part.
        part.transform = *model;
        self.enclosure = super::GeometryEnclosure::transformed_box(*min, *max, matrix);
        self.visual = self.enclosure;
        self.culling_valid = true;
        true
    }

    pub(in crate::world) fn evaluated(&self) -> Result<&CompoundGeometryShape, ErrorReason> {
        #[cfg(feature = "skeletal-animation")]
        if self.invalidated {
            return Err(ErrorReason::InvalidGeometry);
        }
        self.evaluated.as_ref().map_err(|error| *error)
    }
}

impl Default for GeometryEvaluationState {
    fn default() -> Self {
        Self {
            last_model: None,
            changed: true,
            enclosure: None,
            visual: None,
            culling_valid: false,
            program: None,
            inline: None,
            source_key: None,
            #[cfg(feature = "skeletal-animation")]
            binding: None,
            #[cfg(feature = "skeletal-animation")]
            invalidated: false,
            evaluated: Err(ErrorReason::GeometryUnavailable),
        }
    }
}

#[cfg(test)]
#[path = "rigid_bounds_tests.rs"]
mod rigid_bounds_tests;

pub(in crate::world) struct GeometryReadAccess<'a> {
    pub(in crate::world) world: &'a WorldSimulationState,
    pub(in crate::world) assets: &'a AssetManagementService,
    state: &'a GeometrySystemState,
}

impl GeometrySystem {
    /// Refresh each component's own geometry after finalized poses and asset loads.
    pub(in crate::world) fn evaluate(&mut self, runtime: &mut SystemRuntimeAccess<'_>) {
        self.state.bounds.prepare(
            runtime.world,
            crate::components::registry::ComponentStorage::bounding_geometry_ptr,
        );
        self.state.picking.prepare(
            runtime.world,
            crate::components::registry::ComponentStorage::picking_geometry_ptr,
        );
        macro_rules! evaluate {
            ($query:ident, $index:ident, $visual:expr) => {
                for &(entity, binding) in self.state.$query.entries() {
                    let value = binding.get_mut(&mut runtime.world.components);
                    let mut state = value.runtime.evaluation.take().unwrap_or_default();
                    if state.update_rigid(&runtime.world.components) {
                        if state.changed {
                            self.state.$index.publish(super::GeometryPreparedBounds {
                                entity,
                                visual: state.visual,
                                culling: state.enclosure,
                            });
                        }
                        binding
                            .get_mut(&mut runtime.world.components)
                            .runtime
                            .evaluation = Some(state);
                        continue;
                    }

                    let previous = std::mem::replace(
                        &mut state.evaluated,
                        Err(ErrorReason::GeometryUnavailable),
                    );
                    let shape = previous.unwrap_or_default();
                    let value = binding.get(&runtime.world.components);
                    let input = GeometryEvaluationInput {
                        geometry: &value.geometry,
                        source: &value.source,
                        variant: value.variant,
                        #[cfg(feature = "skeletal-animation")]
                        skeleton: value.skeleton,
                    };
                    state.evaluated = GeometryReadAccess::new(
                        runtime.world,
                        runtime.asset_acquisition,
                        &self.state,
                    )
                    .evaluate_geometry_input(entity, &input, &mut state, shape);
                    if state.changed {
                        state.enclosure = state
                            .evaluated
                            .as_ref()
                            .ok()
                            .and_then(GeometryBounds::bounds)
                            .and_then(super::GeometryEnclosure::new);
                    }
                    let generated = input.geometry.is_empty() && input.source.is_empty();
                    let visual_bounds = if generated || !$visual {
                        state.enclosure.map(|value| value.bounds)
                    } else {
                        GeometryReadAccess::new(
                            runtime.world,
                            runtime.asset_acquisition,
                            &self.state,
                        )
                        .visual_bounds(entity)
                        .ok()
                        .flatten()
                    };
                    let visual_changed = state.visual.map(|value| value.bounds) != visual_bounds;
                    if generated || !$visual {
                        state.visual = state.enclosure;
                    } else if visual_changed {
                        state.visual = visual_bounds.and_then(super::GeometryEnclosure::new);
                    }
                    if state.changed || visual_changed {
                        state.culling_valid = (generated || !$visual)
                            || state.evaluated.as_ref().ok().is_some_and(|shape| {
                                visual_bounds.is_some_and(|bounds| shape.encloses_box(bounds))
                            });
                    }
                    let published = super::GeometryPreparedBounds {
                        entity,
                        visual: state.visual,
                        culling: state.culling_valid.then_some(state.enclosure).flatten(),
                    };
                    binding
                        .get_mut(&mut runtime.world.components)
                        .runtime
                        .evaluation = Some(state);
                    self.state.$index.publish(published);
                }
            };
        }
        evaluate!(bounds, spatial_bounds, true);
        evaluate!(picking, spatial_picking, false);
        self.state.spatial_bounds.finish();
        self.state.spatial_picking.finish();
        self.programs_ready = true;
    }
}

impl<'a> GeometryReadAccess<'a> {
    pub(in crate::world) fn new(
        world: &'a WorldSimulationState,
        assets: &'a AssetManagementService,
        state: &'a GeometrySystemState,
    ) -> Self {
        Self {
            world,
            assets,
            state,
        }
    }

    fn compile_program(
        &self,
        entity: EntityId,
        input: &GeometryEvaluationInput<'_>,
    ) -> Result<super::program::GeometryProgram, ErrorReason> {
        use super::program::GeometryProgram;
        // SAFETY: GeometrySystem clears every compiled program before component
        // destruction/replacement, optional membership changes, and asset release.
        // Access is scoped to this World's storage during geometry evaluation.
        let model = unsafe {
            crate::systems::hierarchy::ObjectTransformBinding::bind(&self.world.components, entity)
        };
        if input.geometry.is_empty() && input.source.is_empty() {
            let index = entity.index() as usize;
            #[cfg(feature = "particles")]
            if self.world.components.particle_sprite(index).is_some()
                || self.world.components.particle_mesh(index).is_some()
            {
                return Ok(GeometryProgram::Dynamic);
            }
            #[cfg(feature = "skeletal-animation")]
            if self.world.components.skin(index).is_some() {
                return Ok(GeometryProgram::Dynamic);
            }
            #[cfg(feature = "mesh-poses")]
            if self.world.components.mesh_pose(index).is_some() {
                return Ok(GeometryProgram::Dynamic);
            }
            #[cfg(feature = "surfaces")]
            if self.world.components.surface(index).is_some() {
                return Ok(GeometryProgram::Dynamic);
            }
            let mesh = self
                .world
                .components
                .mesh_instance(index)
                .ok_or(ErrorReason::GeometryUnavailable)?;
            let key = source_key_from_fields(
                self.assets,
                self.world.id,
                None,
                crate::MESH_TYPE,
                &mesh.source,
                mesh.variant,
            )
            .ok_or(ErrorReason::GeometryUnavailable)?;
            let (min, max) = self
                .assets
                .get_typed::<crate::services::asset_management::mesh_metadata::MeshMetadata>(key)
                .ok_or(ErrorReason::GeometryUnavailable)?
                .bounds();
            return Ok(GeometryProgram::Rigid {
                shape: GeometryShape::Box {
                    min: min.map(f64::from),
                    max: max.map(f64::from),
                },
                model,
            });
        }
        let inline;
        let definition = if !input.geometry.is_empty() {
            inline = GeometryDefinition::decode(input.geometry)?;
            &inline
        } else {
            let key = source_key_from_fields(
                self.assets,
                self.world.id,
                None,
                GEOMETRY_TYPE,
                input.source,
                input.variant,
            )
            .ok_or(ErrorReason::GeometryUnavailable)?;
            self.assets
                .get_typed::<GeometryDefinition>(key)
                .ok_or(ErrorReason::GeometryUnavailable)?
        };
        if definition.parts.iter().any(|part| part.joints.is_some()) {
            return Ok(GeometryProgram::Dynamic);
        }
        let parts = definition
            .parts
            .iter()
            .map(|part| {
                Ok(TransformedGeometryShape {
                    shape: part.shape,
                    transform: GeometryShapeTransform::from_matrix(camera::model_matrix(
                        &part.transform,
                    )?)?,
                })
            })
            .collect::<Result<Vec<_>, ErrorReason>>()?;
        Ok(GeometryProgram::Parts {
            parts,
            model,
        })
    }

    fn evaluate_geometry_input(
        &self,
        entity: EntityId,
        input: &GeometryEvaluationInput<'_>,
        state: &mut GeometryEvaluationState,
        mut evaluated: CompoundGeometryShape,
    ) -> Result<CompoundGeometryShape, ErrorReason> {
        state.changed = true;
        #[cfg(feature = "skeletal-animation")]
        if state.invalidated {
            return Err(ErrorReason::InvalidGeometry);
        }
        if state.program.is_none() {
            state.last_model = None;
            state.program = Some(
                self.compile_program(entity, input)
                    .unwrap_or(super::program::GeometryProgram::Dynamic),
            );
        }
        let model = state
            .program
            .as_ref()
            .unwrap()
            .model(&self.world.components)
            .transpose()?;
        let matrix = model.as_ref().map(GeometryShapeTransform::matrix);
        if matrix.is_some() && matrix == state.last_model && !evaluated.parts.is_empty() {
            state.changed = false;
            return Ok(evaluated);
        }
        state.changed = true;
        state.last_model = matrix;
        if let Some(model) = model {
            state
                .program
                .as_ref()
                .unwrap()
                .evaluate(&model, &mut evaluated)?;
            return Ok(evaluated);
        }
        evaluated.parts.clear();
        let definition = if !input.geometry.is_empty() {
            if state.inline.is_none() {
                state.inline = Some(GeometryDefinition::decode(input.geometry)?);
            }
            state.inline.as_ref().unwrap()
        } else if !input.source.is_empty() {
            state.source_key = source_key_from_fields(
                self.assets,
                self.world.id,
                state.source_key,
                GEOMETRY_TYPE,
                input.source,
                input.variant,
            );
            self.assets
                .get_typed::<GeometryDefinition>(
                    state.source_key.ok_or(ErrorReason::GeometryUnavailable)?,
                )
                .ok_or(ErrorReason::GeometryUnavailable)?
        } else {
            return self.mesh_bounding_geometry_into(entity, evaluated);
        };
        let model = self.geometry_model(entity)?;
        let parts = &mut evaluated.parts;
        parts.reserve(definition.parts.len());
        for part in &definition.parts {
            let local =
                GeometryShapeTransform::from_matrix(camera::model_matrix(&part.transform)?)?;
            if let Some(joints) = part.joints {
                #[cfg(feature = "skeletal-animation")]
                {
                    let skeleton = if input.skeleton.to_bits() == 0 {
                        self.world
                            .components
                            .skin(entity.index() as usize)
                            .map_or(entity, |skin| skin.skeleton)
                    } else {
                        input.skeleton
                    };
                    let pose =
                        crate::systems::skeleton::pose(self.world, &self.world.state, skeleton)
                            .ok_or(ErrorReason::GeometryUnavailable)?;
                    let incarnation = self
                        .world
                        .state
                        .entities
                        .get(&skeleton)
                        .and_then(|record| record.input(ComponentValue::SKELETON))
                        .ok_or(ErrorReason::GeometryUnavailable)?
                        .incarnation;
                    let binding = (skeleton, incarnation, pose.source);
                    if state.binding.is_some_and(|previous| previous != binding) {
                        return Err(ErrorReason::InvalidGeometry);
                    }
                    state.binding = Some(binding);
                    let world = self.geometry_model(skeleton)?;
                    let mut transforms = [GeometryShapeTransform::default(); 2];
                    for endpoint in 0..2 {
                        let matrix = *pose
                            .global
                            .get(joints[endpoint] as usize)
                            .ok_or(ErrorReason::InvalidGeometry)?;
                        transforms[endpoint] =
                            GeometryShapeTransform::from_matrix(matrix)?.then(&world)?;
                    }
                    let GeometryShape::Pill {
                        radius,
                        ..
                    } = part.shape
                    else {
                        return Err(ErrorReason::InvalidGeometry);
                    };
                    // Endpoint positions include full hierarchy and world transforms.
                    // A conservative radius keeps the joint-mapped shape spherical
                    // under independent nonuniform scaling/shear of its endpoints.
                    let radius = radius
                        * local.maximum_stretch()
                        * transforms[0]
                            .maximum_stretch()
                            .max(transforms[1].maximum_stretch());
                    parts.push(TransformedGeometryShape {
                        shape: GeometryShape::Pill {
                            start: transforms[0].point(local.point([0.0; 3])),
                            end: transforms[1].point(local.point([0.0; 3])),
                            radius,
                        },
                        transform: GeometryShapeTransform::default(),
                    });
                }
                #[cfg(not(feature = "skeletal-animation"))]
                {
                    let _ = joints;
                    return Err(ErrorReason::InvalidGeometry);
                }
            } else {
                parts.push(TransformedGeometryShape {
                    shape: part.shape,
                    transform: local.then(&model)?,
                });
            }
        }
        Ok(evaluated)
    }

    pub(in crate::world) fn geometry_model(
        &self,
        entity: EntityId,
    ) -> Result<GeometryShapeTransform, ErrorReason> {
        crate::systems::hierarchy::evaluated_affine(self.world, entity)
    }

    /// Asset-generated enclosure. Skinning combines all nonzero influence bounds
    /// into one convex box, which also encloses their normalized linear blends.
    pub fn mesh_bounding_geometry(
        &self,
        entity: EntityId,
    ) -> Result<CompoundGeometryShape, ErrorReason> {
        self.mesh_bounding_geometry_into(entity, CompoundGeometryShape::default())
    }

    fn mesh_bounding_geometry_into(
        &self,
        entity: EntityId,
        mut evaluated: CompoundGeometryShape,
    ) -> Result<CompoundGeometryShape, ErrorReason> {
        evaluated.parts.extend(self.visual_geometry(entity)?);
        Ok(evaluated)
    }

    /// Derive the visual enclosure without allocating a temporary compound shape.
    fn visual_bounds(&self, entity: EntityId) -> Result<Option<[[f64; 3]; 2]>, ErrorReason> {
        let mut parts = self.visual_geometry(entity)?;
        let bounds = parts.next().and_then(|part| part.bounds());
        Ok(bounds.and_then(|bounds| {
            parts.try_fold(bounds, |mut bounds, part| {
                let next = part.bounds()?;
                for axis in 0..3 {
                    bounds[0][axis] = bounds[0][axis].min(next[0][axis]);
                    bounds[1][axis] = bounds[1][axis].max(next[1][axis]);
                }
                Some(bounds)
            })
        }))
    }

    fn visual_geometry(
        &self,
        entity: EntityId,
    ) -> Result<impl Iterator<Item = TransformedGeometryShape>, ErrorReason> {
        let index = entity.index() as usize;
        #[cfg(not(feature = "surfaces"))]
        let surface = None;
        #[cfg(feature = "surfaces")]
        let surface = if let Some(surface) = self.world.components.surface(index) {
            Some(TransformedGeometryShape {
                shape: surface.local_bounding_geometry(),
                transform: self.geometry_model(entity)?,
            })
        } else {
            None
        };
        let has_mesh = self.world.components.mesh_instance(index).is_some();
        #[cfg(feature = "particles")]
        let has_mesh = has_mesh
            || self.world.components.particle_sprite(index).is_some()
            || self.world.components.particle_mesh(index).is_some();
        let mesh = has_mesh.then(|| self.mesh_enclosure(entity)).transpose()?;
        if surface.is_none() && mesh.is_none() {
            return Err(ErrorReason::GeometryUnavailable);
        }
        Ok([surface, mesh].into_iter().flatten())
    }

    fn mesh_enclosure(&self, entity: EntityId) -> Result<TransformedGeometryShape, ErrorReason> {
        let index = entity.index() as usize;
        #[cfg(feature = "particles")]
        if self.world.components.particle_sprite(index).is_some()
            || self.world.components.particle_mesh(index).is_some()
        {
            return self.particle_enclosure(entity);
        }
        let value = self
            .world
            .components
            .mesh_instance(index)
            .ok_or(ErrorReason::GeometryUnavailable)?;
        let key = cached_source_key(
            self.assets,
            self.world.id,
            &self.state.mesh_keys,
            entity,
            crate::MESH_TYPE,
            &value.source,
            value.variant,
        )
        .ok_or(ErrorReason::GeometryUnavailable)?;
        let mesh = self
            .assets
            .get_typed::<crate::services::asset_management::mesh_metadata::MeshMetadata>(key)
            .ok_or(ErrorReason::GeometryUnavailable)?;
        let model = self.geometry_model(entity)?;
        #[cfg(feature = "mesh-poses")]
        let pose = crate::systems::render::mesh_pose(
            self.world,
            self.assets,
            &self.state.mesh_keys,
            &self.state.pose_mesh_keys,
            entity,
        )?
        .map(|(key, weight)| {
            (
                self.assets
                    .get_typed::<crate::services::asset_management::mesh_metadata::MeshMetadata>(
                        AssetKey::from_u64(key.asset),
                    )
                    .expect("resolved pose mesh")
                    .bounds(),
                weight,
            )
        });
        #[cfg(feature = "mesh-poses")]
        let blend_bounds = |local: [[f32; 3]; 2]| -> [[f64; 3]; 2] {
            let local = local.map(|point| point.map(f64::from));
            let Some(((min, max), weight)) = pose else {
                return local;
            };
            let target = [min, max];
            std::array::from_fn(|end| {
                std::array::from_fn(|axis| {
                    local[end][axis] * (1.0 - f64::from(weight))
                        + f64::from(target[end][axis]) * f64::from(weight)
                })
            })
        };
        #[cfg(feature = "skeletal-animation")]
        if self.world.components.skin(index).is_some() {
            let palette = crate::systems::skinning::palette(self.world, entity)
                .ok_or(ErrorReason::GeometryUnavailable)?;
            let mut bounds: Option<[[f64; 3]; 2]> = None;
            for (joint, local) in mesh.joint_bounds().iter().enumerate() {
                let Some(local) = local else {
                    continue;
                };
                let transform = GeometryShapeTransform::from_matrix(
                    *palette.get(joint).ok_or(ErrorReason::InvalidGeometry)?,
                )?
                .then(&model)?;
                // Every target vertex lies in the target enclosure, including
                // vertices influenced by this base-mesh palette slot. Blending
                // its box with this slot's base box is conservative without
                // requiring the target pose to duplicate joint streams.
                #[cfg(feature = "mesh-poses")]
                let local = blend_bounds(*local);
                #[cfg(not(feature = "mesh-poses"))]
                let local = local.map(|point| point.map(f64::from));
                let shape = TransformedGeometryShape {
                    shape: GeometryShape::Box {
                        min: local[0],
                        max: local[1],
                    },
                    transform,
                };
                let next = shape.bounds().unwrap();
                if let Some(previous) = &mut bounds {
                    for axis in 0..3 {
                        previous[0][axis] = previous[0][axis].min(next[0][axis]);
                        previous[1][axis] = previous[1][axis].max(next[1][axis]);
                    }
                } else {
                    bounds = Some(next);
                }
            }
            let [min, max] = bounds.ok_or(ErrorReason::InvalidGeometry)?;
            return Ok(TransformedGeometryShape {
                shape: GeometryShape::Box {
                    min,
                    max,
                },
                transform: GeometryShapeTransform::default(),
            });
        }
        let (min, max) = mesh.bounds();
        #[cfg(feature = "mesh-poses")]
        let [min, max] = blend_bounds([min, max]);
        #[cfg(not(feature = "mesh-poses"))]
        let (min, max) = (min.map(f64::from), max.map(f64::from));
        Ok(TransformedGeometryShape {
            shape: GeometryShape::Box {
                min,
                max,
            },
            transform: model,
        })
    }

    /// Final evaluated enclosure, never substituted from the picking component.
    pub fn bounding_geometry(
        &self,
        entity: EntityId,
    ) -> Result<&'a CompoundGeometryShape, ErrorReason> {
        if !self.world.state.entities.contains_key(&entity) {
            return Err(ErrorReason::MissingComponent);
        }
        self.world
            .components
            .bounding_geometry(entity.index() as usize)
            .and_then(|value| value.runtime.evaluation.as_ref())
            .ok_or(ErrorReason::MissingComponent)?
            .evaluated()
    }

    /// Borrow the evaluated enclosure once for all frusta in a synchronous submission.
    /// Unproven authored enclosures must not reject geometry in any pass.
    pub fn culling_geometry(&self, entity: EntityId) -> Option<&'a CompoundGeometryShape> {
        if !self.world.state.entities.contains_key(&entity) {
            return None;
        }
        let value = self
            .world
            .components
            .bounding_geometry(entity.index() as usize)?;
        let evaluated = value.runtime.evaluation.as_ref()?;
        evaluated
            .culling_valid
            .then(|| evaluated.evaluated().ok())
            .flatten()
    }

    pub fn geometry_visible(
        &self,
        entity: EntityId,
        planes: &[crate::systems::geometry::GeometryPlane; 6],
    ) -> bool {
        self.culling_geometry(entity)
            .is_none_or(|shape| shape.intersects_frustum(planes))
    }

    /// Final evaluated picking union, using the same transforms as visualization.
    pub fn picking_geometry(
        &self,
        entity: EntityId,
    ) -> Result<&'a CompoundGeometryShape, ErrorReason> {
        if !self.world.state.entities.contains_key(&entity) {
            return Err(ErrorReason::MissingComponent);
        }
        self.world
            .components
            .picking_geometry(entity.index() as usize)
            .and_then(|value| value.runtime.evaluation.as_ref())
            .ok_or(ErrorReason::MissingComponent)?
            .evaluated()
    }
}

// Collect CPU demand at the world boundary; the Host polls the union once.
pub(in crate::world) fn evaluation_mesh_demand(
    world: &WorldSimulationState,
    authored: &crate::world::WorldEntityState,
) -> std::collections::BTreeSet<AssetDemandSelection> {
    let mut demand = std::collections::BTreeSet::new();
    visit_evaluation_meshes(world, authored, |source, variant| {
        demand.insert(AssetDemandSelection::new(crate::MESH_TYPE, source, variant));
    });
    demand
}

/// Refresh source membership in O(entities × log(distinct sources)), allocating
/// only for new selections. Reused mesh instances do not copy their shared URI.
pub(in crate::world) fn update_evaluation_mesh_demand(
    world: &WorldSimulationState,
    authored: &crate::world::WorldEntityState,
    demand: &mut std::collections::BTreeMap<AssetDemandSelection, bool>,
) -> bool {
    for seen in demand.values_mut() {
        *seen = false;
    }
    let mut changed = false;
    visit_evaluation_meshes(world, authored, |source, variant| {
        changed |= AssetDemandSelection::mark_selected(demand, crate::MESH_TYPE, source, variant);
    });
    let before = demand.len();
    demand.retain(|_, seen| *seen);
    changed || demand.len() != before
}

fn visit_evaluation_meshes(
    world: &WorldSimulationState,
    authored: &crate::world::WorldEntityState,
    mut visit: impl FnMut(&str, u32),
) {
    for &entity in authored.entities.keys() {
        let index = entity.index() as usize;
        let needs_mesh = world.components.bounding_geometry(index).is_some();
        let needs_mesh = needs_mesh
            || world
                .components
                .picking_geometry(index)
                .is_some_and(|value| value.geometry.is_empty() && value.source.is_empty());
        if needs_mesh && let Some(mesh) = world.components.mesh_instance(index) {
            visit(&mesh.source, mesh.variant);
        }
        #[cfg(feature = "particles")]
        if needs_mesh && let Some(mesh) = world.components.particle_mesh(index) {
            visit(&mesh.source, mesh.variant);
        }
        #[cfg(feature = "mesh-poses")]
        if needs_mesh
            && let Some(pose) = world.components.mesh_pose(index)
            && !pose.source.is_empty()
        {
            visit(&pose.source, pose.variant);
        }
    }
}

impl crate::WorldContext<'_> {
    /// Spatial queries of finalized bounding geometry, including explicit unknown candidates.
    pub fn geometry_spatial_index(&self) -> &super::GeometrySpatialIndex {
        &self
            .system::<GeometrySystem>(GeometrySystem::ID)
            .expect("World requires GeometrySystem")
            .state
            .spatial_bounds
    }

    /// Independent interaction geometry; never substitute visual bounds for picking shapes.
    pub fn picking_spatial_index(&self) -> &super::GeometrySpatialIndex {
        &self
            .system::<GeometrySystem>(GeometrySystem::ID)
            .expect("World requires GeometrySystem")
            .state
            .spatial_picking
    }

    /// Rebuild acceleration outside active queries without changing component identities.
    pub fn set_geometry_spatial_backend(&mut self, backend: super::GeometrySpatialBackend) {
        self.with_system::<GeometrySystem, _>(GeometrySystem::ID, |system, _| {
            system.state.spatial_bounds.set_backend(backend);
            system.state.spatial_picking.set_backend(backend);
            system.state.spatial_bounds.finish();
            system.state.spatial_picking.finish();
        });
    }

    pub(in crate::world) fn geometry_read(&self) -> GeometryReadAccess<'_> {
        GeometryReadAccess::new(
            self.world,
            self.asset_acquisition,
            &self
                .system::<GeometrySystem>(GeometrySystem::ID)
                .expect("World requires GeometrySystem")
                .state,
        )
    }

    /// Refresh geometry after finalized poses or newly completed asset loads.
    pub fn evaluate_geometry(&mut self) {
        self.with_system::<GeometrySystem, _>(GeometrySystem::ID, |system, world| {
            system.evaluate(world)
        });
    }

    /// Asset-generated enclosure. Skinning combines all nonzero influence bounds
    /// into one convex box, which also encloses their normalized linear blends.
    pub fn mesh_bounding_geometry(
        &self,
        entity: EntityId,
    ) -> Result<CompoundGeometryShape, ErrorReason> {
        self.geometry_read().mesh_bounding_geometry(entity)
    }

    /// Asset-generated world-space enclosure without allocating an owned shape union.
    pub fn mesh_bounds(&self, entity: EntityId) -> Result<Option<[[f64; 3]; 2]>, ErrorReason> {
        Ok(self.geometry_read().mesh_enclosure(entity)?.bounds())
    }

    /// Borrow geometry for synchronous rendering after World evaluation. Generated
    /// mesh enclosures are reused; authored bounds require the same enclosure proof
    /// as culling queries. This does not advance evaluation or serve immediate edits.
    pub fn render_geometry(&self, entity: EntityId) -> super::RenderGeometry<'_> {
        if !self.world.state.entities.contains_key(&entity) {
            return super::RenderGeometry {
                mesh_bounds: None,
                culling: None,
            };
        }
        let value = self
            .world
            .components
            .bounding_geometry(entity.index() as usize);
        let evaluated = value.and_then(|value| value.runtime.evaluation.as_ref());
        let mesh_bounds = evaluated
            .and_then(|value| value.visual)
            .map(|value| value.bounds);
        let culling = evaluated
            .filter(|value| value.culling_valid)
            .and_then(|value| value.evaluated().ok());
        super::RenderGeometry {
            mesh_bounds,
            culling,
        }
    }

    /// Trustworthy evaluated culling geometry, borrowed until the next World mutation.
    /// None means unknown or unproven and must keep the object visible.
    pub fn culling_geometry(&self, entity: EntityId) -> Option<&CompoundGeometryShape> {
        self.geometry_read().culling_geometry(entity)
    }

    /// Final evaluated enclosure, never substituted from the picking component.
    pub fn bounding_geometry(
        &self,
        entity: EntityId,
    ) -> Result<&CompoundGeometryShape, ErrorReason> {
        self.geometry_read().bounding_geometry(entity)
    }

    /// Conservative rejection using this entity's explicit BoundingGeometry.
    /// Missing, pending, invalid or unproven enclosures keep the object visible.
    pub fn geometry_visible(
        &self,
        entity: EntityId,
        planes: &[crate::systems::geometry::GeometryPlane; 6],
    ) -> bool {
        self.geometry_read().geometry_visible(entity, planes)
    }

    /// Final evaluated picking union, using the same transforms as visualization.
    pub fn picking_geometry(
        &self,
        entity: EntityId,
    ) -> Result<&CompoundGeometryShape, ErrorReason> {
        self.geometry_read().picking_geometry(entity)
    }
}

#[cfg(feature = "skeletal-animation")]
impl GeometrySystem {
    pub(in crate::world) fn before_geometry_commit(
        &mut self,
        context: &mut crate::systems::SystemCommitContext<'_>,
    ) {
        for &(entity, component) in context.staged.changed.keys() {
            if component != ComponentValue::SKELETON {
                continue;
            }
            let previous = context
                .world_data
                .components
                .skeleton(entity.index() as usize);
            let after =
                context
                    .staged
                    .input_value(&context.world_data.components, entity, component);
            let same_source = match (previous, after) {
                (Some(previous), Some(ComponentValue::Skeleton(after))) => {
                    previous.source == after.source && previous.variant == after.variant
                }
                _ => false,
            };
            if !context.retains_component(entity, component) || !same_source {
                invalidate_skeleton_geometry(
                    &mut context.world_data.components,
                    &context.staged.entities_state,
                    |binding| binding.0 == entity,
                );
            }
        }
    }

    pub(in crate::world) fn before_geometry_asset_release(
        &mut self,
        context: &mut crate::systems::SystemAssetContext<'_>,
        event: &crate::services::asset_management::AssetLifecycleEvent,
    ) {
        invalidate_skeleton_geometry(
            &mut context.world.world.components,
            &context.world.world.state,
            |binding| binding.2 == event.key,
        );
    }
}

#[cfg(feature = "skeletal-animation")]
fn invalidate_skeleton_geometry(
    components: &mut crate::components::registry::ComponentStorage,
    authored: &crate::world::WorldEntityState,
    matches: impl Fn((EntityId, u64, AssetKey)) -> bool,
) {
    for &entity in authored.entities.keys() {
        macro_rules! invalidate {
            ($component:ident) => {
                if let Some(value) = components.$component(entity.index() as usize)
                    && let Some(state) = &mut value.runtime.evaluation
                    && state.binding.is_some_and(&matches)
                {
                    state.invalidated = true;
                }
            };
        }
        invalidate!(bounding_geometry_mut);
        invalidate!(picking_geometry_mut);
    }
}
