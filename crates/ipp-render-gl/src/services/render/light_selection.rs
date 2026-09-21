//! Renderer-private, stable bounded light selection and frame-wide shadow assignment.
use super::{
    custom_material::PreparedCustomMaterial,
    lighting::{MAX_LIGHTS, PreparedLight, RenderLightingFrame},
};
use crate::RenderError;
use ipp_core::{
    EntityId, RenderItem, WorldContext, components::Light, systems::geometry::GeometryBounds,
};
use std::collections::BTreeMap;

type Candidate = (EntityId, [f32; 16], Light);

#[derive(Default)]
pub(super) struct LightSelectionState {
    candidates: Vec<Candidate>,
    draws: BTreeMap<EntityId, Vec<EntityId>>,
    shadows: Vec<EntityId>,
    groups: Vec<(EntityId, LightGroup)>,
    selected: Vec<RankedLight>,
    influences: Vec<LightInfluence>,
    packed: Vec<Option<Result<PreparedLight, RenderError>>>,
    shadow_scores: Vec<f64>,
    prepared: PreparedLighting,
    frustums: Vec<[ipp_core::systems::geometry::GeometryPlane; 6]>,
    query_scratch: ipp_core::systems::geometry::GeometryQueryScratch,
}

pub(super) struct PreparedLighting {
    pub draws: super::draw_lighting::DrawLightingTable,
    pub shadows: Vec<(EntityId, RenderLightingFrame)>,
    pub requested_shadows: usize,
    pub visibility: ipp_core::systems::geometry::GeometryQueryResults,
    pub shadow_queries: Vec<(EntityId, usize)>,
    pub batched: bool,
    pub unlit: RenderLightingFrame,
}

impl Default for PreparedLighting {
    fn default() -> Self {
        Self {
            draws: Default::default(),
            shadows: Vec::new(),
            requested_shadows: 0,
            visibility: Default::default(),
            shadow_queries: Vec::new(),
            batched: false,
            unlit: RenderLightingFrame::empty([0.0; 4], [0.0; 3]),
        }
    }
}

pub(super) struct PreparedDrawLighting {
    pub frame: RenderLightingFrame,
    pub visible: bool,
    selected: [EntityId; MAX_LIGHTS],
    pub(super) selected_count: usize,
}

struct LightGroup {
    origin: [f64; 3],
    enclosure: Option<ipp_core::systems::geometry::GeometryEnclosure>,
    visible: bool,
}

#[derive(Clone, Copy)]
struct RankedLight {
    index: usize,
    score: f64,
    rank: f64,
    entity: EntityId,
}

// Only the bounded selection stays ordered; the full candidate list is never sorted.
fn retain_best(selected: &mut Vec<RankedLight>, candidate: RankedLight, capacity: usize) {
    if capacity == 0 {
        return;
    }
    let position = selected.partition_point(|other| {
        other.rank > candidate.rank
            || (other.rank == candidate.rank && other.entity < candidate.entity)
    });
    if position < capacity {
        selected.insert(position, candidate);
        if selected.len() > capacity {
            selected.pop();
        }
    }
}

struct ObjectInfluence {
    center: [f64; 3],
    radius: f64,
    bounded: bool,
}

impl ObjectInfluence {
    #[cfg(test)]
    fn new(origin: [f64; 3], bounds: Option<[[f64; 3]; 2]>) -> Self {
        Self {
            center: bounds.map_or(origin, |b| {
                std::array::from_fn(|i| (b[0][i] + b[1][i]) * 0.5)
            }),
            radius: bounds.map_or(0.0, |b| {
                (0..3)
                    .map(|i| ((b[1][i] - b[0][i]) * 0.5).powi(2))
                    .sum::<f64>()
                    .sqrt()
            }),
            bounded: bounds.is_some(),
        }
    }
}

struct LightInfluence {
    source: Candidate,
    entity: EntityId,
    kind: u32,
    energy: f64,
    position: [f64; 3],
    forward: [f64; 3],
    length: f64,
    range: f64,
    inner: f64,
    outer: f64,
}

