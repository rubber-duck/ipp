//! Camera-domain lights and spotlight depth views from immutable publications.

use std::collections::BTreeMap;

use ipp_core::{
    EntityId, HostRuntime, OutputRef, PublishedSceneTraversal, WorldPublicationId,
    components::{Camera, Light},
    systems::{
        camera::{self, CameraPublication},
        geometry::GeometryShapeTransform,
        render::{RenderPublication, RenderSystem, plot_presentation::RenderEntity},
    },
};

/// The selected Camera or light transform could not define a rendering view.
#[derive(Debug)]
pub struct InvalidCamera;

/// Maximum lights bound for a single draw.
pub const MAX_DOMAIN_LIGHTS: usize = 8;

/// Edge length of a spotlight atlas tile.
pub const SHADOW_TILE: u32 = 1024;

/// Fixed-layout light inputs for a draw.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, bytemuck::NoUninit)]
pub struct Lighting {
    /// World-space camera position or orthographic direction.
    pub camera: [f32; 4],
    /// Ambient RGB and selected light count.
    pub ambient_count: [f32; 4],
    /// Position, direction, color and cone data for each selected light.
    pub lights: [[[f32; 4]; 4]; MAX_DOMAIN_LIGHTS],
    /// Per-light World to shadow clip transform.
    pub shadow_matrices: [[f32; 16]; MAX_DOMAIN_LIGHTS],
    /// Atlas slot, bias, inverse tile size and grid width.
    pub shadow_settings: [[f32; 4]; MAX_DOMAIN_LIGHTS],
}

struct Candidate {
    entity: RenderEntity,
    light: Light,
    model: [f32; 16],
    record: [[f32; 4]; 4],
    shadow: Option<(u32, [f32; 16])>,
}

#[derive(Default)]
struct LightSelection {
    frame: u64,
    chosen: [Option<RenderEntity>; MAX_DOMAIN_LIGHTS],
}

#[derive(Default)]
/// Per-receiver hysteresis across frames.
pub struct LightSelections {
    previous: BTreeMap<(OutputRef, RenderEntity), LightSelection>,
    frame: u64,
}

impl LightSelections {
    /// Advance the light history and prune expired receivers.
    pub fn begin_frame(&mut self, host: &HostRuntime, frame: u64) {
        self.frame = frame;
        if !frame.is_multiple_of(120) {
            return;
        }
        self.previous.retain(|(output, entity), selection| {
            frame - selection.frame <= 120
                && host.world_fault(entity.world).is_ok()
                && host
                    .latest_publication(output.world().id())
                    .and_then(|publication| host.output(publication, *output))
                    .is_some()
        });
    }
}

/// Lights and atlas assignments in one completed Camera domain.
pub struct LightCatalog {
    camera: [f32; 4],
    ambient: [f32; 3],
    candidates: Vec<Candidate>,
    shadow_grid: u32,
}

