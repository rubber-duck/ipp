//! Renderer-private, stable bounded light selection and frame-wide shadow assignment.
use super::super::materials::custom_material::PreparedCustomMaterial;
use super::super::outputs::scene::{
    RenderEntity as EntityId, RenderScene, SceneItem as RenderItem,
};
use super::lights::{MAX_LIGHTS, PreparedLight, RenderLightingFrame};
use crate::RenderError;
use ipp_core::components::Light;
use std::collections::BTreeMap;

type Candidate = (EntityId, [f32; 16], Light);

#[derive(Default)]
pub(in crate::services::render) struct LightSelectionState {
    candidates: Vec<Candidate>,
    shadows: Vec<EntityId>,
    groups: Vec<(EntityId, LightGroup)>,
    selected: Vec<RankedLight>,
    influences: Vec<LightInfluence>,
    packed: Vec<Option<Result<PreparedLight, RenderError>>>,
    shadow_scores: Vec<f64>,
    prepared: PreparedLighting,
    frustums: Vec<[ipp_core::systems::geometry::GeometryPlane; 6]>,
}

pub(in crate::services::render) struct PreparedLighting {
    pub draws: super::draw_blocks::DrawLightingTable,
    pub shadows: Vec<(EntityId, RenderLightingFrame)>,
    pub requested_shadows: usize,
    pub visibility: super::super::outputs::scene::SceneVisibility,
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

pub(in crate::services::render) struct PreparedDrawLighting {
    pub frame: RenderLightingFrame,
    pub visible: bool,
    selected: Vec<EntityId>,
    pub(super) selected_count: usize,
}

struct LightGroup {
    origin: [f64; 3],
    enclosure: Option<[[f64; 3]; 2]>,
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

impl LightSelectionState {
    pub(in crate::services::render) fn prepare(
        &mut self,
        world: &RenderScene<'_>,
        items: &[RenderItem<'_>],
        customs: &BTreeMap<EntityId, PreparedCustomMaterial>,
        frustum: &[ipp_core::systems::geometry::GeometryPlane; 6],
        shadow_capacity: usize,
    ) -> Result<PreparedLighting, RenderError> {
        #[cfg(feature = "instrumentation")]
        let _allocation_scope = ipp_core::profiling::AllocationScope::new(225, "gl.light-prepare");

        let mut candidates = std::mem::take(&mut self.candidates);
        candidates.clear();
        candidates.extend(world.lights.iter().copied());
        let result =
            self.prepare_candidates(world, items, customs, frustum, shadow_capacity, &candidates);
        self.candidates = candidates;
        result
    }

    fn prepare_candidates(
        &mut self,
        world: &RenderScene<'_>,
        items: &[RenderItem<'_>],
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
            if light.cast_shadows {
                let packed = self.packed[index]
                    .get_or_insert_with(|| PreparedLight::prepare(entity.entity, model, light));
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
        self.prepared
            .visibility
            .prepare(world, items, &self.frustums);
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
                        world.visual_bounds(item.entity)
                    },
                    visible: unbounded || self.prepared.visibility.matches(item.entity, 0),
                },
            ));
        }
        self.prepared.draws.begin();
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
                    selected: Vec::with_capacity(MAX_LIGHTS),
                    selected_count: 0,
                });
            let object = ObjectInfluence::new(group.origin, group.enclosure);
            select_prepared_object(
                &self.influences,
                &object,
                &draw.selected[..draw.selected_count],
                &mut self.selected,
            );
            draw.selected_count = self.selected.len();
            draw.selected.clear();
            draw.selected
                .extend(self.selected.iter().map(|light| light.entity));
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
                    .get_or_insert_with(|| PreparedLight::prepare(entity.entity, model, light))
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

    pub(in crate::services::render) fn recycle(&mut self, prepared: PreparedLighting) {
        self.prepared = prepared;
    }

    pub(in crate::services::render) fn assign_shadows(&mut self, prepared: &mut PreparedLighting) {
        self.shadows.clear();
        self.shadows
            .extend(prepared.shadows.iter().map(|(entity, _)| *entity));
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
#[path = "selection_tests.rs"]
mod light_selection_tests;