impl LightInfluence {
    fn new(&(entity, model, light): &Candidate) -> Self {
        let luminance =
            0.2126 * f64::from(light.r) + 0.7152 * f64::from(light.g) + 0.0722 * f64::from(light.b);
        let forward = [
            -f64::from(model[8]),
            -f64::from(model[9]),
            -f64::from(model[10]),
        ];
        Self {
            source: (entity, model, light),
            entity,
            kind: light.kind,
            energy: luminance * f64::from(light.intensity),
            position: std::array::from_fn(|i| f64::from(model[12 + i])),
            length: forward.iter().map(|v| v * v).sum::<f64>().sqrt(),
            forward,
            range: f64::from(light.range),
            inner: f64::from(light.inner_cone).cos(),
            outer: f64::from(light.outer_cone).cos(),
        }
    }

    fn contact(&self, object: &ObjectInfluence) -> Option<LightContact> {
        if self.energy <= 0.0 || !self.energy.is_finite() {
            return None;
        }
        if self.kind == 0 {
            return Some(LightContact {
                distance_squared: 0.0,
                cone: 1.0,
            });
        }
        let delta: [f64; 3] = std::array::from_fn(|i| object.center[i] - self.position[i]);
        let distance_squared = delta.iter().map(|v| v * v).sum::<f64>();
        let reach = object.radius + self.range;
        if object.bounded && (self.range <= 0.01 || distance_squared >= reach * reach) {
            return None;
        }
        let mut cone = 1.0;
        if self.kind == 2 && distance_squared > object.radius * object.radius {
            let axial = delta
                .iter()
                .zip(self.forward)
                .map(|(a, b)| a * b)
                .sum::<f64>()
                / self.length;
            let tangent = (distance_squared - object.radius * object.radius)
                .max(0.0)
                .sqrt();
            let cosine = if axial >= tangent {
                1.0
            } else {
                (axial * tangent
                    + object.radius * (distance_squared - axial * axial).max(0.0).sqrt())
                    / distance_squared
            };
            cone = ((cosine - self.outer) / (self.inner - self.outer).max(1e-6))
                .clamp(0.0, 1.0)
                .powi(2);
            if object.bounded && (cone <= 0.0 || cone.is_nan()) {
                return None;
            }
        }
        Some(LightContact {
            distance_squared,
            cone,
        })
    }

    fn score(&self, object: &ObjectInfluence, contact: LightContact) -> f64 {
        if self.kind == 0 {
            return self.energy;
        }
        let nearest = (contact.distance_squared.sqrt() - object.radius).max(0.01);
        let mut attenuation = (1.0 - (nearest / self.range).powi(4)).clamp(0.0, 1.0)
            / (nearest * nearest)
            * contact.cone;
        if !object.bounded {
            attenuation = attenuation.max(f64::EPSILON / (nearest * nearest));
        }
        let score = self.energy * attenuation;
        if score.is_finite() {
            score
        } else {
            0.0
        }
    }

    fn influence(&self, object: &ObjectInfluence) -> f64 {
        self.contact(object)
            .map_or(0.0, |contact| self.score(object, contact))
    }
}

fn influence(candidate: &Candidate, origin: [f64; 3], bounds: Option<[[f64; 3]; 2]>) -> f64 {
    let (_, model, light) = candidate;
    let luminance =
        0.2126 * f64::from(light.r) + 0.7152 * f64::from(light.g) + 0.0722 * f64::from(light.b);
    let energy = luminance * f64::from(light.intensity);
    if energy <= 0.0 || !energy.is_finite() {
        return 0.0;
    }
    if light.kind == 0 {
        return energy;
    }
    let center = bounds.map_or(origin, |b| {
        std::array::from_fn(|i| (b[0][i] + b[1][i]) * 0.5)
    });
    let radius = bounds.map_or(0.0, |b| {
        (0..3)
            .map(|i| ((b[1][i] - b[0][i]) * 0.5).powi(2))
            .sum::<f64>()
            .sqrt()
    });
    let delta: [f64; 3] = std::array::from_fn(|i| center[i] - f64::from(model[12 + i]));
    let distance = delta.iter().map(|v| v * v).sum::<f64>().sqrt();
    let nearest = (distance - radius).max(0.01);
    let range = f64::from(light.range);
    let mut attenuation = (1.0 - (nearest / range).powi(4)).clamp(0.0, 1.0) / nearest.powi(2);
    if light.kind == 2 && distance > radius {
        let forward = [
            -f64::from(model[8]),
            -f64::from(model[9]),
            -f64::from(model[10]),
        ];
        let length = forward.iter().map(|v| v * v).sum::<f64>().sqrt();
        let cosine = (delta.iter().zip(forward).map(|(a, b)| a * b).sum::<f64>()
            / (distance * length))
            .clamp(-1.0, 1.0);
        let angle = (cosine.acos() - (radius / distance).min(1.0).asin()).max(0.0);
        let inner = f64::from(light.inner_cone).cos();
        let outer = f64::from(light.outer_cone).cos();
        attenuation *= ((angle.cos() - outer) / (inner - outer).max(1e-6))
            .clamp(0.0, 1.0)
            .powi(2);
    }
    // Missing bounds never prove exclusion. Keep an approximate weak candidate
    // even when its origin lies outside the light's range or cone.
    if bounds.is_none() {
        attenuation = attenuation.max(f64::EPSILON / nearest.powi(2));
    }
    let score = energy * attenuation;
    if score.is_finite() {
        score
    } else {
        0.0
    }
}

