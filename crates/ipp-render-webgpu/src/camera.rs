//! Prepare reusable Camera bindings and encode an opaque mesh pass.
use crate::{
    device::{Graphics, PALETTE_BYTES},
    scene::{Batch, CameraUniform, Instance},
    service::{FrameSummary, RenderError},
};
struct UniformSlot {
    camera: (wgpu::Buffer, wgpu::BindGroup),
    palette: Option<(wgpu::Buffer, wgpu::BindGroup)>,
    material: Option<MaterialBinding>,
    last_camera: Option<CameraUniform>,
    last_palette: Vec<[f32; 16]>,
}

struct MaterialBinding {
    buffer: wgpu::Buffer,
    group: wgpu::BindGroup,
    layout: wgpu::BindGroupLayout,
    textures: Vec<wgpu::TextureView>,
    values: Vec<[u32; 4]>,
}

#[derive(Default)]
pub(super) struct CameraBuffers {
    instances: Option<wgpu::Buffer>,
    slots: Vec<UniformSlot>,
    commands: crate::camera_commands::CameraCommands,
    previous_instances: Vec<u8>,
}

impl CameraBuffers {
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        gpu: &Graphics,
        white: &wgpu::BindGroup,
        identity: &wgpu::BindGroup,
        batches: &[Batch<'_>],
        instances: &[Instance],
        summary: &mut FrameSummary,
        shadow: bool,
        sampled: &wgpu::BindGroup,
    ) -> Result<wgpu::RenderBundle, RenderError> {
        let instance_values = instances;
        let instances: &[u8] = bytemuck::cast_slice(instances);
        let buffer_limit = gpu.device.limits().max_buffer_size;
        if instances.len() as u64 > buffer_limit {
            return Err(RenderError::Draw(
                "Camera instances exceed the WebGPU buffer limit",
            ));
        }

        if !instances.is_empty() && self.previous_instances != instances {
            if self
                .instances
                .as_ref()
                .is_none_or(|b| b.size() < instances.len() as u64)
            {
                self.instances = Some(
                    gpu.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("IPP instances"),
                        size: (instances.len() as u64)
                            .next_power_of_two()
                            .min(buffer_limit),
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    }),
                );
            }
            self.previous_instances.clear();
            self.previous_instances.extend_from_slice(instances);
            gpu.queue
                .write_buffer(self.instances.as_ref().unwrap(), 0, instances);
        }

        self.slots.resize_with(batches.len(), || UniformSlot {
            camera: gpu.uniform(0, size_of::<CameraUniform>() as u64),
            palette: None,
            material: None,
            last_camera: None,
            last_palette: Vec::new(),
        });
        for (i, (slot, batch)) in self.slots.iter_mut().zip(batches).enumerate() {
            if (i == 0 || batches[i - 1].camera != batch.camera)
                && slot.last_camera.as_ref() != Some(&batch.camera)
            {
                gpu.queue
                    .write_buffer(&slot.camera.0, 0, bytemuck::bytes_of(&batch.camera));
                slot.last_camera = Some(batch.camera);
            }
            if let Some(custom) = &batch.custom {
                let bytes = bytemuck::cast_slice(&custom.values);
                let same_group = slot.material.as_ref().is_some_and(|old| {
                    old.layout == custom.program.layout
                        && old.textures.iter().eq(custom.textures.iter().copied())
                });
                if !same_group {
                    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("Material parameters"),
                        size: bytes.len() as u64,
                        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });

                    let mut entries = vec![wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buffer.as_entire_binding(),
                    }];
                    if !custom.textures.is_empty() {
                        entries.push(wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&custom.program.sampler),
                        });
                    }
                    for (i, view) in custom.textures.iter().enumerate() {
                        entries.push(wgpu::BindGroupEntry {
                            binding: 2 + i as u32,
                            resource: wgpu::BindingResource::TextureView(view),
                        });
                    }
                    let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("Custom mesh"),
                        layout: &custom.program.layout,
                        entries: &entries,
                    });
                    slot.material = Some(MaterialBinding {
                        buffer,
                        group,
                        layout: custom.program.layout.clone(),
                        textures: custom.textures.iter().map(|t| (*t).clone()).collect(),
                        values: Vec::new(),
                    });
                }
                let binding = slot.material.as_mut().unwrap();
                if binding.values != custom.values {
                    gpu.queue.write_buffer(&binding.buffer, 0, bytes);
                    binding.values.clone_from(&custom.values);
                }
            }
            if !batch.palette.is_empty() && slot.last_palette != batch.palette {
                slot.last_palette.clear();
                slot.last_palette.extend_from_slice(batch.palette);
                let (buffer, _) = slot
                    .palette
                    .get_or_insert_with(|| gpu.uniform(2, PALETTE_BYTES));
                gpu.queue
                    .write_buffer(buffer, 0, bytemuck::cast_slice(batch.palette));
            }
        }

        let mut camera_slot = 0;
        for (i, (batch, slot)) in batches.iter().zip(&self.slots).enumerate() {
            if i > 0 && batches[i - 1].camera != batch.camera {
                camera_slot = i;
            }
            let (pipeline, material) = if let Some(custom) = &batch.custom {
                (
                    if shadow {
                        custom
                            .program
                            .shadow
                            .as_ref()
                            .ok_or(RenderError::Draw("Missing custom shadow pipeline"))?
                    } else if batch.mesh.layout == crate::assets::MeshLayout::PaintedPlot {
                        &custom.program.plots[usize::from(custom.blended)]
                    } else {
                        &custom.program.pipelines[usize::from(custom.blended)]
                    },
                    &slot.material.as_ref().unwrap().group,
                )
            } else if shadow {
                (&gpu.shadow_pipeline, &gpu.empty_shadow.bind)
            } else {
                let rigid = batch.blend == 0
                    && instance_values[batch.first..batch.first + batch.count]
                        .iter()
                        .all(Instance::rigid_unlit);
                let pipeline = if batch.mesh.layout == crate::assets::MeshLayout::Plot {
                    &gpu.plot[batch.blend]
                } else if batch.blend > 0 {
                    &gpu.transparent[batch.blend - 1]
                } else if rigid {
                    &gpu.rigid_unlit[usize::from(batch.texture.is_some())]
                } else {
                    &gpu.pipeline
                };
                (pipeline, sampled)
            };
            let joints = if batch.palette.is_empty() {
                identity
            } else {
                &slot.palette.as_ref().unwrap().1
            };
            self.commands
                .next
                .push(crate::camera_commands::MeshCommand {
                    pipeline: pipeline.clone(),
                    groups: [
                        self.slots[camera_slot].camera.1.clone(),
                        batch.texture.unwrap_or(white).clone(),
                        joints.clone(),
                        material.clone(),
                    ],
                    vertices: [
                        batch.mesh.vertices.clone(),
                        batch.pose.vertices.clone(),
                        self.instances.as_ref().unwrap().clone(),
                    ],
                    index: batch.mesh.indices.clone(),
                    offset: (batch.first * size_of::<Instance>()) as u64,
                    indices: batch.mesh.count,
                    instances: batch.count as u32,
                });
            summary.draws += 1;
            summary.triangles = summary
                .triangles
                .saturating_add((batch.mesh.count / 3).saturating_mul(batch.count as u32));
        }
        let format = if shadow {
            wgpu::TextureFormat::R32Float
        } else {
            crate::device::COLOR
        };
        Ok(self.commands.prepare(&gpu.device, format).clone())
    }
}

pub(super) fn draw_shadow(
    encoder: &mut wgpu::CommandEncoder,
    atlas: &crate::device::ShadowAtlas,
    tile: u32,
    grid: u32,
    bundle: &wgpu::RenderBundle,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Spotlight shadow"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &atlas.color,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: if tile == 0 {
                    wgpu::LoadOp::Clear(wgpu::Color::WHITE)
                } else {
                    wgpu::LoadOp::Load
                },
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &atlas.view,
            depth_ops: Some(wgpu::Operations {
                load: if tile == 0 {
                    wgpu::LoadOp::Clear(1.)
                } else {
                    wgpu::LoadOp::Load
                },
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        ..Default::default()
    });
    let size = ipp_render_scene::lighting::SHADOW_TILE;
    pass.set_viewport(
        ((tile % grid) * size) as f32,
        ((tile / grid) * size) as f32,
        size as f32,
        size as f32,
        0.,
        1.,
    );
    pass.execute_bundles(std::iter::once(bundle));
}
