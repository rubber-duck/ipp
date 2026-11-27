//! Prepare ordered mesh instances from one completed Camera domain.

use ipp_core::{
    HostRuntime, OutputRef, PublishedSceneTraversal, WorldAttachmentMode, WorldPublicationId,
    math::multiply,
    services::asset_management::{AssetKey, MAX_JOINTS},
    systems::{
        camera::CameraPublication,
        geometry::{GeometryBounds, GeometryPublication, GeometrySystem, frustum_planes},
        plot::{PlotPublication, PlotSystem},
        render::{
            PublishedRenderItem, RenderPublication, RenderSystem, plot_presentation::RenderEntity,
        },
    },
};
use ipp_render_scene::lighting::{
    LightCatalog, LightSelections, Lighting, MAX_DOMAIN_LIGHTS, light_receiver,
};

use crate::{
    assets::{MeshBuffers, WebGpuMesh, WebGpuTexture},
    service::RenderError,
};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct Instance {
    model: [f32; 16],
    tint: [f32; 4],
    options: [f32; 4],
    surface: [f32; 4],
}

impl Instance {
    pub fn rigid_unlit(&self) -> bool {
        self.surface[2] == 0.
            && self.options[0] == 0.
            && self.options[1] == 0.
            && self.tint[3] >= 0.
    }
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct CameraUniform {
    pub projection: [f32; 16],
    eye: [f32; 4],
    ambient_count: [f32; 4],
    lights: [[[f32; 4]; 4]; MAX_DOMAIN_LIGHTS],
    shadow_matrices: [[f32; 16]; MAX_DOMAIN_LIGHTS],
    shadow_settings: [[f32; 4]; MAX_DOMAIN_LIGHTS],
}

impl CameraUniform {
    fn new(projection: [f32; 16], lighting: &Lighting) -> Self {
        Self {
            projection,
            eye: lighting.camera,
            ambient_count: lighting.ambient_count,
            lights: lighting.lights,
            shadow_matrices: lighting.shadow_matrices,
            shadow_settings: lighting.shadow_settings,
        }
    }
}

pub(super) struct Batch<'a> {
    pub custom: Option<crate::custom_mesh::CustomDraw<'a>>,
    pub mesh: MeshBuffers,
    pub pose: MeshBuffers,
    pub blend: usize,
    pub palette: &'a [[f32; 16]],
    pub texture: Option<&'a wgpu::BindGroup>,
    pub camera: CameraUniform,
    pub first: usize,
    pub count: usize,
}

impl Batch<'_> {
    fn matches(&self, streams: &Streams<'_>, camera: &CameraUniform) -> bool {
        self.mesh.vertices == streams.mesh.vertices
            && self.pose.vertices == streams.pose.vertices
            && std::ptr::eq(self.palette, streams.palette)
            && self.texture == streams.texture
            && self.camera == *camera
            && match (&self.custom, &streams.custom) {
                (Some(a), Some(b)) => a.matches(b),
                (None, None) => true,
                _ => false,
            }
    }
}

struct Streams<'a> {
    custom: Option<crate::custom_mesh::CustomDraw<'a>>,
    asset: &'a WebGpuMesh,
    mesh: &'a MeshBuffers,
    pose: &'a MeshBuffers,
    palette: &'a [[f32; 16]],
    texture: Option<&'a wgpu::BindGroup>,
    weight: f32,
}

impl<'a> Streams<'a> {
    fn resolve(
        host: &'a HostRuntime,
        publication: WorldPublicationId,
        published: &'a PublishedRenderItem,
    ) -> Option<Self> {
        let draw = &published.item;
        if draw.particle.is_some_and(|particle| particle.sprite) {
            return None;
        }

        let asset = resource::<WebGpuMesh>(host, publication, draw.mesh.asset)?;
        let mesh = asset.buffers.as_ref()?;
        let custom = match &published.custom {
            Some(custom) => Some(crate::custom_mesh::CustomDraw::resolve(
                host,
                publication,
                custom,
                asset.metadata.attributes(),
            )?),
            None => None,
        };
        let (pose, weight) = match draw.pose {
            Some((key, weight)) => {
                let pose = resource::<WebGpuMesh>(host, publication, key.asset)?;
                if pose.metadata.vertex_count() != asset.metadata.vertex_count() {
                    return None;
                }
                (pose.buffers.as_ref()?, weight)
            }
            None => (mesh, 0.0),
        };

        let palette = if draw.skinned {
            let last = usize::from(asset.metadata.max_joint_index()?);
            published
                .palette
                .as_deref()
                .filter(|p| last < p.len() && p.len() <= MAX_JOINTS)?
        } else {
            &[]
        };

        let texture = match draw.texture {
            Some(key) => Some(
                &resource::<WebGpuTexture>(host, publication, key.asset)?
                    .texture
                    .as_ref()?
                    .0,
            ),
            None => None,
        };
        Some(Self {
            custom,
            asset,
            mesh,
            pose,
            palette,
            texture,
            weight,
        })
    }
}