fn select(
    candidates: &[Candidate],
    origin: [f64; 3],
    enclosure: Option<[[f64; 3]; 2]>,
    previous: &[EntityId],
) -> Vec<RankedLight> {
    let mut selected = Vec::with_capacity(MAX_LIGHTS + 1);
    select_into(candidates, origin, enclosure, previous, &mut selected);
    selected
}

fn select_into(
    candidates: &[Candidate],
    origin: [f64; 3],
    enclosure: Option<[[f64; 3]; 2]>,
    previous: &[EntityId],
    selected: &mut Vec<RankedLight>,
) {
    selected.clear();
    for (index, candidate) in candidates.iter().enumerate() {
        let score = influence(candidate, origin, enclosure);
        if score > 0.0 {
            retain_best(
                selected,
                RankedLight {
                    index,
                    score,
                    rank: score
                        * if previous.contains(&candidate.0) {
                            1.1
                        } else {
                            1.0
                        },
                    entity: candidate.0,
                },
                MAX_LIGHTS,
            );
        }
    }
}

#[derive(Clone, Copy, Default)]
struct LightContact {
    distance_squared: f64,
    cone: f64,
}

#[cfg(test)]
fn select_prepared_into(
    candidates: &[LightInfluence],
    origin: [f64; 3],
    enclosure: Option<[[f64; 3]; 2]>,
    previous: &[EntityId],
    selected: &mut Vec<RankedLight>,
) {
    select_prepared_object(
        candidates,
        &ObjectInfluence::new(origin, enclosure),
        previous,
        selected,
    );
}

fn select_prepared_object(
    candidates: &[LightInfluence],
    object: &ObjectInfluence,
    previous: &[EntityId],
    selected: &mut Vec<RankedLight>,
) {
    selected.clear();
    let mut contacts = [LightContact::default(); MAX_LIGHTS];
    let mut ranked = false;
    let rank = |index: usize, score: f64| RankedLight {
        index,
        score,
        rank: score
            * if previous.contains(&candidates[index].entity) {
                1.1
            } else {
                1.0
            },
        entity: candidates[index].entity,
    };
    for (index, candidate) in candidates.iter().enumerate() {
        let Some(contact) = candidate.contact(object) else {
            continue;
        };
        if !ranked && selected.len() < MAX_LIGHTS {
            contacts[selected.len()] = contact;
            selected.push(RankedLight {
                index,
                score: f64::NAN,
                rank: 0.0,
                entity: candidate.entity,
            });
            continue;
        }
        if !ranked {
            let first: [usize; MAX_LIGHTS] = std::array::from_fn(|i| selected[i].index);
            selected.clear();
            for (slot, first_index) in first.into_iter().enumerate() {
                let score = candidates[first_index].score(object, contacts[slot]);
                if score > 0.0 {
                    retain_best(selected, rank(first_index, score), MAX_LIGHTS);
                }
            }
            ranked = true;
        }
        let score = candidate.score(object, contact);
        if score > 0.0 {
            retain_best(selected, rank(index, score), MAX_LIGHTS);
        }
    }
}