impl LightCatalog {
    /// Collect lights through the spatial publication traversal.
    pub fn prepare(
        host: &HostRuntime,
        traversal: &mut PublishedSceneTraversal,
        output: OutputRef,
        publication: WorldPublicationId,
        camera: &CameraPublication,
    ) -> Result<Self, InvalidCamera> {
        let pose = camera.pose.render_matrix().map_err(|_| InvalidCamera)?;
        let eye = [pose[12], pose[13], pose[14]];
        let camera = if camera.projection.projection == 1 {
            let [x, y, z] = unit([pose[8], pose[9], pose[10]]).ok_or(InvalidCamera)?;
            [x, y, z, 1.0]
        } else {
            [eye[0], eye[1], eye[2], 0.0]
        };
        let mut catalog = Self {
            camera,
            ambient: [0.0; 3],
            candidates: Vec::new(),
            shadow_grid: 0,
        };
        traversal
            .reset(host, publication, Some(output))
            .map_err(|_| InvalidCamera)?;
        while let Some(visit) = traversal.next(host) {
            let Some(scene) = visit
                .publication
                .chunk(RenderSystem::ID)
                .and_then(|chunk| chunk.data::<RenderPublication>())
            else {
                continue;
            };
            if visit.publication.id == publication {
                catalog.ambient = scene.state.ambient_light;
            }
            let placement = visit.placement.render_matrix().map_err(|_| InvalidCamera)?;
            for source in &scene.lights {
                let model = ipp_core::math::multiply(placement, source.model);
                let Some([x, y, z]) = unit([-model[8], -model[9], -model[10]]) else {
                    continue;
                };
                let light = source.light;
                let record = [
                    [model[12], model[13], model[14], light.kind as f32],
                    [x, y, z, light.inner_cone.cos()],
                    [
                        light.r * light.intensity,
                        light.g * light.intensity,
                        light.b * light.intensity,
                        light.range,
                    ],
                    [
                        light.outer_cone.cos(),
                        (f64::from(light.shadow_radius) / (2.0 * f64::from(light.outer_cone).tan()))
                            .min(f64::from(f32::MAX)) as f32,
                        light.shadow_near,
                        light.range,
                    ],
                ];
                let entity = RenderEntity {
                    world: visit.publication.world,
                    entity: source.entity,
                    incarnation: source.incarnation,
                };
                catalog.candidates.push(Candidate {
                    entity,
                    light,
                    model,
                    record,
                    shadow: None,
                });
            }
        }

        let mut shadows: Vec<_> = catalog
            .candidates
            .iter()
            .enumerate()
            .filter(|(_, candidate)| candidate.light.cast_shadows)
            .map(|(index, _)| index)
            .collect();
        if shadows.len() > MAX_DOMAIN_LIGHTS {
            shadows.sort_by(|&a, &b| {
                let a = &catalog.candidates[a];
                let b = &catalog.candidates[b];
                b.influence(eye, 0.0)
                    .total_cmp(&a.influence(eye, 0.0))
                    .then_with(|| a.entity.cmp(&b.entity))
            });
            shadows.truncate(MAX_DOMAIN_LIGHTS);
        }
        for (slot, index) in shadows.iter().copied().enumerate() {
            let candidate = &mut catalog.candidates[index];
            let matrix = shadow_matrix(candidate.entity.entity, candidate.model, candidate.light)?;
            candidate.shadow = Some((slot as u32, matrix));
        }
        catalog.shadow_grid = (shadows.len() as f32).sqrt().ceil() as u32;
        Ok(catalog)
    }

    /// Whether receivers need their own light selection.
    pub fn over_capacity(&self) -> bool {
        self.candidates.len() > MAX_DOMAIN_LIGHTS
    }

    /// Default selection for the domain.
    pub fn root(&self) -> Lighting {
        let mut result = self.blank();
        for (slot, candidate) in self.candidates.iter().take(MAX_DOMAIN_LIGHTS).enumerate() {
            self.push(&mut result, slot, candidate);
        }
        result
    }

    /// Rank bounded influences for this receiver.
    pub fn select(
        &self,
        output: OutputRef,
        receiver: RenderEntity,
        center: [f32; 3],
        radius: f32,
        history: &mut LightSelections,
    ) -> Lighting {
        let selection = history.previous.entry((output, receiver)).or_default();
        selection.frame = history.frame;
        let previous = &mut selection.chosen;
        let mut selected = Vec::with_capacity(MAX_DOMAIN_LIGHTS + 1);
        for candidate in &self.candidates {
            let score = candidate.influence(center, radius);
            let score = if previous.contains(&Some(candidate.entity)) {
                score * 1.1
            } else {
                score
            };
            let index = selected.partition_point(|&(other, rank): &(&Candidate, f64)| {
                rank > score || rank == score && other.entity < candidate.entity
            });
            if index < MAX_DOMAIN_LIGHTS {
                selected.insert(index, (candidate, score));
                selected.truncate(MAX_DOMAIN_LIGHTS);
            }
        }

        previous.fill(None);
        let mut result = self.blank();
        for (slot, (candidate, _)) in selected.into_iter().enumerate() {
            previous[slot] = Some(candidate.entity);
            self.push(&mut result, slot, candidate);
        }
        result
    }

    fn blank(&self) -> Lighting {
        let [r, g, b] = self.ambient;
        Lighting {
            camera: self.camera,
            ambient_count: [r, g, b, 0.0],
            shadow_settings: [[-1.0, 0.0, 0.0, 0.0]; MAX_DOMAIN_LIGHTS],
            ..Default::default()
        }
    }

    fn push(&self, frame: &mut Lighting, slot: usize, candidate: &Candidate) {
        frame.lights[slot] = candidate.record;
        frame.ambient_count[3] += 1.0;
        if let Some((depth, matrix)) = candidate.shadow {
            frame.shadow_matrices[slot] = matrix;
            frame.shadow_settings[slot] = [
                depth as f32,
                candidate.light.shadow_bias,
                1.0 / SHADOW_TILE as f32,
                self.shadow_grid as f32,
            ];
        }
    }

