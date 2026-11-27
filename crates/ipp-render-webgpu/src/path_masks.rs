//! Exact framebuffer-sample coverage for repeated projected Plot paths.
use crate::{
    canvas::Draws,
    device::{COLOR, DEPTH, Graphics, Target},
    service::FrameSummary,
};
use ipp_render_scene::path_masks::MAX_PIXELS;

pub(super) struct MaskPipelines {
    write_layout: wgpu::BindGroupLayout,
    write: wgpu::RenderPipeline,
    pub read: wgpu::RenderPipeline,
}

impl MaskPipelines {
    pub fn supported(adapter: &wgpu::Adapter) -> bool {
        let limits = adapter.limits();
        let bytes = u64::from(MAX_PIXELS) * 4;
        limits.max_storage_buffers_per_shader_stage >= 3
            && limits.max_buffer_size >= bytes
            && limits.max_storage_buffer_binding_size >= bytes
            && adapter
                .get_downlevel_capabilities()
                .flags
                .contains(wgpu::DownlevelFlags::FRAGMENT_WRITABLE_STORAGE)
    }

    pub fn new(
        device: &wgpu::Device,
        groups: &[wgpu::BindGroupLayout; 3],
        read_layout: &wgpu::PipelineLayout,
        vertex: &wgpu::ShaderModule,
    ) -> Self {
        let write_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Path coverage output"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage {
                        read_only: false,
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Rasterize path masks"),
            bind_group_layouts: &[Some(&groups[0]), Some(&write_layout), Some(&groups[2])],
            ..Default::default()
        });

        let source = include_str!("path-mask.frag")
            .replace("// IPP_PATH_COVERAGE", include_str!("path-coverage.glsl"));
        let pipeline = |write| {
            let fragment = crate::shaders::shader_module(
                device,
                wgpu::ShaderModuleDescriptor {
                    label: Some("Projected path coverage"),
                    source: wgpu::ShaderSource::Glsl {
                        shader: std::borrow::Cow::Borrowed(&source),
                        stage: wgpu::naga::ShaderStage::Fragment,
                        defines: if write {
                            &[("IPP_WRITE_MASK", "1")]
                        } else {
                            &[]
                        },
                    },
                },
            );
            let colors = [Some(wgpu::ColorTargetState {
                format: COLOR,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })];
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Projected path coverage"),
                layout: Some(if write {
                    &layout
                } else {
                    read_layout
                }),
                vertex: wgpu::VertexState {
                    module: vertex,
                    entry_point: Some("canvas_vertex"),
                    compilation_options: Default::default(),
                    buffers: &[Some(crate::canvas_device::vertex_layout())],
                },
                primitive: Default::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(if write {
                        wgpu::CompareFunction::Always
                    } else {
                        wgpu::CompareFunction::LessEqual
                    }),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &fragment,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    targets: if write {
                        &[]
                    } else {
                        &colors
                    },
                }),
                multiview_mask: None,
                cache: None,
            })
        };

        let write = pipeline(true);
        let read = pipeline(false);
        Self {
            write_layout,
            write,
            read,
        }
    }
}

#[derive(Default)]
pub(super) struct PathMasks {
    paths: Vec<(u32, u32, wgpu::BindGroup)>,
    size: [u32; 2],
    pub read: Option<wgpu::BindGroup>,
}

impl PathMasks {
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        gpu: &Graphics,
        encoder: &mut wgpu::CommandEncoder,
        target: &Target,
        vertices: &wgpu::Buffer,
        paints: &wgpu::BindGroup,
        draws: &Draws,
        changed: bool,
        summary: &mut FrameSummary,
    ) {
        let Some(pipelines) = gpu.canvas.masks.as_ref() else {
            return;
        };
        let size = [target.color.width(), target.color.height()];
        let paths = || draws.runs.iter().filter(|r| r.masked);
        let same = self.paths.len() == paths().count()
            && paths().zip(&self.paths).all(|(r, (a, b, path))| {
                r.start == *a && r.end == *b && Some(path) == r.path.as_ref()
            });

        // Edits and camera motion use the analytic shader immediately. Spend a
        // cache-building pass only once the entire set of projected samples repeats.
        if changed || size != self.size || !same {
            self.paths.clear();
            self.paths
                .extend(paths().map(|r| (r.start, r.end, r.path.as_ref().unwrap().clone())));
            self.read = None;
            self.size = size;
            return;
        }

        if self.read.is_some() {
            return;
        }

        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Projected path samples"),
            size: u64::from(draws.mask_pixels.next_multiple_of(4)) * 4,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let group = |layout| {
            gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Projected path samples"),
                layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            })
        };
        let write = group(&pipelines.write_layout);
        self.read = Some(group(&gpu.canvas.layouts[2]));

        // Build without occluders. The presentation pass applies current scene
        // depth, tint, clipping and blending at the original triangle positions.
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Rasterize unobstructed path samples"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &target.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_pipeline(&pipelines.write);
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.set_bind_group(0, paints, &[]);
        pass.set_bind_group(1, &write, &[]);
        for (start, end, path) in &self.paths {
            pass.set_bind_group(2, path, &[]);
            pass.draw(*start..*end, 0..1);
            summary.draws += 1;
            summary.triangles += (end - start) / 3;
        }
    }
}