fn bounds(world: &WorldContext<'_>, item: &RenderItem, unbounded: bool) -> Option<[[f64; 3]; 2]> {
    if unbounded {
        return None;
    }
    #[cfg(feature = "particles")]
    if let Some(particle) = item.particle {
        #[cfg(feature = "mesh-poses")]
        if item.pose.is_some() {
            return None;
        }
        #[cfg(feature = "skeletal-animation")]
        if item.skinned {
            return None;
        }
        let (min, max) = if particle.sprite {
            ([-0.5; 3], [0.5; 3])
        } else {
            world.mesh_metadata(item.mesh)?.bounds()
        };
        // A sphere also encloses billboard rotations, including velocity alignment.
        let local_radius = (0..3)
            .map(|i| f64::from(min[i].abs().max(max[i].abs())).powi(2))
            .sum::<f64>()
            .sqrt();
        let scale = (0..3)
            .flat_map(|column| {
                (0..3).map(move |row| f64::from(item.model[column * 4 + row]).powi(2))
            })
            .sum::<f64>()
            .sqrt();
        let radius = local_radius * scale;
        return Some(std::array::from_fn(|side| {
            std::array::from_fn(|i| {
                f64::from(item.model[12 + i])
                    + if side == 0 {
                        -radius
                    } else {
                        radius
                    }
            })
        }));
    }
    if ipp_core::lighting_reuse_enabled() {
        world.render_geometry(item.entity).mesh_bounds
    } else {
        world.mesh_bounding_geometry(item.entity).ok()?.bounds()
    }
}

impl LightSelectionState {
    pub(super) fn prepare(
        &mut self,
        world: &WorldContext<'_>,
        items: &[RenderItem],
        customs: &BTreeMap<EntityId, PreparedCustomMaterial>,
        frustum: &[ipp_core::systems::geometry::GeometryPlane; 6],
        shadow_capacity: usize,
    ) -> Result<PreparedLighting, RenderError> {
        #[cfg(feature = "profiling")]
        let _allocation_scope = ipp_core::profiling::AllocationScope::new(225, "gl.light-prepare");

        if ipp_core::lighting_reuse_enabled() {
            let mut candidates = if ipp_core::render_buffer_reuse_enabled() {
                std::mem::take(&mut self.candidates)
            } else {
                Vec::new()
            };
            candidates.clear();
            candidates.extend(world.light_items());
            let result =
                self.prepare_reused(world, items, customs, frustum, shadow_capacity, &candidates);
            self.candidates = candidates;
            return result;
        }
        let candidates: Vec<_> = world.light_items().collect();
        let mut groups: BTreeMap<EntityId, Vec<&RenderItem>> = BTreeMap::new();
        for item in items {
            if item.pbr.is_some() || customs.contains_key(&item.entity) {
                groups.entry(item.entity).or_default().push(item);
            }
        }
        self.draws.retain(|entity, _| groups.contains_key(entity));
        let mut frames = super::draw_lighting::DrawLightingTable::default();
        let mut shadow_scores: BTreeMap<usize, f64> = BTreeMap::new();
        for (entity, group) in groups {
            let unbounded = customs
                .get(&entity)
                .is_some_and(|custom| custom.custom_vertex && !custom.material.conservative_bounds);
            let first = group[0];
            let mut enclosure = bounds(world, first, unbounded);
            for item in &group[1..] {
                enclosure = enclosure.zip(bounds(world, item, unbounded)).map(|(a, b)| {
                    std::array::from_fn(|side| {
                        std::array::from_fn(|i| {
                            if side == 0 {
                                a[side][i].min(b[side][i])
                            } else {
                                a[side][i].max(b[side][i])
                            }
                        })
                    })
                });
            }
            let visible = unbounded || {
                #[cfg(feature = "particles")]
                let particles = first.particle.is_some();
                #[cfg(not(feature = "particles"))]
                let particles = false;
                particles || world.geometry_visible(entity, frustum)
            };
            let origin = [first.model[12], first.model[13], first.model[14]].map(f64::from);
            let previous = self.draws.entry(entity).or_default();
            let selected = select(&candidates, origin, enclosure, previous);
            *previous = selected.iter().map(|light| light.entity).collect();
            if visible {
                for light in &selected {
                    if candidates[light.index].2.cast_shadows {
                        shadow_scores
                            .entry(light.index)
                            .and_modify(|score| *score = score.max(light.score))
                            .or_insert(light.score);
                    }
                }
            }
            let lights: Vec<_> = selected
                .iter()
                .map(|light| candidates[light.index])
                .collect();
            frames.insert(
                entity,
                PreparedDrawLighting {
                    frame: RenderLightingFrame::prepare(world, &lights)?,
                    visible,
                    selected: std::array::from_fn(|i| {
                        selected
                            .get(i)
                            .map_or(EntityId::from_bits(0), |light| light.entity)
                    }),
                    selected_count: selected.len(),
                },
            );
        }
        let requested_shadows = shadow_scores.len();
        let mut selected = Vec::new();
        for (index, score) in shadow_scores {
            let entity = candidates[index].0;
            retain_best(
                &mut selected,
                RankedLight {
                    index,
                    score,
                    rank: score
                        * if self.shadows.contains(&entity) {
                            1.1
                        } else {
                            1.0
                        },
                    entity,
                },
                shadow_capacity,
            );
        }
        let shadows = selected
            .iter()
            .map(|light| {
                Ok((
                    light.entity,
                    RenderLightingFrame::prepare(world, &[candidates[light.index]])?,
                ))
            })
            .collect::<Result<_, RenderError>>()?;
        Ok(PreparedLighting {
            draws: frames,
            shadows,
            requested_shadows,
            unlit: RenderLightingFrame::prepare(world, &[])?,
            ..Default::default()
        })
    }