pub(super) struct SurfaceVisit<'a> {
    pub edge: &'a ipp_core::PublishedWorldAttachment,
    pub output: OutputRef,
    pub publication: WorldPublicationId,
    pub model: [f32; 16],
}

pub(super) struct CameraScene<'a> {
    pub batches: Vec<Batch<'a>>,
    pub failed: u32,
    pub surfaces: Vec<SurfaceVisit<'a>>,
    pub projection: [f32; 16],
    pub lighting: Lighting,
    pub camera_pose: [f32; 16],
    pub shadow_grid: u32,
    pub shadow_views: Vec<(u32, [f32; 16])>,
    pub casters: Vec<Batch<'a>>,
    pub caster_instances: Vec<Instance>,
    pub plots: Vec<(
        &'a ipp_core::WorldPublication,
        [f32; 16],
        &'a PlotPublication,
    )>,
}

#[derive(Default)]
pub(super) struct Scene {
    traversal: PublishedSceneTraversal,
    lights: LightSelections,
    generated: crate::derived_meshes::DerivedMeshes,
    frame: u64,
    pub instances: Vec<Instance>,
}

impl Scene {
    pub fn prepare<'a>(
        &mut self,
        host: &'a HostRuntime,
        gpu: &crate::device::Graphics,
        output: OutputRef,
        publication: WorldPublicationId,
        extent: [f64; 2],
    ) -> Result<CameraScene<'a>, RenderError> {
        let camera = host
            .output(publication, output)
            .and_then(|chunk| chunk.data::<CameraPublication>())
            .ok_or(RenderError::Draw("WebGPU requires a valid Camera output"))?;
        let lights = LightCatalog::prepare(host, &mut self.traversal, output, publication, camera)
            .map_err(|_| RenderError::Draw("Invalid Camera lighting domain"))?;
        let shadow_views = lights.shadow_views().collect::<Vec<_>>();
        let mut casters = Vec::new();
        let mut caster_instances = Vec::new();
        self.frame = self.frame.saturating_add(1);
        self.lights.begin_frame(host, self.frame);
        let projection = camera
            .prepare_for_extent(extent)
            .map_err(|_| RenderError::Draw("Invalid Camera projection"))?
            .view_projection;
        self.generated.retain(self.frame);
        let camera_pose = camera
            .pose
            .render_matrix()
            .map_err(|_| RenderError::Draw("Invalid camera pose"))?;
        let mut plots = Vec::new();
        let mut sprites = Vec::new();
        let frustum = frustum_planes(projection);
        let lighting = lights.root();
        let state = host
            .publication(publication)
            .and_then(|p| p.chunk(RenderSystem::ID))
            .and_then(|c| c.data::<RenderPublication>())
            .map(|scene| scene.state)
            .unwrap_or_default();

        self.traversal
            .reset(host, publication, Some(output))
            .map_err(|_| RenderError::Draw("Unavailable Camera domain"))?;
        self.instances.clear();
        let mut batches: Vec<Batch<'a>> = Vec::new();
        let mut surfaces = Vec::new();
        let mut failed = 0u32;

        while let Some(visit) = self.traversal.next(host) {
            let source = visit.publication;
            let placement = visit
                .placement
                .render_matrix()
                .map_err(|_| RenderError::Draw("Invalid spatial placement"))?;
            for edge in &source.attachments {
                if edge.mode != WorldAttachmentMode::Spatial
                    && edge.placement_output.is_none_or(|owner| owner == output)
                    && let Some((child, publication)) =
                        edge.output.zip(host.attached_publication(edge))
                {
                    surfaces.push(SurfaceVisit {
                        edge,
                        output: child,
                        publication: publication.id,
                        model: multiply(placement, edge.placement.map(|v| v as f32)),
                    });
                }
            }
            if let Some(plot) = source
                .chunk(PlotSystem::ID)
                .and_then(|c| c.data::<PlotPublication>())
                && !plot.charts.is_empty()
            {
                plots.push((source, placement, plot));
            }
            let Some(scene) = source
                .chunk(RenderSystem::ID)
                .and_then(|chunk| chunk.data::<RenderPublication>())
            else {
                continue;
            };
            for item in scene
                .debug
                .iter()
                .filter(|item| state.show_all_debug_geometries || item.is_rendered)
            {
                let Some(mesh) = self.generated.debug(gpu, &item.geometry, self.frame) else {
                    failed += 1;
                    continue;
                };
                let [r, g, b] = item.color_override.unwrap_or(state.debug_geometry_color);
                generated_batch(
                    &mut batches,
                    &mut self.instances,
                    &mesh.buffers,
                    multiply(placement, item.model),
                    [r, g, b, 1.],
                    CameraUniform::new(projection, &lighting),
                    None,
                );
            }

            let geometry = source
                .chunk(GeometrySystem::ID)
                .and_then(|c| c.data::<GeometryPublication>());
            let planes = frustum.map(|plane| visit.placement.local_plane(&plane));

            for published in &scene.items {
                let draw = &published.item;
                if !shadow_views.is_empty()
                    && (published
                        .custom
                        .as_ref()
                        .is_some_and(|c| c.casts_shadows && c.alpha_mode != 2)
                        || draw.pbr.is_some_and(|p| p.cast_shadows))
                    && let Some(streams) = Streams::resolve(host, source.id, published)
                {
                    if streams
                        .custom
                        .as_ref()
                        .is_some_and(|c| c.program.shadow.is_none())
                    {
                        return Err(RenderError::Draw("Missing custom material shadow pass"));
                    }
                    casters.push(Batch {
                        mesh: streams.mesh.clone(),
                        pose: streams.pose.clone(),
                        custom: streams.custom,
                        blend: 0,
                        palette: streams.palette,
                        texture: None,
                        camera: CameraUniform::new(projection, &lighting),
                        first: caster_instances.len(),
                        count: 1,
                    });
                    caster_instances.push(Instance {
                        model: multiply(placement, draw.model),
                        tint: [1.; 4],
                        options: [streams.weight, f32::from(draw.skinned), 0., 0.],
                        surface: [0.; 4],
                    });
                }
                if draw.particle.is_none()
                    && published
                        .custom
                        .as_ref()
                        .is_none_or(|c| c.conservative_bounds)
                    && let Some(geometry) = geometry
                    && let Ok(i) = geometry
                        .entities
                        .binary_search_by_key(&draw.entity, |item| item.entity)
                    && geometry.entities[i]
                        .culling
                        .as_ref()
                        .is_some_and(|shape| !shape.intersects_frustum(&planes))
                {
                    continue;
                }
                if let Some(particle) = draw.particle.filter(|p| p.sprite) {
                    let Some(model) = ipp_render_scene::particles::billboard(
                        draw,
                        camera_pose,
                        placement,
                        multiply(placement, draw.model),
                    ) else {
                        failed += 1;
                        continue;
                    };
                    let texture = if let Some(key) = draw.texture {
                        let Some(texture) = resource::<WebGpuTexture>(host, source.id, key.asset)
                            .and_then(|t| t.texture.as_ref())
                        else {
                            failed += 1;
                            continue;
                        };
                        Some(&texture.0)
                    } else {
                        None
                    };
                    let depth = if particle.additive {
                        f32::INFINITY
                    } else {
                        multiply(projection, model)[14]
                    };
                    let entity = RenderEntity {
                        world: source.world,
                        entity: draw.entity,
                        incarnation: published.incarnation,
                    };
                    sprites.push((
                        depth,
                        entity,
                        particle.id,
                        texture,
                        usize::from(particle.additive) + 1,
                        Instance {
                            model,
                            tint: [
                                draw.material.r,
                                draw.material.g,
                                draw.material.b,
                                -particle.opacity,
                            ],
                            options: [0.; 4],
                            surface: [0.; 4],
                        },
                    ));
                    continue;
                }
                let Some(streams) = Streams::resolve(host, source.id, published) else {
                    failed = failed.saturating_add(1);
                    continue;
                };

                let model = multiply(placement, draw.model);
                let camera = if draw.pbr.is_some() && lights.over_capacity() {
                    let entity = RenderEntity {
                        world: source.world,
                        entity: draw.entity,
                        incarnation: published.incarnation,
                    };
                    let (center, radius) =
                        light_receiver(model, Some(streams.asset.metadata.bounds()));
                    CameraUniform::new(
                        projection,
                        &lights.select(output, entity, center, radius, &mut self.lights),
                    )
                } else {
                    CameraUniform::new(projection, &lighting)
                };
                if let Some(batch) = batches
                    .last_mut()
                    .filter(|batch| batch.matches(&streams, &camera))
                {
                    batch.count += 1;
                } else {
                    batches.push(Batch {
                        blend: usize::from(streams.custom.as_ref().is_some_and(|m| m.blended)),
                        custom: streams.custom,
                        mesh: streams.mesh.clone(),
                        pose: streams.pose.clone(),
                        palette: streams.palette,
                        texture: streams.texture,
                        camera,
                        first: self.instances.len(),
                        count: 1,
                    });
                }

                self.instances.push(Instance {
                    model,
                    tint: [draw.material.r, draw.material.g, draw.material.b, 1.0],
                    options: [
                        streams.weight,
                        f32::from(draw.skinned),
                        f32::from(draw.texture_weights),
                        f32::from(draw.solid_fallback),
                    ],
                    surface: draw.pbr.map_or([0.0; 4], |p| {
                        [
                            p.metallic,
                            p.roughness,
                            1.0 + f32::from(p.receive_shadows),
                            f32::from(draw.normals),
                        ]
                    }),
                });
            }
        }

        for &(source, placement, plot) in &plots {
            for chart in &plot.charts {
                for item in &chart.meshes {
                    let mesh = &item.geometry.meshes[item.mesh_index];
                    let custom = mesh
                        .material
                        .as_ref()
                        .zip(item.render.custom.as_ref())
                        .and_then(|(streams, material)| {
                            let attributes = u32::from(!streams.colors.is_empty())
                                | (u32::from(!streams.uvs.is_empty()) << 1)
                                | (u32::from(!streams.normals.is_empty()) << 2);
                            crate::custom_mesh::CustomDraw::resolve(
                                host, source.id, material, attributes,
                            )
                        });
                    let Some(generated) = self.generated.plot(gpu, item, custom.is_some()) else {
                        failed += 1;
                        continue;
                    };
                    generated_batch(
                        &mut batches,
                        &mut self.instances,
                        &generated.buffers,
                        multiply(placement, item.render.item.model),
                        mesh.color,
                        CameraUniform::new(projection, &lighting),
                        custom,
                    );
                }
            }
        }
        sprites.sort_by(|a, b| {
            b.0.total_cmp(&a.0)
                .then_with(|| (a.1, a.2).cmp(&(b.1, b.2)))
        });
        for (_, _, _, texture, blend, instance) in sprites {
            if let Some(batch) = batches.last_mut().filter(|b| {
                b.mesh.vertices == gpu.quad.vertices && b.texture == texture && b.blend == blend
            }) {
                batch.count += 1;
            } else {
                batches.push(Batch {
                    custom: None,
                    mesh: gpu.quad.clone(),
                    pose: gpu.quad.clone(),
                    blend,
                    palette: &[],
                    texture,
                    camera: CameraUniform::new(projection, &lighting),
                    first: self.instances.len(),
                    count: 1,
                });
            }
            self.instances.push(instance);
        }
        let depth = |surface: &SurfaceVisit<'_>| {
            let m = surface.model;
            projection[2] * m[12] + projection[6] * m[13] + projection[10] * m[14] + projection[14]
        };
        surfaces.sort_by(|a, b| {
            depth(b)
                .total_cmp(&depth(a))
                .then_with(|| a.edge.anchor.cmp(&b.edge.anchor))
        });
        Ok(CameraScene {
            batches,
            failed,
            surfaces,
            projection,
            lighting,
            camera_pose,
            plots,
            shadow_grid: lights.shadow_grid(),
            shadow_views,
            casters,
            caster_instances,
        })
    }
}

pub(super) fn resource<T: 'static>(
    host: &HostRuntime,
    publication: WorldPublicationId,
    key: u64,
) -> Option<&T> {
    host.publication_resource(publication, AssetKey::from_u64(key))?
        .data()?
        .as_any()
        .downcast_ref()
}

fn generated_batch<'a>(
    batches: &mut Vec<Batch<'a>>,
    instances: &mut Vec<Instance>,
    mesh: &MeshBuffers,
    model: [f32; 16],
    tint: [f32; 4],
    camera: CameraUniform,
    custom: Option<crate::custom_mesh::CustomDraw<'a>>,
) {
    let blend = usize::from(custom.as_ref().map_or(tint[3] < 1., |m| m.blended));
    batches.push(Batch {
        blend,
        custom,
        mesh: mesh.clone(),
        pose: mesh.clone(),
        palette: &[],
        texture: None,
        camera,
        first: instances.len(),
        count: 1,
    });
    instances.push(Instance {
        model,
        tint,
        options: [0.; 4],
        surface: [0., 0., 0., 1.],
    });
}