    /// Assigned shadow tiles and their projection matrices.
    pub fn shadow_views(&self) -> impl Iterator<Item = (u32, [f32; 16])> + '_ {
        self.candidates
            .iter()
            .filter_map(|candidate| candidate.shadow)
    }

    /// Tiles per edge of the atlas.
    pub fn shadow_grid(&self) -> u32 {
        self.shadow_grid
    }
}

impl Candidate {
    fn influence(&self, center: [f32; 3], radius: f32) -> f64 {
        let light = self.light;
        let energy = f64::from(light.intensity)
            * (0.2126 * f64::from(light.r)
                + 0.7152 * f64::from(light.g)
                + 0.0722 * f64::from(light.b));
        if light.kind == 0 {
            return energy;
        }
        let delta: [f64; 3] =
            std::array::from_fn(|i| f64::from(center[i]) - f64::from(self.model[12 + i]));
        let length = delta[0].hypot(delta[1]).hypot(delta[2]);
        let radius = f64::from(radius);
        let nearest = (length - radius).max(0.01);
        let attenuation = (1.0 - (nearest / f64::from(light.range)).powi(4)).clamp(0.0, 1.0)
            / (nearest * nearest);
        if light.kind != 2 || length <= radius {
            return energy * attenuation;
        }
        let axial = delta
            .into_iter()
            .zip(self.record[1])
            .take(3)
            .map(|(a, b)| a * f64::from(b))
            .sum::<f64>();
        let tangent = (length * length - radius * radius).max(0.0).sqrt();
        let cosine = if axial >= tangent {
            1.0
        } else {
            (axial * tangent + radius * (length * length - axial * axial).max(0.0).sqrt())
                / (length * length)
        };
        let outer = f64::from(light.outer_cone).cos();
        let inner = f64::from(light.inner_cone).cos();
        let cone = ((cosine - outer) / (inner - outer).max(1e-6))
            .clamp(0.0, 1.0)
            .powi(2);
        energy * attenuation * cone
    }
}

fn unit(value: [f32; 3]) -> Option<[f32; 3]> {
    let value = value.map(f64::from);
    let length = value.iter().map(|axis| axis * axis).sum::<f64>().sqrt();
    (length.is_finite() && length > 0.0).then(|| value.map(|axis| (axis / length) as f32))
}

fn shadow_matrix(
    entity: EntityId,
    model: [f32; 16],
    light: ipp_core::components::Light,
) -> Result<[f32; 16], InvalidCamera> {
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let front = unit([model[8], model[9], model[10]]).ok_or(InvalidCamera)?;
    let right = unit(cross([model[4], model[5], model[6]], front)).ok_or(InvalidCamera)?;
    let up = cross(front, right);
    let pose = GeometryShapeTransform::from_matrix([
        right[0], right[1], right[2], 0.0, up[0], up[1], up[2], 0.0, front[0], front[1], front[2],
        0.0, model[12], model[13], model[14], 1.0,
    ])
    .map_err(|_| InvalidCamera)?;
    let projection = Camera {
        fov_y: 2.0 * light.outer_cone,
        near: light.shadow_near,
        far: light.range,
        ..Camera::default()
    };
    let prepared =
        camera::prepare_affine(entity, &projection, &pose, 1, 1).map_err(|_| InvalidCamera)?;
    Ok(prepared.view_projection)
}

/// Transform local mesh bounds for per-draw light ranking.
pub fn light_receiver(model: [f32; 16], bounds: Option<([f32; 3], [f32; 3])>) -> ([f32; 3], f32) {
    let (min, max) = bounds.unwrap_or(([0.0; 3], [0.0; 3]));
    let center: [f32; 3] = std::array::from_fn(|i| min[i].midpoint(max[i]));
    let half: [f32; 3] = std::array::from_fn(|i| (max[i] - min[i]) * 0.5);
    let position = std::array::from_fn(|i| {
        model[12 + i] + model[i] * center[0] + model[4 + i] * center[1] + model[8 + i] * center[2]
    });
    let extent: [f32; 3] = std::array::from_fn(|i| {
        model[i].abs() * half[0] + model[4 + i].abs() * half[1] + model[8 + i].abs() * half[2]
    });
    (position, extent[0].hypot(extent[1]).hypot(extent[2]))
}