    fn prepare_reused(
        &mut self,
        world: &WorldContext<'_>,
        items: &[RenderItem],
        customs: &BTreeMap<EntityId, PreparedCustomMaterial>,
        frustum: &[ipp_core::systems::geometry::GeometryPlane; 6],
        shadow_capacity: usize,
        candidates: &[Candidate],
    ) -> Result<PreparedLighting, RenderError> {
        let camera = RenderLightingFrame::prepare(world, &[])?;
        self.influences.truncate(candidates.len());
        self.packed.resize_with(candidates.len(), || None);
        for (index, candidate) in candidates.iter().enumerate() {
            if index == self.influences.len() {
                self.influences.push(LightInfluence::new(candidate));
                self.packed[index] = None;
            } else if self.influences[index].source != *candidate {
                self.influences[index] = LightInfluence::new(candidate);
                self.packed[index] = None;
            }
        }
        self.frustums.clear();
        self.frustums.push(*frustum);
        self.prepared.shadow_queries.clear();
        for (index, &(entity, model, light)) in candidates.iter().enumerate() {
            if cfg!(feature = "shadows") && light.cast_shadows {
                let packed = self.packed[index]
                    .get_or_insert_with(|| PreparedLight::prepare(entity, model, light));
                // Preparation errors are observed only if a receiver selects this light.
                if let Ok(packed) = packed {
                    let query = self.frustums.len();
                    self.frustums
                        .push(ipp_core::systems::geometry::frustum_planes(
                            packed.shadow_matrix(),
                        ));
                    self.prepared.shadow_queries.push((entity, query));
                }
            }
        }
        let geometry = world.geometry_spatial_index();
        geometry.query_frustums(
            &self.frustums,
            &mut self.prepared.visibility,
            &mut self.query_scratch,
        );
        self.prepared.batched = true;
        self.groups.clear();
        for item in items {
            if self
                .groups
                .last()
                .is_some_and(|(entity, _)| *entity == item.entity)
            {
                continue;
            }
            let custom = if item.custom_material {
                customs.get(&item.entity)
            } else {
                None
            };
            let receives = custom.map_or(item.pbr.is_some(), |m| m.material.receives_light);
            if !receives {
                continue;
            }
            let unbounded =
                custom.is_some_and(|m| m.custom_vertex && !m.material.conservative_bounds);
            self.groups.push((
                item.entity,
                LightGroup {
                    origin: [item.model[12], item.model[13], item.model[14]].map(f64::from),
                    enclosure: if unbounded {
                        None
                    } else {
                        geometry.get(item.entity).and_then(|row| row.visual)
                    },
                    visible: unbounded || self.prepared.visibility.matches(item.entity, 0),
                },
            ));
        }
        self.prepared.draws.begin(
            items
                .iter()
                .map(|item| item.entity.index() as usize + 1)
                .max()
                .unwrap_or(0),
        );
        self.shadow_scores.resize(candidates.len(), 0.0);
        self.shadow_scores.fill(0.0);
        let rank_shadows = self.prepared.shadow_queries.len() > shadow_capacity;
        for (entity, group) in &self.groups {
            let draw = self
                .prepared
                .draws
                .get_or_insert(*entity, || PreparedDrawLighting {
                    frame: RenderLightingFrame::empty(camera.camera, camera.ambient),
                    visible: group.visible,
                    selected: [EntityId::from_bits(0); MAX_LIGHTS],
                    selected_count: 0,
                });
            let object = ObjectInfluence {
                center: group.enclosure.map_or(group.origin, |b| b.center),
                radius: group.enclosure.map_or(0.0, |b| b.radius),
                bounded: group.enclosure.is_some(),
            };
            select_prepared_object(
                &self.influences,
                &object,
                &draw.selected[..draw.selected_count],
                &mut self.selected,
            );
            draw.selected_count = self.selected.len();
            for (slot, light) in draw.selected.iter_mut().zip(&self.selected) {
                *slot = light.entity;
            }
            if group.visible {
                for light in &self.selected {
                    if candidates[light.index].2.cast_shadows {
                        let score = if !rank_shadows {
                            1.0
                        } else if light.score.is_nan() {
                            self.influences[light.index].influence(&object)
                        } else {
                            light.score
                        };
                        self.shadow_scores[light.index] =
                            self.shadow_scores[light.index].max(score);
                    }
                }
            }
            draw.visible = group.visible;
            if draw.visible {
                draw.frame.reset(camera.camera, camera.ambient);
            }
            for (index, selected) in self.selected.iter().enumerate() {
                let &(entity, model, light) = &candidates[selected.index];
                let packed = self.packed[selected.index]
                    .get_or_insert_with(|| PreparedLight::prepare(entity, model, light))
                    .as_ref()
                    .map_err(Clone::clone)?;
                if draw.visible {
                    draw.frame.push(index, packed);
                }
            }
            if draw.visible {
                draw.frame.finish();
            }
        }

        self.selected.clear();
        let mut requested_shadows = 0;
        for (index, &score) in self.shadow_scores.iter().enumerate() {
            if score == 0.0 {
                continue;
            }
            requested_shadows += 1;
            let entity = candidates[index].0;
            retain_best(
                &mut self.selected,
                RankedLight {
                    index,
                    score,
                    rank: score
                        * if self.shadows.contains(&entity) {
                            1.1
                        } else {
                            1.0
                        },
                    entity,
                },
                shadow_capacity,
            );
        }
        self.prepared.shadows.clear();
        for light in &self.selected {
            let mut frame = RenderLightingFrame::empty(camera.camera, camera.ambient);
            let packed = self.packed[light.index]
                .as_ref()
                .expect("selected shadow light")
                .as_ref()
                .map_err(Clone::clone)?;
            frame.push(0, packed);
            frame.finish();
            self.prepared.shadows.push((light.entity, frame));
        }
        self.prepared.requested_shadows = requested_shadows;
        self.prepared.unlit = camera;
        Ok(std::mem::take(&mut self.prepared))
    }

