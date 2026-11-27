//! Prepared indexed commands. Resource contents may change while their bindings remain valid.
#[derive(PartialEq)]
pub(super) struct MeshCommand {
    pub pipeline: wgpu::RenderPipeline,
    pub groups: [wgpu::BindGroup; 4],
    pub vertices: [wgpu::Buffer; 3],
    pub index: wgpu::Buffer,
    pub offset: u64,
    pub indices: u32,
    pub instances: u32,
}

#[derive(Default)]
pub(super) struct CameraCommands {
    commands: Vec<MeshCommand>,
    format: Option<wgpu::TextureFormat>,
    bundle: Option<wgpu::RenderBundle>,
    pub next: Vec<MeshCommand>,
}

impl CameraCommands {
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> &wgpu::RenderBundle {
        if self.format != Some(format) || self.commands != self.next {
            let mut encoder =
                device.create_render_bundle_encoder(&wgpu::RenderBundleEncoderDescriptor {
                    label: Some("Indexed Camera draws"),
                    color_formats: &[Some(format)],
                    depth_stencil: Some(wgpu::RenderBundleDepthStencil {
                        format: crate::device::DEPTH,
                        depth_read_only: false,
                        stencil_read_only: true,
                    }),
                    sample_count: 1,
                    multiview: None,
                });
            let mut last = None::<&MeshCommand>;
            for cmd in &self.next {
                if last.is_none_or(|c| c.pipeline != cmd.pipeline) {
                    encoder.set_pipeline(&cmd.pipeline);
                }
                for (i, group) in cmd.groups.iter().enumerate() {
                    if last.is_none_or(|c| c.pipeline != cmd.pipeline || c.groups[i] != *group) {
                        encoder.set_bind_group(i as u32, group, &[]);
                    }
                }
                for (i, buffer) in cmd.vertices.iter().enumerate() {
                    if last.is_none_or(|c| {
                        c.vertices[i] != *buffer || i == 2 && c.offset != cmd.offset
                    }) {
                        let offset = if i == 2 {
                            cmd.offset
                        } else {
                            0
                        };
                        encoder.set_vertex_buffer(i as u32, buffer.slice(offset..));
                    }
                }
                if last.is_none_or(|c| c.index != cmd.index) {
                    encoder.set_index_buffer(cmd.index.slice(..), wgpu::IndexFormat::Uint16);
                }
                encoder.draw_indexed(0..cmd.indices, 0, 0..cmd.instances);
                last = Some(cmd);
            }
            self.bundle = Some(encoder.finish(&wgpu::RenderBundleDescriptor {
                label: Some("Camera draw list"),
            }));
            self.format = Some(format);
            std::mem::swap(&mut self.commands, &mut self.next);
        }
        self.next.clear();
        self.bundle.as_ref().unwrap()
    }
}