    pub(super) fn recycle(&mut self, prepared: PreparedLighting) {
        if ipp_core::lighting_reuse_enabled() {
            self.prepared = prepared;
        }
    }

    pub(super) fn assign_shadows(&mut self, prepared: &mut PreparedLighting) {
        if ipp_core::lighting_reuse_enabled() {
            self.shadows.clear();
            self.shadows
                .extend(prepared.shadows.iter().map(|(entity, _)| *entity));
        } else {
            self.shadows = prepared.shadows.iter().map(|(entity, _)| *entity).collect();
        }
        let count = self.shadows.len() as u32;
        let grid = (f64::from(count).sqrt().ceil()) as f32;
        for (slot, (_, frame)) in prepared.shadows.iter_mut().enumerate() {
            frame.shadow_count = count;
            frame.shadow_settings[0] = slot as f32;
            frame.shadow_settings[3] = grid;
        }
        for draw in prepared.draws.values_mut() {
            if !draw.visible {
                continue;
            }
            let frame = &mut draw.frame;
            frame.shadow_count = count;
            for (index, light) in draw.selected[..draw.selected_count].iter().enumerate() {
                frame.shadow_settings[index * 4] = self
                    .shadows
                    .iter()
                    .position(|entity| entity == light)
                    .map_or(-1.0, |slot| slot as f32);
                frame.shadow_settings[index * 4 + 3] = grid;
            }
        }
    }
}

#[cfg(test)]
#[path = "light_selection_tests.rs"]
mod light_selection_tests;
